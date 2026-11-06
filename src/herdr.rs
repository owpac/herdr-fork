//! The calls to the herdr CLI.
//!
//! Each herdr command prints JSON. On success, the JSON has a `result`
//! object. On failure, herdr exits with 1 and prints `{"error": {"code",
//! "message"}}` on stderr.

use std::env;
use std::fmt;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use crate::fork::{AgentKind, Placement};

/// A new pane shows its shell prompt after a short time. Until then,
/// `herdr agent start` answers `agent_pane_busy`.
const SHELL_READY_TIMEOUT: Duration = Duration::from_secs(10);
const SHELL_READY_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// A failure of this plugin, with a message for the user.
#[derive(Debug)]
pub enum Error {
    /// herdr refused a command.
    Herdr { code: String, message: String },
    /// A command did not start, or printed an unexpected output.
    Command(String),
    /// The pane has no agent that this plugin can fork.
    NoForkableAgent(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Herdr { code, message } => write!(formatter, "herdr: {code}: {message}"),
            Error::Command(message) => formatter.write_str(message),
            Error::NoForkableAgent(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

/// The agent of a pane, as `herdr agent get` gives it.
#[derive(Debug, Deserialize)]
pub struct Agent {
    pub pane_id: String,
    pub workspace_id: String,
    pub agent: Option<String>,
    pub cwd: Option<PathBuf>,
    pub foreground_cwd: Option<PathBuf>,
    pub agent_session: Option<AgentSession>,
}

/// The session that the herdr integration of the agent reports.
#[derive(Debug, Deserialize)]
pub struct AgentSession {
    pub value: Option<String>,
}

/// A live agent that has a session this plugin can fork.
#[derive(Debug)]
pub struct ForkableAgent {
    pub pane_id: String,
    pub workspace_id: String,
    pub kind: AgentKind,
    pub session_id: String,
    pub folder: PathBuf,
}

/// The herdr executable: herdr gives its path to the plugin commands.
fn herdr_executable() -> PathBuf {
    match env::var_os("HERDR_BIN_PATH") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from("herdr"),
    }
}

/// Run a herdr command and return the `result` of its JSON.
fn run(arguments: &[&str]) -> Result<Value, Error> {
    let output = Command::new(herdr_executable())
        .args(arguments)
        .output()
        .map_err(|error| Error::Command(format!("cannot run herdr: {error}")))?;
    if !output.status.success() {
        return Err(error_of(&output.stderr));
    }
    let json: Value = serde_json::from_slice(&output.stdout).map_err(|error| {
        Error::Command(format!(
            "herdr {}: invalid JSON: {error}",
            arguments.join(" ")
        ))
    })?;
    Ok(json.get("result").cloned().unwrap_or(Value::Null))
}

/// Make the error of a herdr command from its stderr.
fn error_of(stderr: &[u8]) -> Error {
    let text = String::from_utf8_lossy(stderr);
    let parsed: Option<Value> = serde_json::from_str(text.trim()).ok();
    let error = parsed.as_ref().and_then(|json| json.get("error"));
    let field = |name: &str| {
        error
            .and_then(|error| error.get(name))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    match (field("code"), field("message")) {
        (Some(code), Some(message)) => Error::Herdr { code, message },
        _ => Error::Command(format!("herdr failed: {}", text.trim())),
    }
}

/// Read one string field of a JSON object, or fail with a clear message.
fn string_field(json: &Value, pointer: &str) -> Result<String, Error> {
    json.pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Error::Command(format!("herdr gave no {pointer}")))
}

/// Return the agent of `pane_id`, if it has a session that can fork.
pub fn forkable_agent(pane_id: &str) -> Result<ForkableAgent, Error> {
    let no_agent = || {
        Error::NoForkableAgent(format!(
            "The pane {pane_id} has no Claude Code or Codex agent."
        ))
    };
    let result = match run(&["agent", "get", pane_id]) {
        Err(Error::Herdr { code, .. }) if code == "agent_not_found" => return Err(no_agent()),
        other => other?,
    };
    let agent: Agent = serde_json::from_value(result["agent"].clone())
        .map_err(|error| Error::Command(format!("herdr agent get: {error}")))?;
    let kind_name = agent.agent.clone().unwrap_or_default();
    let Some(kind) = AgentKind::from_herdr(&kind_name) else {
        return Err(no_agent());
    };
    // The herdr integration of the agent reports the session. A new
    // session without a message has nothing to fork yet.
    let session_id = agent
        .agent_session
        .and_then(|session| session.value)
        .ok_or_else(|| {
            Error::NoForkableAgent(format!(
                "herdr knows no {kind} session in the pane {pane_id} yet. \
                 Send a first message to the agent. If the problem continues, \
                 install the herdr integration: herdr integration install {kind}"
            ))
        })?;
    let folder = agent
        .foreground_cwd
        .or(agent.cwd)
        .ok_or_else(|| Error::Command(format!("herdr gave no folder for {pane_id}")))?;
    Ok(ForkableAgent {
        pane_id: agent.pane_id,
        workspace_id: agent.workspace_id,
        kind,
        session_id,
        folder,
    })
}

/// Return the names of the live agents.
pub fn agent_names() -> Result<Vec<String>, Error> {
    let result = run(&["agent", "list"])?;
    let agents = result["agents"].as_array().cloned().unwrap_or_default();
    let names = agents
        .iter()
        .filter_map(|agent| agent["name"].as_str())
        .map(str::to_owned)
        .collect();
    Ok(names)
}

/// Open a shell pane for the fork, and return its ID. The pane gets the focus.
pub fn open_shell_pane(
    source: &ForkableAgent,
    placement: Placement,
    label: &str,
) -> Result<String, Error> {
    let folder = source.folder.to_string_lossy();
    let result = match placement {
        Placement::Tab => run(&[
            "tab",
            "create",
            "--workspace",
            &source.workspace_id,
            "--cwd",
            &folder,
            "--label",
            label,
            "--focus",
        ])?,
        Placement::Right | Placement::Down => {
            let direction = if placement == Placement::Right {
                "right"
            } else {
                "down"
            };
            run(&[
                "pane",
                "split",
                "--pane",
                &source.pane_id,
                "--direction",
                direction,
                "--cwd",
                &folder,
                "--focus",
            ])?
        }
    };
    match placement {
        Placement::Tab => string_field(&result, "/root_pane/pane_id"),
        Placement::Right | Placement::Down => string_field(&result, "/pane/pane_id"),
    }
}

/// Start an agent in the shell pane `pane_id`.
///
/// `agent_not_ready` means that the agent runs, but waits for an answer,
/// for example "trust this folder?". The user answers in the pane.
pub fn start_agent(
    name: &str,
    kind: AgentKind,
    pane_id: &str,
    agent_arguments: &[String],
) -> Result<(), Error> {
    let mut arguments = vec![
        "agent",
        "start",
        name,
        "--kind",
        kind.herdr_kind(),
        "--pane",
        pane_id,
        "--",
    ];
    arguments.extend(agent_arguments.iter().map(String::as_str));
    let deadline = Instant::now() + SHELL_READY_TIMEOUT;
    loop {
        match run(&arguments) {
            Ok(_) => return Ok(()),
            Err(Error::Herdr { code, .. }) if code == "agent_not_ready" => return Ok(()),
            Err(Error::Herdr { code, .. })
                if code == "agent_pane_busy" && Instant::now() < deadline =>
            {
                thread::sleep(SHELL_READY_POLL_INTERVAL);
            }
            Err(error) => return Err(error),
        }
    }
}

/// Show a message to the user in a herdr notification.
pub fn notify(message: &str) {
    // A failed notification must not hide the first error: ignore it.
    let _ = run(&["notification", "show", "fork", "--body", message]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_of_reads_the_json_error_of_herdr() {
        let stderr = br#"{"error":{"code":"agent_pane_busy","message":"busy"},"id":"cli"}"#;

        let error = error_of(stderr);

        assert!(matches!(error, Error::Herdr { code, .. } if code == "agent_pane_busy"));
    }

    #[test]
    fn error_of_keeps_a_text_error() {
        let error = error_of(b"usage: herdr agent start");

        assert!(matches!(error, Error::Command(message) if message.contains("usage")));
    }
}
