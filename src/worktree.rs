//! The command `worktree`: fork an agent into the herdr workspace of a git
//! worktree, or start a new agent there with `--brief`.
//!
//! An orchestrator agent calls this command, after `wt switch --create`.
//! The binary of the plugin moves to a new folder at each update: the
//! agent finds it with `herdr plugin list --plugin herdr-fork --json`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config;
use crate::conversation;
use crate::fork::{self, AgentKind, ForkNames};
use crate::herdr::{self, Error, WorkspacePane};
use crate::model::{self, ModelSource};

pub const USAGE: &str = "usage: herdr-fork worktree --path <worktree> \
    (--agent <name|pane> | --brief [--kind claude|codex]) \
    [--name <agent name>] [--model <model>] [--prompt <task>]";

/// The command line of `worktree`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct WorktreeArguments {
    pub path: PathBuf,
    /// The agent to fork: its herdr name or its pane.
    pub agent: Option<String>,
    /// Start a new agent, without the conversation.
    pub is_brief: bool,
    /// The kind of the new agent of `--brief` (default: claude).
    pub kind: Option<AgentKind>,
    /// The herdr name of the new agent.
    pub name: Option<String>,
    /// The model of the new agent. Without it, the agent uses its default
    /// model, not the model of the agent that it forks.
    pub model: Option<String>,
    /// The first prompt of the new agent: its task.
    pub prompt: Option<String>,
}

/// Read the arguments after `worktree`.
pub fn parse_arguments(arguments: &[String]) -> Result<WorktreeArguments, String> {
    let mut parsed = WorktreeArguments::default();
    let mut path = None;
    let mut rest = arguments.iter();
    while let Some(option) = rest.next() {
        if option == "--brief" {
            parsed.is_brief = true;
            continue;
        }
        let value = rest.next().ok_or_else(|| USAGE.to_owned())?.clone();
        match option.as_str() {
            "--path" => path = Some(PathBuf::from(value)),
            "--agent" => parsed.agent = Some(value),
            "--name" => parsed.name = Some(value),
            "--prompt" => parsed.prompt = Some(value),
            "--model" => {
                // The model becomes an argument of the agent: refuse a value
                // that can be an option.
                if !model::is_valid_model(&value) {
                    return Err(format!("invalid model {value:?}"));
                }
                parsed.model = Some(value);
            }
            "--kind" => {
                parsed.kind = Some(
                    AgentKind::from_herdr(&value)
                        .ok_or_else(|| format!("unknown kind {value:?}: use claude or codex"))?,
                );
            }
            _ => return Err(USAGE.to_owned()),
        }
    }
    parsed.path = path.ok_or_else(|| USAGE.to_owned())?;
    // A fork needs the agent to fork. A brief can name the orchestrator:
    // the new agent then inherits its kind and its model.
    if parsed.agent.is_none() && !parsed.is_brief {
        return Err(USAGE.to_owned());
    }
    if parsed.is_brief && parsed.name.is_none() {
        return Err("--brief needs --name: a new agent has no conversation title".to_owned());
    }
    if let Some(name) = &parsed.name {
        if !is_valid_agent_name(name) {
            return Err(format!(
                "invalid agent name {name:?}: use [a-z][a-z0-9_-], 32 characters or less"
            ));
        }
    }
    Ok(parsed)
}

/// Tell if herdr accepts `name` as an agent name: `[a-z][a-z0-9_-]{0,31}`.
pub fn is_valid_agent_name(name: &str) -> bool {
    let mut characters = name.chars();
    let starts_with_letter = characters
        .next()
        .is_some_and(|character| character.is_ascii_lowercase());
    starts_with_letter
        && name.len() <= 32
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'
                || character == '_'
        })
}

/// Where the new agent starts in the workspace of the worktree.
#[derive(Debug, PartialEq, Eq)]
pub enum PaneChoice {
    /// The only pane of the workspace: a shell without agent.
    Use(String),
    /// A new tab of the workspace.
    NewTab,
}

/// Choose the pane of the new agent.
///
/// A workspace that herdr opens for the call has one shell: the agent
/// takes it. The workspace that the post-start hook of wt opens has one
/// shell too. A workspace with an agent or with several panes gets a new
/// tab: the user can use its shells.
pub fn choose_pane(
    is_already_open: bool,
    root_pane_id: &str,
    panes: &[WorkspacePane],
) -> PaneChoice {
    if !is_already_open {
        return PaneChoice::Use(root_pane_id.to_owned());
    }
    match panes {
        [only] if !only.has_agent => PaneChoice::Use(only.pane_id.clone()),
        _ => PaneChoice::NewTab,
    }
}

/// The arguments of a new agent of `--brief`.
fn brief_arguments(kind: AgentKind, title: &str) -> Vec<String> {
    match kind {
        AgentKind::Claude => vec!["--name".to_owned(), title.to_owned()],
        AgentKind::Codex => vec!["--no-daemon".to_owned()],
    }
}

/// Add the model to the arguments of an agent.
fn with_model(model: Option<&str>, mut arguments: Vec<String>) -> Vec<String> {
    if let Some(model) = model {
        arguments.extend(["--model".to_owned(), model.to_owned()]);
    }
    arguments
}

/// The main checkout of the repository of `worktree`: the parent of the
/// common git folder.
fn main_checkout(worktree: &Path) -> Result<PathBuf, Error> {
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .map_err(|error| Error::Command(format!("cannot run git: {error}")))?;
    if !output.status.success() {
        return Err(Error::Command(format!(
            "{} is not in a git repository",
            worktree.display()
        )));
    }
    let common_dir = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    common_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| Error::Command("git gave no main checkout".to_owned()))
}

/// The result of the command, for the agent that calls it.
#[derive(Debug)]
pub struct Started {
    pub agent_name: String,
    pub title: String,
    pub pane_id: String,
    pub workspace_id: String,
    /// `fork` or `brief`.
    pub mode: &'static str,
    /// The model of the agent, or None for its default model.
    pub model: Option<String>,
    pub model_source: ModelSource,
}

/// Fork the agent, or start a new agent, in the workspace of the
/// worktree, then send the task.
pub fn run(arguments: &WorktreeArguments) -> Result<Started, Error> {
    let config = config::load();
    if let Some(message) = config.agentic_mode.refusal(arguments.is_brief) {
        return Err(Error::Command(message));
    }
    let worktree = fs::canonicalize(&arguments.path)
        .map_err(|error| Error::Command(format!("{}: {error}", arguments.path.display())))?;
    let main = main_checkout(&worktree)?;
    if main == worktree {
        return Err(Error::Command(format!(
            "{} is the main checkout, not a linked worktree",
            worktree.display()
        )));
    }
    // The agent of --agent: the agent to fork, or the orchestrator of a
    // brief (its kind and its model).
    let source = match &arguments.agent {
        Some(agent) => Some(herdr::forkable_agent(agent)?),
        None => None,
    };
    let (kind, names, agent_arguments) = match (&source, arguments.is_brief) {
        (Some(source), false) => {
            if conversation::is_missing(source.kind, &source.session_id) {
                return Err(Error::Command(
                    source.kind.empty_session_message(&source.pane_id),
                ));
            }
            let names = match &arguments.name {
                Some(name) => ForkNames {
                    title: name.clone(),
                    agent_name: name.clone(),
                },
                None => fork::fork_names(&source.title, &source.session_id, &herdr::live_agents()?),
            };
            let fork_arguments = source.kind.fork_arguments(&source.session_id, &names.title);
            (source.kind, names, fork_arguments)
        }
        _ => {
            let kind = arguments
                .kind
                .or(source.as_ref().map(|source| source.kind))
                .unwrap_or(AgentKind::Claude);
            let name = arguments.name.clone().unwrap_or_default();
            let names = ForkNames {
                title: name.clone(),
                agent_name: name,
            };
            let new_arguments = brief_arguments(kind, &names.title);
            (kind, names, new_arguments)
        }
    };
    // Only an agent of the same kind gives its model: a Claude model means
    // nothing to Codex.
    let inherited = || {
        source
            .as_ref()
            .filter(|source| source.kind == kind)
            .and_then(|source| conversation::last_model(source.kind, &source.session_id))
    };
    let (model, model_source) =
        model::resolve_model(arguments.model.as_deref(), &config.agentic_model, inherited);
    let agent_arguments = with_model(model.as_deref(), agent_arguments);
    let workspace = herdr::open_worktree(&main, &worktree)?;
    let panes = if workspace.is_already_open {
        herdr::workspace_panes(&workspace.workspace_id)?
    } else {
        Vec::new()
    };
    let choice = choose_pane(workspace.is_already_open, &workspace.root_pane_id, &panes);
    let mut pane_id = match &choice {
        PaneChoice::Use(pane_id) => pane_id.clone(),
        PaneChoice::NewTab => herdr::open_tab(&workspace.workspace_id, &worktree, &names.title)?,
    };
    let started = herdr::start_agent(&names.agent_name, kind, &pane_id, &agent_arguments);
    match started {
        // The shell of the workspace runs a command: take a new tab.
        Err(Error::Herdr { code, .. })
            if code == "agent_pane_busy" && matches!(choice, PaneChoice::Use(_)) =>
        {
            pane_id = herdr::open_tab(&workspace.workspace_id, &worktree, &names.title)?;
            herdr::start_agent(&names.agent_name, kind, &pane_id, &agent_arguments)?;
        }
        other => other?,
    }
    if let Some(prompt) = &arguments.prompt {
        herdr::prompt_agent(&names.agent_name, prompt).map_err(|error| match error {
            Error::Herdr { code, .. } if code == "agent_blocked" => Error::Command(format!(
                "The agent {} waits for an answer in the pane {pane_id}, for example the \
                 trust of a new folder. Answer it, then send the task with: \
                 herdr agent prompt {} \"<task>\"",
                names.agent_name, names.agent_name
            )),
            other => other,
        })?;
    }
    Ok(Started {
        agent_name: names.agent_name,
        title: names.title,
        pane_id,
        workspace_id: workspace.workspace_id,
        mode: if arguments.is_brief { "brief" } else { "fork" },
        model,
        model_source,
    })
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
    fn parse_reads_a_fork() {
        let parsed = parse_arguments(&strings(&[
            "--path",
            "/work/repo.feature",
            "--agent",
            "orch-repo",
            "--prompt",
            "Fix the parser",
        ]))
        .unwrap();

        assert_eq!(parsed.path, PathBuf::from("/work/repo.feature"));
        assert_eq!(parsed.agent.as_deref(), Some("orch-repo"));
        assert_eq!(parsed.prompt.as_deref(), Some("Fix the parser"));
        assert!(!parsed.is_brief);
    }

    #[test]
    fn parse_reads_a_brief() {
        let parsed = parse_arguments(&strings(&[
            "--brief",
            "--kind",
            "codex",
            "--name",
            "task-7a7-3",
            "--path",
            "/work/repo.feature",
        ]))
        .unwrap();

        assert!(parsed.is_brief);
        assert_eq!(parsed.kind, Some(AgentKind::Codex));
        assert_eq!(parsed.name.as_deref(), Some("task-7a7-3"));
    }

    #[test]
    fn parse_needs_a_path_and_one_source() {
        assert!(parse_arguments(&strings(&["--agent", "orch"])).is_err());
        assert!(parse_arguments(&strings(&["--path", "/w"])).is_err());
        // A brief can name its orchestrator, for the kind and the model.
        assert!(parse_arguments(&strings(&[
            "--path", "/w", "--agent", "a", "--brief", "--name", "b"
        ]))
        .is_ok());
    }

    #[test]
    fn a_brief_needs_a_valid_name() {
        assert!(parse_arguments(&strings(&["--path", "/w", "--brief"])).is_err());
        assert!(parse_arguments(&strings(&[
            "--path",
            "/w",
            "--brief",
            "--name",
            "task-7a7.3"
        ]))
        .is_err());
    }

    #[test]
    fn agent_names_follow_the_rule_of_herdr() {
        assert!(is_valid_agent_name("f1-parser_fix"));
        assert!(!is_valid_agent_name("1-parser"));
        assert!(!is_valid_agent_name("Parser"));
        assert!(!is_valid_agent_name("task-7a7.3"));
        assert!(!is_valid_agent_name(&"a".repeat(33)));
    }

    fn pane(pane_id: &str, has_agent: bool) -> WorkspacePane {
        WorkspacePane {
            pane_id: pane_id.to_owned(),
            has_agent,
        }
    }

    #[test]
    fn a_new_workspace_gives_its_root_pane() {
        assert_eq!(
            choose_pane(false, "w2:p1", &[]),
            PaneChoice::Use("w2:p1".to_owned())
        );
    }

    #[test]
    fn an_open_workspace_with_one_shell_gives_this_shell() {
        assert_eq!(
            choose_pane(true, "w2:p1", &[pane("w2:p3", false)]),
            PaneChoice::Use("w2:p3".to_owned())
        );
    }

    #[test]
    fn an_open_workspace_with_an_agent_or_2_panes_gets_a_new_tab() {
        assert_eq!(
            choose_pane(true, "w2:p1", &[pane("w2:p1", true)]),
            PaneChoice::NewTab
        );
        assert_eq!(
            choose_pane(true, "w2:p1", &[pane("w2:p1", false), pane("w2:p2", false)]),
            PaneChoice::NewTab
        );
    }

    #[test]
    fn the_model_comes_after_the_arguments_of_the_agent() {
        assert_eq!(
            with_model(Some("sonnet"), brief_arguments(AgentKind::Claude, "t")),
            strings(&["--name", "t", "--model", "sonnet"])
        );
        assert_eq!(
            with_model(None, brief_arguments(AgentKind::Codex, "t")),
            strings(&["--no-daemon"])
        );
    }

    #[test]
    fn parse_reads_the_model_and_refuses_an_option() {
        let parsed = parse_arguments(&strings(&[
            "--path", "/w", "--agent", "a", "--model", "sonnet",
        ]))
        .unwrap();
        assert_eq!(parsed.model.as_deref(), Some("sonnet"));
        assert!(parse_arguments(&strings(&[
            "--path", "/w", "--agent", "a", "--model", "--x"
        ]))
        .is_err());
    }
}
