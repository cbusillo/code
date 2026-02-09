use crate::error_code::INTERNAL_ERROR_CODE;
use crate::error_code::INVALID_REQUEST_ERROR_CODE;
use code_app_server_protocol::ConfigBatchWriteParams;
use code_app_server_protocol::ConfigEdit;
use code_app_server_protocol::ConfigLayer;
use code_app_server_protocol::ConfigLayerMetadata;
use code_app_server_protocol::ConfigLayerSource;
use code_app_server_protocol::ConfigReadParams;
use code_app_server_protocol::ConfigReadResponse;
use code_app_server_protocol::ConfigRequirementsReadResponse;
use code_app_server_protocol::ConfigValueWriteParams;
use code_app_server_protocol::ConfigWriteErrorCode;
use code_app_server_protocol::ConfigWriteResponse;
use code_app_server_protocol::MergeStrategy;
use code_app_server_protocol::WriteStatus;
use code_app_server_protocol::v2;
use code_core::config::Config;
use code_core::config::ConfigToml;
use code_utils_absolute_path::AbsolutePathBuf;
use code_utils_json_to_toml::json_to_toml;
use mcp_types::JSONRPCErrorError;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use toml::Value as TomlValue;

const CONFIG_FILE_NAME: &str = "config.toml";
const MANAGED_CONFIG_OVERRIDE_ENV: &str = "CODEX_APP_SERVER_MANAGED_CONFIG_PATH";

#[derive(Clone)]
pub(crate) struct ConfigRpc {
    code_home: PathBuf,
}

impl ConfigRpc {
    pub(crate) fn new(config: Arc<Config>) -> Self {
        Self {
            code_home: config.code_home.clone(),
        }
    }

    pub(crate) async fn read(
        &self,
        params: ConfigReadParams,
    ) -> Result<ConfigReadResponse, JSONRPCErrorError> {
        let layers = self.load_layers(params.cwd.as_deref()).await?;

        let mut merged = TomlValue::Table(Default::default());
        let mut origins: HashMap<String, ConfigLayerMetadata> = HashMap::new();
        for layer in &layers {
            merge_toml_values(&mut merged, &layer.config);
            write_origins_for_layer(&mut origins, layer);
        }

        let merged_json = serde_json::to_value(&merged).map_err(internal_error)?;
        let config: v2::Config = serde_json::from_value(merged_json).map_err(internal_error)?;

        let layers = if params.include_layers {
            Some(
                layers
                    .iter()
                    .rev()
                    .map(|layer| {
                        let config = serde_json::to_value(&layer.config).map_err(internal_error)?;
                        Ok(ConfigLayer {
                            name: layer.source.clone(),
                            version: layer.version.clone(),
                            config,
                            disabled_reason: None,
                        })
                    })
                    .collect::<Result<Vec<_>, JSONRPCErrorError>>()?,
            )
        } else {
            None
        };

        Ok(ConfigReadResponse {
            config,
            origins,
            layers,
        })
    }

    pub(crate) async fn write_value(
        &self,
        params: ConfigValueWriteParams,
    ) -> Result<ConfigWriteResponse, JSONRPCErrorError> {
        let response = self
            .write_edits(
                params.file_path,
                vec![ConfigEdit {
                    key_path: params.key_path,
                    value: params.value,
                    merge_strategy: params.merge_strategy,
                }],
                params.expected_version,
            )
            .await?;
        Ok(response)
    }

    pub(crate) async fn batch_write(
        &self,
        params: ConfigBatchWriteParams,
    ) -> Result<ConfigWriteResponse, JSONRPCErrorError> {
        self.write_edits(params.file_path, params.edits, params.expected_version)
            .await
    }

    pub(crate) fn read_requirements(&self) -> ConfigRequirementsReadResponse {
        ConfigRequirementsReadResponse { requirements: None }
    }

    async fn write_edits(
        &self,
        file_path: Option<String>,
        edits: Vec<ConfigEdit>,
        expected_version: Option<String>,
    ) -> Result<ConfigWriteResponse, JSONRPCErrorError> {
        let resolved = self.resolve_target_path(file_path.as_deref())?;
        let file_contents = tokio::fs::read_to_string(&resolved)
            .await
            .unwrap_or_else(|_| String::new());
        let mut config = if file_contents.trim().is_empty() {
            TomlValue::Table(Default::default())
        } else {
            toml::from_str::<TomlValue>(&file_contents)
                .map_err(|err| config_write_error(ConfigWriteErrorCode::ConfigValidationError, err))?
        };

        let current_version = value_version(&config);
        if let Some(expected) = expected_version
            && expected != current_version
        {
            return Err(config_write_error(
                ConfigWriteErrorCode::ConfigVersionConflict,
                "config version conflict",
            ));
        }

        for edit in edits {
            apply_edit(
                &mut config,
                &edit.key_path,
                json_to_toml(edit.value),
                edit.merge_strategy,
            )?;
        }

        let serialized = toml::to_string_pretty(&config).map_err(internal_error)?;
        if let Some(parent) = resolved.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(internal_error)?;
        }
        tokio::fs::write(&resolved, serialized)
            .await
            .map_err(internal_error)?;

        let file_path = AbsolutePathBuf::try_from(resolved).map_err(internal_error)?;
        Ok(ConfigWriteResponse {
            status: WriteStatus::Ok,
            version: value_version(&config),
            file_path,
            overridden_metadata: None,
        })
    }

    async fn load_layers(&self, cwd: Option<&str>) -> Result<Vec<ResolvedLayer>, JSONRPCErrorError> {
        let user_path = self.code_home.join(CONFIG_FILE_NAME);
        let user_value = read_toml_or_empty(&user_path).await?;

        let mut layers: Vec<ResolvedLayer> = Vec::new();

        if cfg!(unix) {
            let system_path = PathBuf::from("/etc/code/config.toml");
            let system_value = read_toml_or_empty(&system_path).await?;
            layers.push(ResolvedLayer::new(
                ConfigLayerSource::System {
                    file: AbsolutePathBuf::from_absolute_path(&system_path).map_err(internal_error)?,
                },
                system_value,
            ));
        }

        layers.push(ResolvedLayer::new(
            ConfigLayerSource::User {
                file: AbsolutePathBuf::from_absolute_path(&user_path).map_err(internal_error)?,
            },
            user_value.clone(),
        ));

        if let Some(cwd) = cwd {
            let project_layers = self.project_layers_for_cwd(cwd, &user_value).await?;
            layers.extend(project_layers);
        }

        if let Some((managed_path, managed_value)) = self.load_managed_layer().await? {
            layers.push(ResolvedLayer::new(
                ConfigLayerSource::LegacyManagedConfigTomlFromFile {
                    file: AbsolutePathBuf::from_absolute_path(&managed_path).map_err(internal_error)?,
                },
                managed_value,
            ));
        }

        Ok(layers)
    }

    async fn project_layers_for_cwd(
        &self,
        cwd: &str,
        user_layer: &TomlValue,
    ) -> Result<Vec<ResolvedLayer>, JSONRPCErrorError> {
        let current_dir = std::env::current_dir().map_err(internal_error)?;
        let resolved_cwd =
            AbsolutePathBuf::resolve_path_against_base(cwd, current_dir).map_err(internal_error)?;
        let user_cfg: ConfigToml = user_layer.clone().try_into().unwrap_or_default();
        if !user_cfg.is_cwd_trusted(resolved_cwd.as_path()) {
            return Ok(vec![]);
        }

        let mut project_dirs = Vec::new();
        let mut cursor = Some(resolved_cwd.to_path_buf());
        while let Some(path) = cursor {
            let dot_codex = path.join(".codex");
            if tokio::fs::metadata(dot_codex.join(CONFIG_FILE_NAME)).await.is_ok() {
                project_dirs.push(dot_codex);
            }
            cursor = path.parent().map(Path::to_path_buf);
        }
        project_dirs.reverse();

        let mut layers = Vec::new();
        for dot_codex in project_dirs {
            let config_path = dot_codex.join(CONFIG_FILE_NAME);
            let config_value = read_toml_or_empty(&config_path).await?;
            layers.push(ResolvedLayer::new(
                ConfigLayerSource::Project {
                    dot_codex_folder: AbsolutePathBuf::from_absolute_path(&dot_codex)
                        .map_err(internal_error)?,
                },
                config_value,
            ));
        }

        Ok(layers)
    }

    async fn load_managed_layer(
        &self,
    ) -> Result<Option<(PathBuf, TomlValue)>, JSONRPCErrorError> {
        let managed_path = if let Ok(path) = std::env::var(MANAGED_CONFIG_OVERRIDE_ENV) {
            PathBuf::from(path)
        } else if cfg!(unix) {
            PathBuf::from("/etc/code/managed_config.toml")
        } else {
            self.code_home.join("managed_config.toml")
        };

        match tokio::fs::read_to_string(&managed_path).await {
            Ok(contents) => {
                let parsed = toml::from_str::<TomlValue>(&contents)
                    .map_err(|err| internal_error(std::io::Error::other(err)))?;
                Ok(Some((managed_path, parsed)))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(internal_error(err)),
        }
    }

    fn resolve_target_path(&self, file_path: Option<&str>) -> Result<PathBuf, JSONRPCErrorError> {
        let absolute = match file_path {
            Some(path) => AbsolutePathBuf::resolve_path_against_base(path, &self.code_home)
                .map_err(internal_error)?,
            None => AbsolutePathBuf::resolve_path_against_base(CONFIG_FILE_NAME, &self.code_home)
                .map_err(internal_error)?,
        };
        Ok(absolute.into_path_buf())
    }
}

#[derive(Clone)]
struct ResolvedLayer {
    source: ConfigLayerSource,
    version: String,
    config: TomlValue,
}

impl ResolvedLayer {
    fn new(source: ConfigLayerSource, config: TomlValue) -> Self {
        let version = value_version(&config);
        Self {
            source,
            version,
            config,
        }
    }
}

async fn read_toml_or_empty(path: &Path) -> Result<TomlValue, JSONRPCErrorError> {
    match tokio::fs::read_to_string(path).await {
        Ok(contents) if contents.trim().is_empty() => Ok(TomlValue::Table(Default::default())),
        Ok(contents) => toml::from_str::<TomlValue>(&contents).map_err(internal_error),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Ok(TomlValue::Table(Default::default()))
        }
        Err(err) => Err(internal_error(err)),
    }
}

fn merge_toml_values(base: &mut TomlValue, overlay: &TomlValue) {
    if let TomlValue::Table(overlay_table) = overlay
        && let TomlValue::Table(base_table) = base
    {
        for (key, value) in overlay_table {
            if let Some(existing) = base_table.get_mut(key) {
                merge_toml_values(existing, value);
            } else {
                base_table.insert(key.clone(), value.clone());
            }
        }
    } else {
        *base = overlay.clone();
    }
}

fn write_origins_for_layer(origins: &mut HashMap<String, ConfigLayerMetadata>, layer: &ResolvedLayer) {
    let mut paths = Vec::new();
    collect_origin_paths(&layer.config, None, &mut paths);
    for path in paths {
        clear_descendant_origins(origins, &path);
        origins.insert(
            path,
            ConfigLayerMetadata {
                name: layer.source.clone(),
                version: layer.version.clone(),
            },
        );
    }
}

fn collect_origin_paths(value: &TomlValue, prefix: Option<&str>, paths: &mut Vec<String>) {
    match value {
        TomlValue::Table(table) => {
            for (key, nested) in table {
                let next = if let Some(prefix) = prefix {
                    format!("{prefix}.{key}")
                } else {
                    key.clone()
                };
                collect_origin_paths(nested, Some(next.as_str()), paths);
            }
        }
        TomlValue::Array(values) => {
            for (index, nested) in values.iter().enumerate() {
                let key = if let Some(prefix) = prefix {
                    format!("{prefix}.{index}")
                } else {
                    index.to_string()
                };
                collect_origin_paths(nested, Some(key.as_str()), paths);
            }
        }
        _ => {
            if let Some(prefix) = prefix {
                paths.push(prefix.to_string());
            }
        }
    }
}

fn clear_descendant_origins(origins: &mut HashMap<String, ConfigLayerMetadata>, path: &str) {
    let prefix = format!("{path}.");
    origins.retain(|key, _| key != path && !key.starts_with(prefix.as_str()));
}

fn apply_edit(
    root: &mut TomlValue,
    key_path: &str,
    value: TomlValue,
    merge_strategy: MergeStrategy,
) -> Result<(), JSONRPCErrorError> {
    let segments: Vec<&str> = key_path.split('.').filter(|segment| !segment.is_empty()).collect();
    if segments.is_empty() {
        return Err(config_write_error(
            ConfigWriteErrorCode::ConfigValidationError,
            "config key path cannot be empty",
        ));
    }
    apply_segments(root, &segments, value, merge_strategy)
}

fn apply_segments(
    node: &mut TomlValue,
    segments: &[&str],
    value: TomlValue,
    merge_strategy: MergeStrategy,
) -> Result<(), JSONRPCErrorError> {
    if !node.is_table() {
        *node = TomlValue::Table(Default::default());
    }

    let Some(table) = node.as_table_mut() else {
        return Err(config_write_error(
            ConfigWriteErrorCode::ConfigValidationError,
            "expected table while applying config edit",
        ));
    };

    if segments.len() == 1 {
        let key = segments[0].to_string();
        match merge_strategy {
            MergeStrategy::Replace => {
                table.insert(key, value);
            }
            MergeStrategy::Upsert => {
                if let Some(existing) = table.get_mut(&key) {
                    merge_toml_values(existing, &value);
                } else {
                    table.insert(key, value);
                }
            }
        }
        return Ok(());
    }

    let entry = table
        .entry(segments[0].to_string())
        .or_insert_with(|| TomlValue::Table(Default::default()));
    if !entry.is_table() {
        *entry = TomlValue::Table(Default::default());
    }
    apply_segments(entry, &segments[1..], value, merge_strategy)
}

fn value_version(value: &TomlValue) -> String {
    let bytes = toml::to_string(value).unwrap_or_default().into_bytes();
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a:{hash:016x}")
}

fn internal_error(err: impl std::fmt::Display) -> JSONRPCErrorError {
    JSONRPCErrorError {
        code: INTERNAL_ERROR_CODE,
        message: err.to_string(),
        data: None,
    }
}

fn config_write_error(code: ConfigWriteErrorCode, message: impl std::fmt::Display) -> JSONRPCErrorError {
    JSONRPCErrorError {
        code: INVALID_REQUEST_ERROR_CODE,
        message: message.to_string(),
        data: Some(json!({ "config_write_error_code": code })),
    }
}
