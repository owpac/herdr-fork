//! The config file of the plugin: `config.toml` in the config folder that
//! herdr gives to the plugin (`herdr plugin config-dir herdr-fork`).
//!
//! ```toml
//! manual_model  = "inherited"  # the forks of the actions right, down and tab
//! agentic_model = "auto"       # the agents of the command worktree
//! agentic_mode  = "auto"       # fork or brief for the command worktree
//! ```

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::model::{ModeSetting, ModelSetting};

/// The options of the plugin.
#[derive(Debug, PartialEq, Eq)]
pub struct Config {
    /// The model of a fork of the actions right, down and tab (default:
    /// `inherited`). `auto` acts as `inherited`: no orchestrator chooses.
    pub manual_model: ModelSetting,
    /// The model of an agent of the command worktree (default: `auto`).
    pub agentic_model: ModelSetting,
    /// Fork or brief for the command worktree (default: `auto`).
    pub agentic_mode: ModeSetting,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            manual_model: ModelSetting::Inherited,
            agentic_model: ModelSetting::Auto,
            agentic_mode: ModeSetting::Auto,
        }
    }
}

/// The config folder that herdr gives. A pane of an agent does not get
/// `HERDR_PLUGIN_CONFIG_DIR`: the plugin then asks herdr, which knows
/// `XDG_CONFIG_HOME` and `HERDR_HOME`. The last fallback is the default
/// folder.
fn config_dir() -> PathBuf {
    if let Some(folder) = env::var_os("HERDR_PLUGIN_CONFIG_DIR") {
        return PathBuf::from(folder);
    }
    let asked = Command::new("herdr")
        .args(["plugin", "config-dir", "herdr-fork"])
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|folder| !folder.is_empty());
    if let Some(folder) = asked {
        return PathBuf::from(folder);
    }
    let mut folder = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    folder.extend([".config", "herdr", "plugins", "config", "herdr-fork"]);
    folder
}

/// Read the config of the plugin. A missing file gives the defaults.
pub fn load() -> Config {
    load_from(&config_dir())
}

fn load_from(folder: &Path) -> Config {
    match fs::read_to_string(folder.join("config.toml")) {
        Ok(text) => parse(&text),
        Err(_) => Config::default(),
    }
}

/// Read the top-level keys that the plugin knows, and ignore the others.
/// A value that the plugin cannot read keeps the default.
fn parse(text: &str) -> Config {
    let mut config = Config::default();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            break;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.split('#').next().unwrap_or_default().trim();
        // A basic string ("...") or a literal string ('...') of TOML.
        let value = value.trim_matches('"').trim_matches('\'');
        let key = key.trim();
        let is_read = match key {
            "manual_model" => ModelSetting::parse(value)
                .map(|setting| config.manual_model = setting)
                .is_some(),
            "agentic_model" => ModelSetting::parse(value)
                .map(|setting| config.agentic_model = setting)
                .is_some(),
            "agentic_mode" => ModeSetting::parse(value)
                .map(|setting| config.agentic_mode = setting)
                .is_some(),
            _ => true,
        };
        if !is_read {
            eprintln!("herdr-fork: config.toml: {key} = {value:?} is not a valid value; the default applies");
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_literal_string_of_toml_is_read() {
        let config = parse("agentic_mode = 'brief'\nagentic_model = 'sonnet' # comment\n");

        assert_eq!(config.agentic_mode, ModeSetting::Brief);
        assert_eq!(
            config.agentic_model,
            ModelSetting::Named("sonnet".to_owned())
        );
    }

    #[test]
    fn an_empty_file_gives_the_defaults() {
        assert_eq!(parse(""), Config::default());
        assert_eq!(load_from(Path::new("/no/such/folder")), Config::default());
    }

    #[test]
    fn the_3_keys_are_read() {
        let text = "manual_model = \"default\"\nagentic_model = \"sonnet\" # code\nagentic_mode = \"brief\"\n";

        assert_eq!(
            parse(text),
            Config {
                manual_model: ModelSetting::Default,
                agentic_model: ModelSetting::Named("sonnet".to_owned()),
                agentic_mode: ModeSetting::Brief,
            }
        );
    }

    #[test]
    fn an_invalid_value_keeps_the_default() {
        let text = "agentic_model = \"--danger\"\nagentic_mode = \"maybe\"\n";

        assert_eq!(parse(text), Config::default());
    }
}
