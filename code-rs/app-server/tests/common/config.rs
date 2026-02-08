use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feature {
    GhostCommit,
    ShellTool,
    UnifiedExec,
    WebSearchRequest,
    WebSearchCached,
    ShellSnapshot,
    RuntimeMetrics,
    Sqlite,
    MemoryTool,
    ChildAgentsMd,
    ApplyPatchFreeform,
    ExecPolicy,
    UseLinuxSandboxBwrap,
    RequestRule,
    WindowsSandbox,
    WindowsSandboxElevated,
    RemoteCompaction,
    RemoteModels,
    PowershellUtf8,
    EnableRequestCompression,
    Collab,
    Apps,
    SkillMcpDependencyInstall,
    SkillEnvVarDependencyPrompt,
    Steer,
    CollaborationModes,
    Personality,
    ResponsesWebsockets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    UnderDevelopment,
    Experimental {
        name: &'static str,
        menu_description: &'static str,
        announcement: &'static str,
    },
    Stable,
    Deprecated,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureSpec {
    pub id: Feature,
    pub key: &'static str,
    pub stage: Stage,
    pub default_enabled: bool,
}

pub const FEATURES: &[FeatureSpec] = &[
    FeatureSpec {
        id: Feature::GhostCommit,
        key: "undo",
        stage: Stage::Stable,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::ShellTool,
        key: "shell_tool",
        stage: Stage::Stable,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::UnifiedExec,
        key: "unified_exec",
        stage: Stage::Stable,
        default_enabled: !cfg!(windows),
    },
    FeatureSpec {
        id: Feature::WebSearchRequest,
        key: "web_search_request",
        stage: Stage::Deprecated,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::WebSearchCached,
        key: "web_search_cached",
        stage: Stage::Deprecated,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::ShellSnapshot,
        key: "shell_snapshot",
        stage: Stage::Experimental {
            name: "Shell snapshot",
            menu_description:
                "Snapshot your shell environment to avoid re-running login scripts for every command.",
            announcement:
                "NEW! Try shell snapshotting to make your Codex faster. Enable in /experimental!",
        },
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::RuntimeMetrics,
        key: "runtime_metrics",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::Sqlite,
        key: "sqlite",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::MemoryTool,
        key: "memory_tool",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::ChildAgentsMd,
        key: "child_agents_md",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::ApplyPatchFreeform,
        key: "apply_patch_freeform",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::ExecPolicy,
        key: "exec_policy",
        stage: Stage::UnderDevelopment,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::UseLinuxSandboxBwrap,
        key: "use_linux_sandbox_bwrap",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::RequestRule,
        key: "request_rule",
        stage: Stage::Stable,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::WindowsSandbox,
        key: "experimental_windows_sandbox",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::WindowsSandboxElevated,
        key: "elevated_windows_sandbox",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::RemoteCompaction,
        key: "remote_compaction",
        stage: Stage::UnderDevelopment,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::RemoteModels,
        key: "remote_models",
        stage: Stage::UnderDevelopment,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::PowershellUtf8,
        key: "powershell_utf8",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::EnableRequestCompression,
        key: "enable_request_compression",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::Collab,
        key: "collab",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::Apps,
        key: "apps",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::SkillMcpDependencyInstall,
        key: "skill_mcp_dependency_install",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::SkillEnvVarDependencyPrompt,
        key: "skill_env_var_dependency_prompt",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::Steer,
        key: "steer",
        stage: Stage::Stable,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::CollaborationModes,
        key: "collaboration_modes",
        stage: Stage::Stable,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::Personality,
        key: "personality",
        stage: Stage::Stable,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::ResponsesWebsockets,
        key: "responses_websockets",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
];

pub fn write_mock_responses_config_toml(
    codex_home: &Path,
    server_uri: &str,
    feature_flags: &BTreeMap<Feature, bool>,
    auto_compact_limit: i64,
    requires_openai_auth: Option<bool>,
    model_provider_id: &str,
    compact_prompt: &str,
) -> std::io::Result<()> {
    // Phase 1: build the features block for config.toml.
    let mut features = BTreeMap::from([(Feature::RemoteModels, false)]);
    for (feature, enabled) in feature_flags {
        features.insert(*feature, *enabled);
    }
    let feature_entries = features
        .into_iter()
        .map(|(feature, enabled)| {
            let key = FEATURES
                .iter()
                .find(|spec| spec.id == feature)
                .map(|spec| spec.key)
                .unwrap_or_else(|| panic!("missing feature key for {feature:?}"));
            format!("{key} = {enabled}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    // Phase 2: build provider-specific config bits.
    let requires_line = match requires_openai_auth {
        Some(true) => "requires_openai_auth = true\n".to_string(),
        Some(false) | None => String::new(),
    };
    let provider_block = if model_provider_id == "openai" {
        String::new()
    } else {
        format!(
            r#"
[model_providers.mock_provider]
name = "Mock provider for test"
base_url = "{server_uri}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
{requires_line}
"#
        )
    };
    // Phase 3: write the final config file.
    let config_toml = codex_home.join("config.toml");
    std::fs::write(
        config_toml,
        format!(
            r#"
model = "mock-model"
approval_policy = "never"
sandbox_mode = "read-only"
compact_prompt = "{compact_prompt}"
model_auto_compact_token_limit = {auto_compact_limit}

model_provider = "{model_provider_id}"

[features]
{feature_entries}
{provider_block}
"#
        ),
    )
}
