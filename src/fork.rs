//! The rules of a fork: where it goes, its name and its command line.
//!
//! This module does not call herdr, so the tests can check each rule.

use std::fmt;
use std::str::FromStr;

/// The longest agent name that herdr accepts: `[a-z][a-z0-9_-]{0,31}`.
const AGENT_NAME_MAX_LENGTH: usize = 32;

/// The number of characters of the session ID in the name of a fork.
const SESSION_ID_PREFIX_LENGTH: usize = 8;

/// The longest title of a conversation in a fork title. herdr refuses a
/// command line that is too long for the terminal.
const MAX_TITLE_CHARACTERS: usize = 80;

/// The longest session ID: a UUID has 36 characters.
const MAX_SESSION_ID_LENGTH: usize = 64;

/// Tell if `session_id` has the form of a session ID of Claude Code or
/// Codex: hexadecimal digits and `-`, never `-` first. Such an ID cannot
/// be an option of the agent.
pub fn is_valid_session_id(session_id: &str) -> bool {
    let has_valid_length = !session_id.is_empty() && session_id.len() <= MAX_SESSION_ID_LENGTH;
    let has_valid_characters = session_id
        .chars()
        .all(|character| character.is_ascii_hexdigit() || character == '-');
    has_valid_length && has_valid_characters && !session_id.starts_with('-')
}

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

    /// The arguments that start a fork of `session_id`, with the title
    /// `title` (Claude Code only: Codex has no session name).
    ///
    /// herdr types `<kind> <arguments>` in the shell of the new pane, so
    /// the shell aliases of the agent apply. The fork gets a new session
    /// ID, and the original conversation does not change.
    pub fn fork_arguments(self, session_id: &str, title: &str) -> Vec<String> {
        match self {
            AgentKind::Claude => vec![
                "--resume".to_owned(),
                session_id.to_owned(),
                "--fork-session".to_owned(),
                "--name".to_owned(),
                title.to_owned(),
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

/// The names of a fork of one conversation.
#[derive(Debug, PartialEq, Eq)]
pub struct ForkNames {
    /// The free title: `[F<n>] <title of the original conversation>`.
    /// Claude Code shows it as the session name, and herdr as the tab
    /// label of a fork in a tab.
    pub title: String,
    /// The herdr agent name: `f<n>-<title in lowercase>`. herdr accepts
    /// only `[a-z][a-z0-9_-]{0,31}`.
    pub agent_name: String,
}

/// A live agent of herdr, as the rules of the names see it.
#[derive(Debug, Default)]
pub struct LiveAgent {
    pub name: Option<String>,
    pub title: Option<String>,
}

/// Make the names of a new fork of the conversation `origin_title`.
///
/// The forks of one conversation get the numbers 1, 2, 3... A fork of a
/// fork belongs to the same conversation: `[F1] Fix the parser` gives
/// `[F2] Fix the parser`. The number is the first one that no live
/// agent uses. If the conversation has no title, the names use the
/// start of the session ID.
pub fn fork_names(origin_title: &str, session_id: &str, live_agents: &[LiveAgent]) -> ForkNames {
    let mut conversation = conversation_title(origin_title);
    if conversation.is_empty() {
        conversation = session_id.chars().take(SESSION_ID_PREFIX_LENGTH).collect();
    }
    let slug = agent_name_slug(&conversation);
    let mut number = 1;
    loop {
        let names = ForkNames {
            title: format!("[F{number}] {conversation}"),
            agent_name: agent_name(number, &slug),
        };
        let is_taken = live_agents.iter().any(|agent| {
            agent.title.as_deref() == Some(names.title.as_str())
                || agent.name.as_deref() == Some(names.agent_name.as_str())
        });
        if !is_taken {
            return names;
        }
        number += 1;
    }
}

/// Remove the `[F<n>] ` prefix of a fork title and the spaces, and cut
/// the title to `MAX_TITLE_CHARACTERS`.
fn conversation_title(title: &str) -> String {
    let title: String = title.trim().chars().take(MAX_TITLE_CHARACTERS).collect();
    let title = title.trim_end();
    let Some(rest) = title.strip_prefix("[F") else {
        return title.to_owned();
    };
    let Some((number, conversation)) = rest.split_once("] ") else {
        return title.to_owned();
    };
    if number.is_empty() || !number.chars().all(|character| character.is_ascii_digit()) {
        return title.to_owned();
    }
    conversation.trim().to_owned()
}

/// The herdr agent name `f<number>-<slug>`, cut to the length limit.
fn agent_name(number: u32, slug: &str) -> String {
    let mut name = format!("f{number}-{slug}");
    name.truncate(AGENT_NAME_MAX_LENGTH);
    name.trim_end_matches('-').to_owned()
}

/// Write text with only the characters of a herdr agent name.
///
/// Each other character, a space or an accented letter for example,
/// becomes one `-`.
fn agent_name_slug(text: &str) -> String {
    let mut slug = String::new();
    for character in text.chars() {
        let lowercase = character.to_ascii_lowercase();
        let is_allowed =
            lowercase.is_ascii_lowercase() || lowercase.is_ascii_digit() || lowercase == '_';
        if is_allowed {
            slug.push(lowercase);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-').to_owned();
    if slug.is_empty() {
        "fork".to_owned()
    } else {
        slug
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
    fn claude_forks_with_resume_and_a_title() {
        let arguments = AgentKind::Claude.fork_arguments("4f7552dc-0000", "[F1] Fix the parser");

        assert_eq!(
            arguments,
            [
                "--resume",
                "4f7552dc-0000",
                "--fork-session",
                "--name",
                "[F1] Fix the parser"
            ]
        );
    }

    #[test]
    fn codex_forks_with_its_fork_command() {
        let arguments = AgentKind::Codex.fork_arguments("019a-session", "[F1] Fix the parser");

        assert_eq!(arguments, ["fork", "019a-session"]);
    }

    fn live_agent(name: &str, title: &str) -> LiveAgent {
        LiveAgent {
            name: Some(name.to_owned()),
            title: Some(title.to_owned()),
        }
    }

    #[test]
    fn the_first_fork_gets_the_number_1() {
        let names = fork_names("Plugins herdr pour agents Claude", "e3ce1810-6f24", &[]);

        assert_eq!(names.title, "[F1] Plugins herdr pour agents Claude");
        assert_eq!(names.agent_name, "f1-plugins-herdr-pour-agents-cla");
    }

    #[test]
    fn a_fork_gets_the_first_free_number() {
        let live = [live_agent("f1-fix-the-parser", "[F1] Fix the parser")];

        let names = fork_names("Fix the parser", "e3ce1810", &live);

        assert_eq!(names.title, "[F2] Fix the parser");
        assert_eq!(names.agent_name, "f2-fix-the-parser");
    }

    #[test]
    fn a_fork_of_a_fork_belongs_to_the_same_conversation() {
        let live = [live_agent("f1-fix-the-parser", "[F1] Fix the parser")];

        let names = fork_names("[F1] Fix the parser", "e3ce1810", &live);

        assert_eq!(names.title, "[F2] Fix the parser");
    }

    #[test]
    fn a_herdr_name_is_enough_to_take_a_number() {
        // The title of a new fork shows only when its agent runs.
        let live = [LiveAgent {
            name: Some("f1-fix-the-parser".to_owned()),
            title: None,
        }];

        let names = fork_names("Fix the parser", "e3ce1810", &live);

        assert_eq!(names.agent_name, "f2-fix-the-parser");
    }

    #[test]
    fn a_conversation_without_title_uses_the_session_id() {
        let names = fork_names("  ", "49f3f4a3-c670-4974", &[]);

        assert_eq!(names.title, "[F1] 49f3f4a3");
        assert_eq!(names.agent_name, "f1-49f3f4a3");
    }

    #[test]
    fn the_herdr_name_replaces_the_other_characters() {
        let names = fork_names("Réparer l'API (v2)", "e3ce1810", &[]);

        assert_eq!(names.title, "[F1] Réparer l'API (v2)");
        assert_eq!(names.agent_name, "f1-r-parer-l-api-v2");
    }

    #[test]
    fn a_uuid_is_a_valid_session_id() {
        assert!(is_valid_session_id("e3ce1810-6f24-4b2c-98e4-08d8964039be"));
    }

    #[test]
    fn an_option_is_not_a_session_id() {
        assert!(!is_valid_session_id("--dangerously-skip-permissions"));
        assert!(!is_valid_session_id("-e3ce1810"));
        assert!(!is_valid_session_id(""));
        assert!(!is_valid_session_id("e3ce1810 6f24"));
    }

    #[test]
    fn a_title_without_space_after_the_prefix_stays_complete() {
        let names = fork_names("[F1]", "e3ce1810", &[]);

        assert_eq!(names.title, "[F1] [F1]");
    }

    #[test]
    fn a_long_title_is_cut() {
        let long_title = "a".repeat(200);

        let names = fork_names(&long_title, "e3ce1810", &[]);

        assert_eq!(
            names.title.chars().count(),
            "[F1] ".len() + MAX_TITLE_CHARACTERS
        );
    }

    #[test]
    fn a_two_digit_number_keeps_the_herdr_limit() {
        let conversation = "x".repeat(60);
        let live: Vec<LiveAgent> = (1..10)
            .map(|number| LiveAgent {
                name: None,
                title: Some(format!("[F{number}] {conversation}")),
            })
            .collect();

        let names = fork_names(&conversation, "e3ce1810", &live);

        assert!(names.title.starts_with("[F10] "));
        assert!(names.agent_name.starts_with("f10-"));
        assert!(names.agent_name.len() <= AGENT_NAME_MAX_LENGTH);
    }
}
