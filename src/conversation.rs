//! The conversation files of the agents on the disk.
//!
//! Claude Code gives the session ID to the herdr hook at the start of the
//! session, but it writes the conversation only after the first message.
//! Until then, `claude --resume <session ID>` finds no conversation.
//!
//! Codex has no such problem: the herdr hook of Codex gives the session
//! ID only after the first message.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::fork::AgentKind;

/// Tell if the session `session_id` of the agent `kind` has no
/// conversation on the disk yet.
///
/// The answer is `false` when the plugin cannot know: the fork then
/// starts, and the agent gives its own error.
pub fn is_missing(kind: AgentKind, session_id: &str) -> bool {
    match kind {
        AgentKind::Claude => is_missing_in_claude_projects(&claude_projects_folders(), session_id),
        AgentKind::Codex => false,
    }
}

/// The folders of the Claude Code conversations: `projects` in
/// `CLAUDE_CONFIG_DIR`, and in `~/.claude`.
///
/// The plugin runs with the environment of herdr, not with the
/// environment of the agent. The agent can use `~/.claude` when herdr has
/// a `CLAUDE_CONFIG_DIR`, so the plugin looks in the 2 folders.
fn claude_projects_folders() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if let Some(config_folder) = env::var_os("CLAUDE_CONFIG_DIR") {
        if !config_folder.is_empty() {
            folders.push(PathBuf::from(config_folder).join("projects"));
        }
    }
    if let Some(home) = env::var_os("HOME") {
        folders.push(PathBuf::from(home).join(".claude").join("projects"));
    }
    folders
}

/// Tell if no project folder of `projects_folders` has the conversation
/// file `<session_id>.jsonl`.
///
/// Claude Code keeps the conversations of each folder in its own project
/// folder. The plugin looks in each project folder: the name of a project
/// folder is not always the path of the folder with `-`. If no folder of
/// `projects_folders` can be read, the answer is `false`.
fn is_missing_in_claude_projects(projects_folders: &[PathBuf], session_id: &str) -> bool {
    let file_name = format!("{session_id}.jsonl");
    let mut has_readable_folder = false;
    for projects_folder in projects_folders {
        let Ok(project_folders) = fs::read_dir(projects_folder) else {
            continue;
        };
        has_readable_folder = true;
        let is_found = project_folders
            .flatten()
            .any(|project_folder| project_folder.path().join(&file_name).is_file());
        if is_found {
            return false;
        }
    }
    has_readable_folder
}

/// Return the model of the last turn of the session, or None if the
/// plugin cannot read it. A fork with the setting `inherited` uses it.
pub fn last_model(kind: AgentKind, session_id: &str) -> Option<String> {
    match kind {
        AgentKind::Claude => {
            let path = find_claude_conversation(&claude_projects_folders(), session_id)?;
            last_claude_model(&fs::read_to_string(path).ok()?)
        }
        AgentKind::Codex => {
            let path = find_codex_rollout(&codex_sessions_folder()?, session_id, 0)?;
            last_codex_model(&fs::read_to_string(path).ok()?)
        }
    }
}

/// Find the conversation file `<session_id>.jsonl` of Claude Code.
fn find_claude_conversation(projects_folders: &[PathBuf], session_id: &str) -> Option<PathBuf> {
    let file_name = format!("{session_id}.jsonl");
    projects_folders.iter().find_map(|projects_folder| {
        fs::read_dir(projects_folder)
            .ok()?
            .flatten()
            .map(|project_folder| project_folder.path().join(&file_name))
            .find(|path| path.is_file())
    })
}

/// The folder of the Codex sessions: `sessions` in `CODEX_HOME`, else in
/// `~/.codex`.
fn codex_sessions_folder() -> Option<PathBuf> {
    match env::var_os("CODEX_HOME") {
        Some(home) if !home.is_empty() => Some(PathBuf::from(home).join("sessions")),
        _ => Some(
            PathBuf::from(env::var_os("HOME")?)
                .join(".codex")
                .join("sessions"),
        ),
    }
}

/// The deepest folder level of the Codex sessions: `YYYY/MM/DD`.
const CODEX_SESSION_DEPTH: usize = 3;

/// Find the file `rollout-<time>-<session_id>.jsonl` under `folder`.
fn find_codex_rollout(folder: &Path, session_id: &str, depth: usize) -> Option<PathBuf> {
    let suffix = format!("-{session_id}.jsonl");
    for entry in fs::read_dir(folder).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() && depth < CODEX_SESSION_DEPTH {
            if let Some(found) = find_codex_rollout(&path, session_id, depth + 1) {
                return Some(found);
            }
        } else if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(&suffix))
        {
            return Some(path);
        }
    }
    None
}

/// The model of the last answer in a conversation file of Claude Code.
/// The function reads the lines from the end, and stops at the first
/// model.
fn last_claude_model(text: &str) -> Option<String> {
    text.lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| entry["type"] == "assistant")
        .filter_map(|entry| entry["message"]["model"].as_str().map(str::to_owned))
        .find(|model| model != "<synthetic>")
}

/// The model of the last turn in a session file of Codex.
fn last_codex_model(text: &str) -> Option<String> {
    text.lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| entry["type"] == "turn_context")
        .find_map(|entry| entry["payload"]["model"].as_str().map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_claude_model_skips_the_synthetic_answers() {
        let text = concat!(
            r#"{"type":"user","message":{"content":"hi"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"model":"claude-opus-5-5"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"model":"claude-sonnet-5-5"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"model":"<synthetic>"}}"#,
            "\n",
            "not json\n",
        );

        assert_eq!(last_claude_model(text).as_deref(), Some("claude-sonnet-5-5"));
    }

    #[test]
    fn the_last_codex_model_comes_from_the_turn_context() {
        let text = concat!(
            r#"{"type":"session_meta","payload":{"id":"x"}}"#,
            "\n",
            r#"{"type":"turn_context","payload":{"model":"gpt-a"}}"#,
            "\n",
            r#"{"type":"turn_context","payload":{"model":"gpt-b"}}"#,
            "\n",
        );

        assert_eq!(last_codex_model(text).as_deref(), Some("gpt-b"));
        assert_eq!(last_codex_model("{}\n"), None);
    }

    /// A new empty folder for one test, in the temporary folder.
    fn empty_folder(test_name: &str) -> PathBuf {
        let folder = env::temp_dir().join(format!("herdr-fork-{test_name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn a_conversation_file_in_a_project_folder_is_found() {
        let projects = empty_folder("found");
        fs::create_dir(projects.join("-Users-me-test")).unwrap();
        fs::write(projects.join("-Users-me-test/f429d85f.jsonl"), "{}\n").unwrap();

        let is_missing = is_missing_in_claude_projects(std::slice::from_ref(&projects), "f429d85f");
        fs::remove_dir_all(&projects).unwrap();

        assert!(!is_missing);
    }

    #[test]
    fn a_session_without_a_conversation_file_is_missing() {
        let projects = empty_folder("missing");
        fs::create_dir(projects.join("-Users-me-test")).unwrap();
        fs::write(projects.join("-Users-me-test/0000aaaa.jsonl"), "{}\n").unwrap();

        let is_missing = is_missing_in_claude_projects(std::slice::from_ref(&projects), "f429d85f");
        fs::remove_dir_all(&projects).unwrap();

        assert!(is_missing);
    }

    #[test]
    fn a_folder_that_cannot_be_read_is_not_missing() {
        let parent = empty_folder("unreadable");
        let projects = parent.join("no-such-folder");

        let is_missing = is_missing_in_claude_projects(std::slice::from_ref(&projects), "f429d85f");
        fs::remove_dir_all(&parent).unwrap();

        assert!(!is_missing);
    }

    #[test]
    fn a_conversation_file_in_the_second_folder_is_found() {
        let parent = empty_folder("second");
        let config_projects = parent.join("config/projects");
        let home_projects = parent.join("home/projects");
        fs::create_dir_all(&config_projects).unwrap();
        fs::create_dir_all(home_projects.join("-Users-me-test")).unwrap();
        fs::write(home_projects.join("-Users-me-test/f429d85f.jsonl"), "{}\n").unwrap();

        let is_missing =
            is_missing_in_claude_projects(&[config_projects, home_projects], "f429d85f");
        fs::remove_dir_all(&parent).unwrap();

        assert!(!is_missing);
    }

    #[test]
    fn a_codex_session_is_never_missing() {
        assert!(!is_missing(AgentKind::Codex, "019a-session"));
    }
}
