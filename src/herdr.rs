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

use crate::fork::{self, AgentKind, LiveAgent, Placement};

/// A new pane shows its shell prompt after a short time. Until then,
/// `herdr agent start` answers `agent_pane_busy`. herdr 0.8.2 and later
/// already try again for 2 seconds: a slow shell can need more time.
const SHELL_READY_TIMEOUT: Duration = Duration::from_secs(10);
const SHELL_READY_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// The time that `herdr agent start` waits for the agent (default: 30 s).
/// The resume of a long session with MCP servers can take more time.
const AGENT_START_TIMEOUT_MILLISECONDS: &str = "120000";

/// A failure of this plugin, with a message for the user.
#[derive(Debug)]
pub enum Error {
    /// herdr refused a command.
    Herdr { code: String, message: String },
    /// A command did not start, printed an unexpected output, or the pane
    /// has no agent that this plugin can fork.
    Command(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Herdr { code, message } => write!(formatter, "herdr: {code}: {message}"),
            Error::Command(message) => formatter.write_str(message),
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
    /// The terminal title, without the status sign of the agent.
    pub terminal_title_stripped: Option<String>,
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
    /// The title of the conversation: Claude Code shows it in the
    /// terminal title.
    pub title: String,
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
        Error::Command(format!(
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
    // The herdr integration of the agent reports the session. Codex
    // reports it only after the first message.
    let session_id = agent
        .agent_session
        .and_then(|session| session.value)
        .ok_or_else(|| Error::Command(kind.missing_session_message(pane_id)))?;
    // The session ID becomes an argument of the agent: refuse an ID that
    // can be an option (a program with access to the herdr socket can
    // report any session ID).
    if !fork::is_valid_session_id(&session_id) {
        return Err(Error::Command(format!(
            "herdr gave an invalid {kind} session ID for the pane {pane_id}."
        )));
    }
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
        title: agent.terminal_title_stripped.unwrap_or_default(),
    })
}

/// Return the names and the titles of the live agents.
pub fn live_agents() -> Result<Vec<LiveAgent>, Error> {
    let result = run(&["agent", "list"])?;
    let agents = result["agents"].as_array().cloned().unwrap_or_default();
    let live_agents = agents
        .iter()
        .map(|agent| LiveAgent {
            name: agent["name"].as_str().map(str::to_owned),
            title: agent["terminal_title_stripped"].as_str().map(str::to_owned),
        })
        .collect();
    Ok(live_agents)
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
        "--timeout",
        AGENT_START_TIMEOUT_MILLISECONDS,
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

/// Close a pane.
pub fn close_pane(pane_id: &str) -> Result<(), Error> {
    run(&["pane", "close", pane_id])?;
    Ok(())
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
