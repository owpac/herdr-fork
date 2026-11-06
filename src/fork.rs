//! The rules of a fork: where it goes, its name and its command line.
//!
//! This module does not call herdr, so the tests can check each rule.

use std::fmt;
use std::str::FromStr;

/// The longest agent name that herdr accepts: `[a-z][a-z0-9_-]{0,31}`.
const AGENT_NAME_MAX_LENGTH: usize = 32;

/// The number of characters of the session ID in the name of a fork.
const SESSION_ID_PREFIX_LENGTH: usize = 8;

/// Where the fork opens, next to the original conversation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// A new split on the right of the pane.
    Right,
    /// A new split below the pane.
    Down,
    /// A new tab in the workspace of the pane.
    Tab,
}

impl FromStr for Placement {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "right" => Ok(Placement::Right),
            "down" => Ok(Placement::Down),
            "tab" => Ok(Placement::Tab),
            other => Err(format!(
                "unknown placement {other:?}: use right, down or tab"
            )),
        }
    }
}

/// An agent that can fork its session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Codex,
}

impl AgentKind {
    /// The agent kind of herdr (`herdr agent list`), if this plugin can fork it.
    pub fn from_herdr(kind: &str) -> Option<Self> {
        match kind {
            "claude" => Some(AgentKind::Claude),
            "codex" => Some(AgentKind::Codex),
            _ => None,
        }
    }

    /// The `--kind` value of `herdr agent start`.
    pub fn herdr_kind(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
        }
    }

    /// The arguments that start a fork of `session_id`.
    ///
    /// herdr types `<kind> <arguments>` in the shell of the new pane, so
    /// the shell aliases of the agent apply. The fork gets a new session
    /// ID, and the original conversation does not change.
    pub fn fork_arguments(self, session_id: &str, name: &str) -> Vec<String> {
        match self {
            AgentKind::Claude => vec![
                "--resume".to_owned(),
                session_id.to_owned(),
                "--fork-session".to_owned(),
                "--name".to_owned(),
                name.to_owned(),
            ],
            AgentKind::Codex => vec!["fork".to_owned(), session_id.to_owned()],
        }
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.herdr_kind())
    }
}

/// The herdr name of a fork: `fork-<start of the session ID>`.
///
/// If a live agent already has this name, the name gets the first free
/// suffix `-2`, `-3`... The name always matches `[a-z][a-z0-9_-]{0,31}`.
pub fn fork_name(session_id: &str, taken_names: &[String]) -> String {
    let session_prefix: String = session_id
        .chars()
        .take(SESSION_ID_PREFIX_LENGTH)
        .map(agent_name_character)
        .collect();
    let base = format!("fork-{session_prefix}");
    let mut name = base.clone();
    let mut suffix = 2;
    while taken_names.contains(&name) {
        name = format!("{base}-{suffix}");
        suffix += 1;
    }
    name.truncate(AGENT_NAME_MAX_LENGTH);
    name
}

/// Replace a character that herdr refuses in an agent name with `-`.
fn agent_name_character(character: char) -> char {
    let lowercase = character.to_ascii_lowercase();
    if lowercase.is_ascii_lowercase() || lowercase.is_ascii_digit() || lowercase == '_' {
        lowercase
    } else {
        '-'
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_reads_the_three_names() {
        assert_eq!("right".parse(), Ok(Placement::Right));
        assert_eq!("down".parse(), Ok(Placement::Down));
        assert_eq!("tab".parse(), Ok(Placement::Tab));
    }

    #[test]
    fn placement_refuses_an_unknown_name() {
        assert!("left".parse::<Placement>().is_err());
    }

    #[test]
    fn only_claude_and_codex_can_fork() {
        assert_eq!(AgentKind::from_herdr("claude"), Some(AgentKind::Claude));
        assert_eq!(AgentKind::from_herdr("codex"), Some(AgentKind::Codex));
        assert_eq!(AgentKind::from_herdr("pi"), None);
    }

    #[test]
    fn claude_forks_with_resume_and_a_name() {
        let arguments = AgentKind::Claude.fork_arguments("4f7552dc-0000", "fork-4f7552dc");

        assert_eq!(
            arguments,
            [
                "--resume",
                "4f7552dc-0000",
                "--fork-session",
                "--name",
                "fork-4f7552dc"
            ]
        );
    }

    #[test]
    fn codex_forks_with_its_fork_command() {
        let arguments = AgentKind::Codex.fork_arguments("019a-session", "fork-019a-ses");

        assert_eq!(arguments, ["fork", "019a-session"]);
    }

    #[test]
    fn fork_name_uses_the_start_of_the_session() {
        let name = fork_name("49f3f4a3-c670-4974", &[]);

        assert_eq!(name, "fork-49f3f4a3");
    }

    #[test]
    fn fork_name_adds_the_first_free_suffix() {
        let taken = vec!["fork-49f3f4a3".to_owned(), "fork-49f3f4a3-2".to_owned()];

        let name = fork_name("49f3f4a3-c670-4974", &taken);

        assert_eq!(name, "fork-49f3f4a3-3");
    }

    #[test]
    fn fork_name_replaces_the_characters_that_herdr_refuses() {
        let name = fork_name("AB.c/9_x", &[]);

        assert_eq!(name, "fork-ab-c-9_x");
    }
}
