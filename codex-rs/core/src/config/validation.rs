use super::ConfigToml;

pub(super) fn upgrade_legacy_model_slugs(cfg: &mut ConfigToml) {
    maybe_upgrade(&mut cfg.model);
    maybe_upgrade(&mut cfg.review_model);

    for profile in cfg.profiles.values_mut() {
        maybe_upgrade(&mut profile.model);
    }
}

pub(super) fn upgrade_legacy_model_slug(slug: &str) -> Option<&'static str> {
    if slug.eq_ignore_ascii_case("claude-sonnet-4.5") {
        return Some("claude-sonnet-4.6");
    }

    if slug.eq_ignore_ascii_case("gemini-2.5-pro")
        || slug.eq_ignore_ascii_case("gemini-3-pro")
        || slug.eq_ignore_ascii_case("gemini-3-pro-preview")
    {
        return Some("gemini-3.1-pro-preview");
    }

    if slug.eq_ignore_ascii_case("gemini-2.5-flash")
        || slug.eq_ignore_ascii_case("gemini-3-flash")
    {
        return Some("gemini-3-flash-preview");
    }

    if slug.eq_ignore_ascii_case("qwen-3-coder") {
        return Some("qwen3-coder-plus");
    }

    None
}

fn maybe_upgrade(field: &mut Option<String>) {
    let Some(existing) = field.as_deref() else {
        return;
    };
    let Some(new) = upgrade_legacy_model_slug(existing) else {
        return;
    };

    tracing::info!(
        target: "codex.config",
        old = existing,
        new,
        "upgrading legacy provider model slug"
    );
    *field = Some(new.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::config::ConfigOverrides;
    use crate::config::profile::ConfigProfile;
    use std::collections::HashMap;
    use tempfile::tempdir;

    #[test]
    fn upgrade_legacy_model_slug_updates_provider_aliases() {
        assert_eq!(
            upgrade_legacy_model_slug("claude-sonnet-4.5"),
            Some("claude-sonnet-4.6")
        );
        assert_eq!(
            upgrade_legacy_model_slug("gemini-2.5-pro"),
            Some("gemini-3.1-pro-preview")
        );
        assert_eq!(
            upgrade_legacy_model_slug("gemini-3-pro"),
            Some("gemini-3.1-pro-preview")
        );
        assert_eq!(
            upgrade_legacy_model_slug("gemini-3-pro-preview"),
            Some("gemini-3.1-pro-preview")
        );
        assert_eq!(
            upgrade_legacy_model_slug("gemini-2.5-flash"),
            Some("gemini-3-flash-preview")
        );
        assert_eq!(
            upgrade_legacy_model_slug("gemini-3-flash"),
            Some("gemini-3-flash-preview")
        );
        assert_eq!(
            upgrade_legacy_model_slug("qwen-3-coder"),
            Some("qwen3-coder-plus")
        );
    }

    #[test]
    fn upgrade_legacy_model_slug_leaves_current_and_unknown_slugs_alone() {
        assert_eq!(upgrade_legacy_model_slug("claude-sonnet-4.6"), None);
        assert_eq!(upgrade_legacy_model_slug("my-custom-model"), None);
    }

    #[test]
    fn upgrade_legacy_model_slugs_updates_top_level_model_fields() {
        let mut cfg = ConfigToml {
            model: Some("claude-sonnet-4.5".to_string()),
            review_model: Some("gemini-3-flash".to_string()),
            ..ConfigToml::default()
        };

        upgrade_legacy_model_slugs(&mut cfg);

        assert_eq!(cfg.model.as_deref(), Some("claude-sonnet-4.6"));
        assert_eq!(cfg.review_model.as_deref(), Some("gemini-3-flash-preview"));
    }

    #[test]
    fn upgrade_legacy_model_slugs_updates_profile_models() {
        let mut cfg = ConfigToml {
            profiles: HashMap::from([(
                "provider".to_string(),
                ConfigProfile {
                    model: Some("qwen-3-coder".to_string()),
                    ..ConfigProfile::default()
                },
            )]),
            ..ConfigToml::default()
        };

        upgrade_legacy_model_slugs(&mut cfg);

        assert_eq!(
            cfg.profiles
                .get("provider")
                .and_then(|profile| profile.model.as_deref()),
            Some("qwen3-coder-plus")
        );
    }

    #[test]
    fn upgrade_legacy_model_slugs_is_noop_for_current_values() {
        let mut cfg = ConfigToml {
            model: Some("claude-sonnet-4.6".to_string()),
            review_model: Some("gpt-5.4".to_string()),
            ..ConfigToml::default()
        };

        upgrade_legacy_model_slugs(&mut cfg);

        assert_eq!(cfg.model.as_deref(), Some("claude-sonnet-4.6"));
        assert_eq!(cfg.review_model.as_deref(), Some("gpt-5.4"));
    }

    #[test]
    fn load_config_upgrades_legacy_provider_model_slugs() {
        let codex_home = tempdir().expect("tempdir");
        let config = Config::load_from_base_config_with_overrides(
            ConfigToml {
                model: Some("claude-sonnet-4.5".to_string()),
                review_model: Some("gemini-3-pro".to_string()),
                ..ConfigToml::default()
            },
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        )
        .expect("load config");

        assert_eq!(config.model.as_deref(), Some("claude-sonnet-4.6"));
        assert_eq!(config.review_model.as_deref(), Some("gemini-3.1-pro-preview"));
    }
}
