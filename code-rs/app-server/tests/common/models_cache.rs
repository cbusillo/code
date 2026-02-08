use chrono::DateTime;
use chrono::Utc;
use code_common::model_presets::ModelPreset as CommonModelPreset;
use code_common::model_presets::ReasoningEffortPreset as CommonReasoningEffortPreset;
use code_common::model_presets::all_model_presets;
use code_core::protocol_config_types::ReasoningEffort as ConfigReasoningEffort;
use code_protocol::openai_models::ConfigShellToolType;
use code_protocol::openai_models::ModelInfo;
use code_protocol::openai_models::ModelInfoUpgrade;
use code_protocol::openai_models::ModelPreset;
use code_protocol::openai_models::ModelVisibility;
use code_protocol::openai_models::ReasoningEffort;
use code_protocol::openai_models::ReasoningEffortPreset;
use code_protocol::openai_models::TruncationPolicyConfig;
use code_protocol::openai_models::default_input_modalities;
use serde_json::json;
use std::path::Path;

const DEFAULT_MODEL_IDS: &[&str] = &[
    "gpt-5.2-codex",
    "gpt-5.1-codex-max",
    "gpt-5.1-codex-mini",
    "gpt-5.2",
];

fn map_reasoning_effort(effort: ConfigReasoningEffort) -> ReasoningEffort {
    match effort {
        ConfigReasoningEffort::Minimal => ReasoningEffort::Minimal,
        ConfigReasoningEffort::Low => ReasoningEffort::Low,
        ConfigReasoningEffort::Medium => ReasoningEffort::Medium,
        ConfigReasoningEffort::High => ReasoningEffort::High,
        ConfigReasoningEffort::XHigh => ReasoningEffort::XHigh,
        ConfigReasoningEffort::None => ReasoningEffort::None,
    }
}

fn map_reasoning_preset(preset: &CommonReasoningEffortPreset) -> ReasoningEffortPreset {
    ReasoningEffortPreset {
        effort: map_reasoning_effort(preset.effort),
        description: preset.description.clone(),
    }
}

/// Convert a ModelPreset to ModelInfo for cache storage.
fn common_preset_to_protocol(preset: &CommonModelPreset) -> ModelPreset {
    ModelPreset {
        id: preset.id.clone(),
        model: preset.model.clone(),
        display_name: preset.display_name.clone(),
        description: preset.description.clone(),
        default_reasoning_effort: map_reasoning_effort(preset.default_reasoning_effort),
        supported_reasoning_efforts: preset
            .supported_reasoning_efforts
            .iter()
            .map(map_reasoning_preset)
            .collect(),
        supports_personality: false,
        is_default: preset.is_default,
        upgrade: preset.upgrade.as_ref().map(|u| code_protocol::openai_models::ModelUpgrade {
            id: u.id.clone(),
            reasoning_effort_mapping: u.reasoning_effort_mapping.as_ref().map(|mapping| {
                mapping
                    .iter()
                    .map(|(k, v)| (map_reasoning_effort(*k), map_reasoning_effort(*v)))
                    .collect()
            }),
            migration_config_key: u.migration_config_key.clone(),
            model_link: None,
            upgrade_copy: None,
            migration_markdown: None,
        }),
        show_in_picker: preset.show_in_picker,
        supported_in_api: true,
        input_modalities: default_input_modalities(),
    }
}

fn preset_to_info(preset: &CommonModelPreset, priority: i32) -> ModelInfo {
    let preset = common_preset_to_protocol(preset);
    ModelInfo {
        slug: preset.id.clone(),
        display_name: preset.display_name.clone(),
        description: Some(preset.description.clone()),
        default_reasoning_level: Some(preset.default_reasoning_effort),
        supported_reasoning_levels: preset.supported_reasoning_efforts.clone(),
        shell_type: ConfigShellToolType::ShellCommand,
        visibility: if preset.show_in_picker {
            ModelVisibility::List
        } else {
            ModelVisibility::Hide
        },
        supported_in_api: preset.supported_in_api,
        priority,
        upgrade: preset.upgrade.as_ref().map(ModelInfoUpgrade::from),
        base_instructions: "base instructions".to_string(),
        model_messages: None,
        supports_reasoning_summaries: false,
        support_verbosity: false,
        default_verbosity: None,
        apply_patch_tool_type: None,
        truncation_policy: TruncationPolicyConfig::bytes(10_000),
        supports_parallel_tool_calls: false,
        context_window: Some(272_000),
        auto_compact_token_limit: None,
        effective_context_window_percent: 95,
        experimental_supported_tools: Vec::new(),
        input_modalities: default_input_modalities(),
    }
}

/// Write a models_cache.json file to the codex home directory.
/// This prevents ModelsManager from making network requests to refresh models.
/// The cache will be treated as fresh (within TTL) and used instead of fetching from the network.
/// Uses the built-in model presets from ModelsManager, converted to ModelInfo format.
pub fn write_models_cache(codex_home: &Path) -> std::io::Result<()> {
    // Get all presets and filter for show_in_picker (same as builtin_model_presets does)
    let presets_by_id = all_model_presets()
        .iter()
        .filter(|preset| preset.show_in_picker)
        .map(|preset| (preset.id.as_str(), preset))
        .collect::<std::collections::HashMap<_, _>>();
    let presets: Vec<&CommonModelPreset> = DEFAULT_MODEL_IDS
        .iter()
        .map(|id| {
            presets_by_id
                .get(id)
                .copied()
                .unwrap_or_else(|| panic!("missing model preset for {id}"))
        })
        .collect();
    // Convert presets to ModelInfo, assigning priorities (lower = earlier in list).
    // Priority is used for sorting, so the first model gets the lowest priority.
    let models: Vec<ModelInfo> = presets
        .iter()
        .enumerate()
        .map(|(idx, preset)| {
            // Lower priority = earlier in list.
            let priority = idx as i32;
            preset_to_info(preset, priority)
        })
        .collect();

    write_models_cache_with_models(codex_home, models)
}

/// Write a models_cache.json file with specific models.
/// Useful when tests need specific models to be available.
pub fn write_models_cache_with_models(
    codex_home: &Path,
    models: Vec<ModelInfo>,
) -> std::io::Result<()> {
    let cache_path = codex_home.join("models_cache.json");
    // DateTime<Utc> serializes to RFC3339 format by default with serde
    let fetched_at: DateTime<Utc> = Utc::now();
    let client_version = format!(
        "{}.{}.{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );
    let cache = json!({
        "fetched_at": fetched_at,
        "etag": null,
        "client_version": client_version,
        "models": models
    });
    std::fs::write(cache_path, serde_json::to_string_pretty(&cache)?)
}
