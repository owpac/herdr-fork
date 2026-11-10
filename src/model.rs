//! The choice of the model and of the mode (fork or brief) of a new agent.
//!
//! This module does not call herdr, so the tests can check each rule.

use crate::fork::AgentKind;

use std::fmt;

/// The setting of a model in the config of the plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelSetting {
    /// The model of the agent that the new agent forks.
    Inherited,
    /// The orchestrator chooses with `--model`. Without `--model`: as
    /// `Inherited`.
    Auto,
    /// The default model of Claude Code or Codex.
    Default,
    /// This model, for example `sonnet`.
    Named(String),
}

impl ModelSetting {
    /// Read a value of the config, or None if the value is not valid.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "inherited" => Some(ModelSetting::Inherited),
            "auto" => Some(ModelSetting::Auto),
            "default" => Some(ModelSetting::Default),
            other if is_valid_model(other) => Some(ModelSetting::Named(other.to_owned())),
            _ => None,
        }
    }
}

/// Tell if `model` can be an argument of the agent: not empty, and not an
/// option (a value that starts with `-`).
pub fn is_valid_model(model: &str) -> bool {
    !model.is_empty()
        && !model.starts_with('-')
        && model
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-._:/[]".contains(character))
}

/// The setting of the mode of the command worktree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeSetting {
    /// The orchestrator chooses: `--agent` for a fork, `--brief` for a brief.
    Auto,
    /// Only forks.
    Fork,
    /// Only briefs.
    Brief,
}

impl ModeSetting {
    /// Read a value of the config, or None if the value is not valid.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(ModeSetting::Auto),
            "fork" => Some(ModeSetting::Fork),
            "brief" => Some(ModeSetting::Brief),
            _ => None,
        }
    }

    /// Return an error message if the setting refuses the mode that the
    /// orchestrator asked for.
    pub fn refusal(self, is_brief: bool) -> Option<String> {
        match (self, is_brief) {
            (ModeSetting::Fork, true) => Some(
                "agentic_mode = \"fork\": start a fork of your conversation (--agent, without --brief)."
                    .to_owned(),
            ),
            (ModeSetting::Brief, false) => Some(
                "agentic_mode = \"brief\": start a new agent with --brief, and write the context of the task in Beads."
                    .to_owned(),
            ),
            _ => None,
        }
    }
}

/// Where the model of a new agent comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelSource {
    /// The option `--model`.
    Option,
    /// A model name in the config.
    Setting,
    /// The model of the forked agent.
    Inherited,
    /// The default model of the agent.
    Default,
}

impl fmt::Display for ModelSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            ModelSource::Option => "option",
            ModelSource::Setting => "setting",
            ModelSource::Inherited => "inherited",
            ModelSource::Default => "default",
        })
    }
}

/// Choose the model of a new agent.
///
/// `--model` comes first, then the setting. `inherited` (and `auto`
/// without `--model`) reads the model of the forked agent: if the plugin
/// cannot read it, the agent uses its default model. A model of the other
/// kind of agent (`--model sonnet` for Codex) acts as inherited: the agent
/// cannot start with it.
pub fn resolve_model(
    option: Option<&str>,
    setting: &ModelSetting,
    kind: AgentKind,
    inherited: impl FnOnce() -> Option<String>,
) -> (Option<String>, ModelSource) {
    match option {
        Some(model) if fits_kind(model, kind) => {
            return (Some(model.to_owned()), ModelSource::Option);
        }
        Some(_) => {
            return match inherited() {
                Some(model) if is_valid_model(&model) && fits_kind(&model, kind) => {
                    (Some(model), ModelSource::Inherited)
                }
                _ => (None, ModelSource::Default),
            };
        }
        None => {}
    }
    match setting {
        ModelSetting::Named(model) if fits_kind(model, kind) => {
            (Some(model.clone()), ModelSource::Setting)
        }
        // A model of the other kind of agent (sonnet for Codex): the
        // setting acts as inherited.
        ModelSetting::Named(_) => match inherited() {
            Some(model) if is_valid_model(&model) => (Some(model), ModelSource::Inherited),
            _ => (None, ModelSource::Default),
        },
        ModelSetting::Default => (None, ModelSource::Default),
        ModelSetting::Inherited | ModelSetting::Auto => match inherited() {
            Some(model) if is_valid_model(&model) => (Some(model), ModelSource::Inherited),
            _ => (None, ModelSource::Default),
        },
    }
}

/// Return false if the name of `model` belongs to the other kind of
/// agent: a Claude alias or ID for Codex, a GPT or Codex model for Claude.
/// An unknown name fits both kinds.
fn fits_kind(model: &str, kind: AgentKind) -> bool {
    let name = model.to_ascii_lowercase();
    let is_claude = name.starts_with("claude")
        || ["sonnet", "opus", "haiku", "fable", "best", "opusplan"]
            .iter()
            .any(|alias| name == *alias || name.starts_with(&format!("{alias}[")));
    let is_codex = name.starts_with("gpt") || name.contains("codex");
    match kind {
        AgentKind::Claude => !is_codex,
        AgentKind::Codex => !is_claude,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_model_of_the_other_kind_acts_as_inherited() {
        let setting = ModelSetting::Named("sonnet".to_owned());

        assert_eq!(
            resolve_model(None, &setting, AgentKind::Codex, || Some(
                "gpt-6.1".to_owned()
            )),
            (Some("gpt-6.1".to_owned()), ModelSource::Inherited)
        );
        assert_eq!(
            resolve_model(None, &setting, AgentKind::Claude, never),
            (Some("sonnet".to_owned()), ModelSource::Setting)
        );
        assert!(fits_kind("opus[1m]", AgentKind::Claude));
        assert!(!fits_kind("opus[1m]", AgentKind::Codex));
        assert!(!fits_kind("gpt-6.1-codex", AgentKind::Claude));
        assert!(fits_kind("my-gateway-model", AgentKind::Codex));
    }

    fn never() -> Option<String> {
        panic!("the session must not be read");
    }

    #[test]
    fn an_option_of_the_other_kind_acts_as_inherited() {
        let setting = ModelSetting::Auto;

        assert_eq!(
            resolve_model(Some("sonnet"), &setting, AgentKind::Codex, || Some(
                "gpt-6.1".to_owned()
            )),
            (Some("gpt-6.1".to_owned()), ModelSource::Inherited)
        );
        assert_eq!(
            resolve_model(Some("sonnet"), &setting, AgentKind::Codex, || None),
            (None, ModelSource::Default)
        );
        assert_eq!(
            resolve_model(Some("gpt-6.1-codex"), &setting, AgentKind::Claude, || Some(
                "opus".to_owned()
            )),
            (Some("opus".to_owned()), ModelSource::Inherited)
        );
    }

    #[test]
    fn the_option_comes_first() {
        let (model, source) = resolve_model(
            Some("sonnet"),
            &ModelSetting::Named("sonnet".to_owned()),
            AgentKind::Claude,
            never,
        );

        assert_eq!(
            (model.as_deref(), source),
            (Some("sonnet"), ModelSource::Option)
        );
    }

    #[test]
    fn a_named_setting_gives_its_model() {
        let (model, source) = resolve_model(
            None,
            &ModelSetting::Named("sonnet".to_owned()),
            AgentKind::Claude,
            never,
        );

        assert_eq!(
            (model.as_deref(), source),
            (Some("sonnet"), ModelSource::Setting)
        );
    }

    #[test]
    fn inherited_and_auto_read_the_forked_session() {
        for setting in [ModelSetting::Inherited, ModelSetting::Auto] {
            let (model, source) = resolve_model(None, &setting, AgentKind::Claude, || {
                Some("claude-sonnet-5-5".to_owned())
            });

            assert_eq!(
                (model.as_deref(), source),
                (Some("claude-sonnet-5-5"), ModelSource::Inherited)
            );
        }
    }

    #[test]
    fn an_unknown_or_invalid_inherited_model_gives_the_default() {
        assert_eq!(
            resolve_model(None, &ModelSetting::Inherited, AgentKind::Claude, || None),
            (None, ModelSource::Default)
        );
        assert_eq!(
            resolve_model(None, &ModelSetting::Inherited, AgentKind::Claude, || Some(
                "--x".to_owned()
            )),
            (None, ModelSource::Default)
        );
        assert_eq!(
            resolve_model(None, &ModelSetting::Default, AgentKind::Claude, never),
            (None, ModelSource::Default)
        );
    }

    #[test]
    fn the_settings_read_their_values() {
        assert_eq!(
            ModelSetting::parse("claude-opus-5-5"),
            Some(ModelSetting::Named("claude-opus-5-5".to_owned()))
        );
        assert_eq!(ModelSetting::parse("--x"), None);
        assert_eq!(ModelSetting::parse(""), None);
        assert_eq!(ModeSetting::parse("brief"), Some(ModeSetting::Brief));
        assert_eq!(ModeSetting::parse("maybe"), None);
    }

    #[test]
    fn a_fixed_mode_refuses_the_other_mode() {
        assert!(ModeSetting::Fork.refusal(true).is_some());
        assert!(ModeSetting::Brief.refusal(false).is_some());
        assert!(ModeSetting::Fork.refusal(false).is_none());
        assert!(ModeSetting::Auto.refusal(true).is_none());
    }
}
