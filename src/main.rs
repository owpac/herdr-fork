//! herdr-fork: fork the Claude Code or Codex session of the focused pane.
//!
//! Usage: `herdr-fork right|down|tab [--pane <pane_id>]`, or `herdr-fork worktree ...`
//!
//! herdr runs this command for the actions of the plugin. The focused
//! pane comes from `HERDR_PLUGIN_CONTEXT_JSON`. Give `--pane` to fork
//! another pane, for example from a shell.
//!
//! `herdr-fork message` is the command of the popup `message`: it shows
//! the message of `HERDR_FORK_MESSAGE`.
//!
//! `herdr-fork worktree --path <worktree> --agent <name|pane> ...` forks
//! an agent into the herdr workspace of a git worktree (see `worktree`).
//! It prints a JSON object, and shows no popup: an agent calls it.

mod conversation;
mod fork;
mod herdr;
mod ui;
mod worktree;

use std::env;
use std::process::ExitCode;

use fork::Placement;
use herdr::Error;

const USAGE: &str = "usage: herdr-fork right|down|tab [--pane <pane_id>]";

/// The title of the popup `message`.
const MESSAGE_TITLE: &str = "Fork";

/// The command line of the plugin.
struct Arguments {
    placement: Placement,
    pane_id: Option<String>,
}

fn parse_arguments(arguments: &[String]) -> Result<Arguments, String> {
    let Some(placement) = arguments.first() else {
        return Err(USAGE.to_owned());
    };
    let placement = placement.parse()?;
    let pane_id = match &arguments[1..] {
        [] => None,
        [option, pane_id] if option == "--pane" => Some(pane_id.clone()),
        _ => return Err(USAGE.to_owned()),
    };
    Ok(Arguments { placement, pane_id })
}

/// The pane that has the focus when the user runs the action.
fn focused_pane_id() -> Result<String, Error> {
    let context = env::var("HERDR_PLUGIN_CONTEXT_JSON").map_err(|_| {
        Error::Command(
            "No herdr action context: run this command from herdr, or give --pane.".to_owned(),
        )
    })?;
    let context: serde_json::Value = serde_json::from_str(&context)
        .map_err(|error| Error::Command(format!("Invalid herdr action context: {error}")))?;
    context["focused_pane_id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::Command("The herdr action context has no focused pane.".to_owned()))
}

/// Fork the agent of the pane into a new pane, and return the fork title.
fn fork(arguments: &Arguments) -> Result<String, Error> {
    let pane_id = match &arguments.pane_id {
        Some(pane_id) => pane_id.clone(),
        None => focused_pane_id()?,
    };
    let source = herdr::forkable_agent(&pane_id)?;
    // Do not open a pane for a fork that cannot start.
    if conversation::is_missing(source.kind, &source.session_id) {
        return Err(Error::Command(
            source.kind.empty_session_message(&source.pane_id),
        ));
    }
    let names = fork::fork_names(&source.title, &source.session_id, &herdr::live_agents()?);
    let shell_pane_id = herdr::open_shell_pane(&source, arguments.placement, &names.title)?;
    let fork_arguments = source.kind.fork_arguments(&source.session_id, &names.title);
    let started = herdr::start_agent(
        &names.agent_name,
        source.kind,
        &shell_pane_id,
        &fork_arguments,
    );
    if let Err(error) = started {
        // Do not leave an empty pane with the focus. A failed close must
        // not hide the first error.
        let _ = herdr::close_pane(&shell_pane_id);
        return Err(error);
    }
    Ok(names.title)
}

/// The command `worktree`: print the new agent as JSON, or the error on
/// stderr. No popup: an agent calls this command, not a key of herdr.
fn fork_into_worktree(command_line: &[String]) -> ExitCode {
    let arguments = match worktree::parse_arguments(command_line) {
        Ok(arguments) => arguments,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    match worktree::run(&arguments) {
        Ok(started) => {
            let json = serde_json::json!({
                "agent": started.agent_name,
                "title": started.title,
                "pane_id": started.pane_id,
                "workspace_id": started.workspace_id,
            });
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("herdr-fork: {error}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let command_line: Vec<String> = env::args().skip(1).collect();
    if command_line == ["message"] {
        let message = env::var(herdr::MESSAGE_VARIABLE).unwrap_or_default();
        ui::show_message(MESSAGE_TITLE, &message);
        return ExitCode::SUCCESS;
    }
    if command_line.first().map(String::as_str) == Some("worktree") {
        return fork_into_worktree(&command_line[1..]);
    }
    let arguments = match parse_arguments(&command_line) {
        Ok(arguments) => arguments,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    match fork(&arguments) {
        Ok(name) => {
            println!("{name}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("herdr-fork: {error}");
            herdr::show_message(&error.to_string());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(arguments: &[&str]) -> Vec<String> {
        arguments
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect()
    }

    #[test]
    fn parse_reads_a_placement() {
        let arguments = parse_arguments(&strings(&["tab"])).unwrap();

        assert_eq!(arguments.placement, Placement::Tab);
        assert_eq!(arguments.pane_id, None);
    }

    #[test]
    fn parse_reads_the_pane_option() {
        let arguments = parse_arguments(&strings(&["right", "--pane", "w1:p2"])).unwrap();

        assert_eq!(arguments.pane_id.as_deref(), Some("w1:p2"));
    }

    #[test]
    fn parse_refuses_an_unknown_option() {
        assert!(parse_arguments(&strings(&["right", "--focus"])).is_err());
    }

    #[test]
    fn parse_refuses_an_empty_command_line() {
        assert!(parse_arguments(&[]).is_err());
    }
}
