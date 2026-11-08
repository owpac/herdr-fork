//! The choice of the model and of the mode (fork or brief) of a new agent.
//!
//! This module does not call herdr, so the tests can check each rule.

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
/// cannot read it, the agent uses its default model.
pub fn resolve_model(
    option: Option<&str>,
    setting: &ModelSetting,
    inherited: impl FnOnce() -> Option<String>,
) -> (Option<String>, ModelSource) {
    if let Some(model) = option {
        return (Some(model.to_owned()), ModelSource::Option);
    }
    match setting {
        ModelSetting::Named(model) => (Some(model.clone()), ModelSource::Setting),
        ModelSetting::Default => (None, ModelSource::Default),
        ModelSetting::Inherited | ModelSetting::Auto => match inherited() {
            Some(model) if is_valid_model(&model) => (Some(model), ModelSource::Inherited),
            _ => (None, ModelSource::Default),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn never() -> Option<String> {
        panic!("the session must not be read");
    }

    #[test]
    fn the_option_comes_first() {
        let (model, source) = resolve_model(
            Some("haiku"),
            &ModelSetting::Named("sonnet".to_owned()),
            never,
        );

        assert_eq!(
            (model.as_deref(), source),
            (Some("haiku"), ModelSource::Option)
        );
    }

    #[test]
    fn a_named_setting_gives_its_model() {
        let (model, source) = resolve_model(None, &ModelSetting::Named("sonnet".to_owned()), never);

        assert_eq!(
            (model.as_deref(), source),
            (Some("sonnet"), ModelSource::Setting)
        );
    }

    #[test]
    fn inherited_and_auto_read_the_forked_session() {
        for setting in [ModelSetting::Inherited, ModelSetting::Auto] {
            let (model, source) =
                resolve_model(None, &setting, || Some("claude-haiku-5-5".to_owned()));

            assert_eq!(
                (model.as_deref(), source),
                (Some("claude-haiku-5-5"), ModelSource::Inherited)
            );
        }
    }

    #[test]
    fn an_unknown_or_invalid_inherited_model_gives_the_default() {
        assert_eq!(
            resolve_model(None, &ModelSetting::Inherited, || None),
            (None, ModelSource::Default)
        );
        assert_eq!(
            resolve_model(None, &ModelSetting::Inherited, || Some("--x".to_owned())),
            (None, ModelSource::Default)
        );
        assert_eq!(
            resolve_model(None, &ModelSetting::Default, never),
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
