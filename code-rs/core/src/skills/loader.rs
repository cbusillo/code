use crate::config::Config;
use crate::config::SkillConfigRuleSelector;
use crate::config::resolve_code_path_for_read;
use crate::git_info::resolve_root_git_project_for_trust;
use crate::skills::model::SkillCommand;
use crate::skills::model::SkillCommandMatcher;
use crate::skills::model::SkillCommandPolicy;
use crate::skills::model::SkillCommandPolicyAction;
use crate::skills::model::SkillCommandPolicyPreferred;
use crate::skills::model::SkillCommandPolicyPreferredKind;
use crate::skills::model::SkillError;
use crate::skills::model::SkillLoadOutcome;
use crate::skills::model::SkillMetadata;
use crate::skills::model::SkillPolicy;
use crate::skills::model::SkillResource;
use crate::skills::model::SkillResourceKind;
use crate::skills::model::SkillScope;
use crate::skills::model::SkillWorkflowDefault;
use crate::skills::system::system_cache_root_dir;
use crate::skills::system::install_system_skills;
use dunce::canonicalize as normalize_path;
use serde::Deserialize;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use tracing::error;

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
    #[serde(default)]
    metadata: Option<SkillFrontmatterMetadata>,
    #[serde(default)]
    resources: Vec<SkillFrontmatterResource>,
    #[serde(default)]
    commands: Vec<SkillFrontmatterCommand>,
    #[serde(default)]
    workflow_defaults: Vec<SkillFrontmatterWorkflowDefault>,
    #[serde(default)]
    policy: Option<SkillFrontmatterPolicy>,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatterMetadata {
    #[serde(default, rename = "short-description")]
    short_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatterResource {
    path: PathBuf,
    kind: SkillFrontmatterResourceKind,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SkillFrontmatterResourceKind {
    Script,
    Reference,
    Template,
    Asset,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatterCommand {
    name: String,
    resource_path: PathBuf,
    #[serde(default)]
    example_argv: Vec<String>,
    purpose: String,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatterWorkflowDefault {
    name: String,
    value: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatterPolicy {
    #[serde(default)]
    allow_implicit_invocation: Option<bool>,
    #[serde(default)]
    command_policies: Vec<SkillFrontmatterCommandPolicy>,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatterCommandPolicy {
    id: String,
    #[serde(rename = "match")]
    matcher: SkillFrontmatterCommandMatcher,
    action: SkillFrontmatterCommandPolicyAction,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    preferred: Vec<SkillFrontmatterCommandPolicyPreferred>,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatterCommandMatcher {
    #[serde(default)]
    argv_exact: Option<Vec<String>>,
    #[serde(default)]
    argv_prefix: Option<Vec<String>>,
    #[serde(default)]
    shell_regex: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SkillFrontmatterCommandPolicyAction {
    RequirePreferred,
    RequireConfirm,
    Reject,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatterCommandPolicyPreferred {
    kind: SkillFrontmatterCommandPolicyPreferredKind,
    #[serde(default)]
    path: Option<PathBuf>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    example_argv: Vec<String>,
    #[serde(default)]
    purpose: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SkillFrontmatterCommandPolicyPreferredKind {
    Script,
    Skill,
    Command,
}

const SKILLS_FILENAME: &str = "SKILL.md";
const SKILLS_DIR_NAME: &str = "skills";
const AGENTS_DIR_NAME: &str = ".agents";
const REPO_ROOT_CONFIG_DIR_NAME: &str = ".codex";
const ADMIN_SKILLS_ROOT: &str = "/etc/codex/skills";
const MAX_NAME_LEN: usize = 64;
const MAX_DESCRIPTION_LEN: usize = 1024;
const MAX_SHORT_DESCRIPTION_LEN: usize = 160;
const MAX_COMMAND_POLICY_ID_LEN: usize = 96;
const MAX_COMMAND_POLICY_MESSAGE_LEN: usize = 512;
const MAX_COMMAND_POLICY_TOKEN_LEN: usize = 160;
const MAX_COMMAND_POLICY_REGEX_LEN: usize = 512;
const MAX_COMMAND_POLICY_PURPOSE_LEN: usize = 256;
const MAX_COMMAND_POLICIES_PER_SKILL: usize = 64;
const MAX_COMMAND_POLICY_PREFERRED: usize = 8;
const MAX_COMMAND_POLICY_ARGV_TOKENS: usize = 32;
const MAX_STRUCTURED_ITEMS_PER_SKILL: usize = 64;
const MAX_STRUCTURED_ID_LEN: usize = 96;
const MAX_STRUCTURED_PURPOSE_LEN: usize = 256;
const MAX_STRUCTURED_VALUE_LEN: usize = 512;

#[derive(Debug)]
enum SkillParseError {
    Read(std::io::Error),
    MissingFrontmatter,
    InvalidYaml(serde_yaml::Error),
    MissingField(&'static str),
    InvalidField { field: &'static str, reason: String },
}

impl fmt::Display for SkillParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkillParseError::Read(e) => write!(f, "failed to read file: {e}"),
            SkillParseError::MissingFrontmatter => {
                write!(f, "missing YAML frontmatter delimited by ---")
            }
            SkillParseError::InvalidYaml(e) => write!(f, "invalid YAML: {e}"),
            SkillParseError::MissingField(field) => write!(f, "missing field `{field}`"),
            SkillParseError::InvalidField { field, reason } => {
                write!(f, "invalid {field}: {reason}")
            }
        }
    }
}

impl Error for SkillParseError {}

pub fn load_skills(config: &Config) -> SkillLoadOutcome {
    if let Err(err) = install_system_skills(&config.code_home) {
        tracing::error!("failed to install system skills: {err}");
    }
    let mut outcome = load_skills_from_roots(skill_roots(config));
    apply_skill_config_rules(&mut outcome.skills, config);
    outcome
}

fn apply_skill_config_rules(skills: &mut [SkillMetadata], config: &Config) {
    for rule in &config.skills.config {
        match &rule.selector {
            SkillConfigRuleSelector::Name(name) => {
                for skill in skills.iter_mut().filter(|skill| skill.name == *name) {
                    set_allow_implicit_invocation(skill, rule.enabled);
                }
            }
            SkillConfigRuleSelector::Path(path) => {
                for skill in skills.iter_mut().filter(|skill| skill.path == *path) {
                    set_allow_implicit_invocation(skill, rule.enabled);
                }
            }
        }
    }
}

fn set_allow_implicit_invocation(skill: &mut SkillMetadata, enabled: bool) {
    let policy = skill.policy.get_or_insert_with(SkillPolicy::default);
    policy.allow_implicit_invocation = Some(enabled);
}

pub(crate) struct SkillRoot {
    pub(crate) path: PathBuf,
    pub(crate) scope: SkillScope,
}

pub(crate) fn load_skills_from_roots<I>(roots: I) -> SkillLoadOutcome
where
    I: IntoIterator<Item = SkillRoot>,
{
    let mut outcome = SkillLoadOutcome::default();
    for root in roots {
        discover_skills_under_root(&root.path, root.scope, &mut outcome);
    }

    let mut seen: HashSet<String> = HashSet::new();
    outcome
        .skills
        .retain(|skill| seen.insert(skill.name.clone()));

    outcome
        .skills
        .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));

    outcome
}

pub(crate) fn user_skills_root(config: &Config) -> SkillRoot {
    let root = resolve_code_path_for_read(&config.code_home, Path::new(SKILLS_DIR_NAME));
    SkillRoot {
        path: root,
        scope: SkillScope::User,
    }
}

pub(crate) fn home_agents_skills_root() -> Option<SkillRoot> {
    let home = dirs::home_dir()?;
    Some(SkillRoot {
        path: home.join(AGENTS_DIR_NAME).join(SKILLS_DIR_NAME),
        scope: SkillScope::User,
    })
}

pub(crate) fn system_skills_root(config: &Config) -> SkillRoot {
    SkillRoot {
        path: system_cache_root_dir(&config.code_home),
        scope: SkillScope::System,
    }
}

pub(crate) fn admin_skills_root() -> SkillRoot {
    SkillRoot {
        path: PathBuf::from(ADMIN_SKILLS_ROOT),
        scope: SkillScope::Admin,
    }
}

fn repo_search_dirs(cwd: &Path) -> Vec<PathBuf> {
    let Some(base) = (if cwd.is_dir() { Some(cwd) } else { cwd.parent() }) else {
        return Vec::new();
    };
    let base = normalize_path(base).unwrap_or_else(|_| base.to_path_buf());

    let repo_root =
        resolve_root_git_project_for_trust(&base).map(|root| normalize_path(&root).unwrap_or(root));

    if let Some(repo_root) = repo_root.as_deref() {
        let mut dirs = Vec::new();
        for dir in base.ancestors() {
            dirs.push(dir.to_path_buf());

            if dir == repo_root {
                break;
            }
        }
        return dirs;
    }

    vec![base]
}

fn repo_skills_roots_for_config_dir(cwd: &Path, config_dir: &str) -> Vec<SkillRoot> {
    repo_search_dirs(cwd)
        .into_iter()
        .map(|dir| dir.join(config_dir).join(SKILLS_DIR_NAME))
        .filter(|path| path.is_dir())
        .map(|path| SkillRoot {
            path,
            scope: SkillScope::Repo,
        })
        .collect()
}

fn dedupe_skill_roots_by_path(roots: &mut Vec<SkillRoot>) {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    roots.retain(|root| {
        let normalized = normalize_path(&root.path).unwrap_or_else(|_| root.path.clone());
        seen.insert(normalized)
    });
}

fn skill_roots(config: &Config) -> Vec<SkillRoot> {
    let mut roots = Vec::new();

    roots.extend(repo_skills_roots_for_config_dir(&config.cwd, AGENTS_DIR_NAME));
    roots.extend(repo_skills_roots_for_config_dir(
        &config.cwd,
        REPO_ROOT_CONFIG_DIR_NAME,
    ));

    if let Some(home_root) = home_agents_skills_root() {
        roots.push(home_root);
    }

    // Load order matters: we dedupe by name, keeping the first occurrence.
    // This makes repo/user skills win over system/admin skills.
    roots.push(user_skills_root(config));
    roots.push(system_skills_root(config));
    roots.push(admin_skills_root());

    dedupe_skill_roots_by_path(&mut roots);

    roots
}

fn discover_skills_under_root(root: &Path, scope: SkillScope, outcome: &mut SkillLoadOutcome) {
    let Ok(root) = normalize_path(root) else {
        return;
    };

    if !root.is_dir() {
        return;
    }

    fn enqueue_dir(queue: &mut VecDeque<PathBuf>, visited_dirs: &mut HashSet<PathBuf>, path: PathBuf) {
        if visited_dirs.insert(path.clone()) {
            queue.push_back(path);
        }
    }

    let follow_symlinks = matches!(scope, SkillScope::Repo | SkillScope::User | SkillScope::Admin);

    let mut visited_dirs: HashSet<PathBuf> = HashSet::new();
    visited_dirs.insert(root.clone());
    let mut queue: VecDeque<PathBuf> = VecDeque::from([root.clone()]);

    while let Some(dir) = queue.pop_front() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                error!("failed to read skills dir {}: {e:#}", dir.display());
                continue;
            }
        };

        let mut entries: Vec<fs::DirEntry> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            let path = entry.path();
            let file_name = match path.file_name().and_then(|f| f.to_str()) {
                Some(name) => name,
                None => continue,
            };

            if file_name.starts_with('.') {
                continue;
            }

            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            if file_type.is_symlink() {
                if !follow_symlinks {
                    continue;
                }

                let metadata = match fs::metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(e) => {
                        error!(
                            "failed to stat skills entry {} (symlink): {e:#}",
                            path.display()
                        );
                        continue;
                    }
                };

                if metadata.is_dir() {
                    let Ok(resolved_dir) = normalize_path(&path) else {
                        continue;
                    };
                    enqueue_dir(
                        &mut queue,
                        &mut visited_dirs,
                        resolved_dir,
                    );
                }
                continue;
            }

            if file_type.is_dir() {
                let Ok(resolved_dir) = normalize_path(&path) else {
                    continue;
                };
                enqueue_dir(
                    &mut queue,
                    &mut visited_dirs,
                    resolved_dir,
                );
                continue;
            }

            if file_type.is_file() && file_name == SKILLS_FILENAME {
                match parse_skill_file(&path, scope) {
                    Ok(skill) => {
                        outcome.skills.push(skill);
                    }
                    Err(err) => {
                        if scope != SkillScope::System {
                            outcome.errors.push(SkillError {
                                path,
                                message: err.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
}

fn parse_skill_file(path: &Path, scope: SkillScope) -> Result<SkillMetadata, SkillParseError> {
    let contents = fs::read_to_string(path).map_err(SkillParseError::Read)?;

    let frontmatter = extract_frontmatter(&contents).ok_or(SkillParseError::MissingFrontmatter)?;

    let parsed: SkillFrontmatter =
        serde_yaml::from_str(&frontmatter).map_err(SkillParseError::InvalidYaml)?;

    let name = sanitize_single_line(&parsed.name);
    let description = sanitize_single_line(&parsed.description);
    let short_description = parsed
        .metadata
        .and_then(|metadata| metadata.short_description)
        .map(|value| sanitize_single_line(&value))
        .filter(|value| !value.is_empty());

    validate_field(&name, MAX_NAME_LEN, "name")?;
    validate_field(&description, MAX_DESCRIPTION_LEN, "description")?;
    if let Some(short_description) = short_description.as_deref() {
        validate_field(
            short_description,
            MAX_SHORT_DESCRIPTION_LEN,
            "metadata.short-description",
        )?;
    }

    let resolved_path = normalize_path(path).unwrap_or_else(|_| path.to_path_buf());
    let skill_dir = resolved_path.parent().unwrap_or_else(|| Path::new("."));
    let resources = parse_skill_resources(parsed.resources, skill_dir)?;
    let commands = parse_skill_commands(parsed.commands, skill_dir, &resources)?;
    let workflow_defaults = parse_workflow_defaults(parsed.workflow_defaults)?;
    let policy = parsed
        .policy
        .map(|policy| parse_skill_policy(policy, skill_dir))
        .transpose()?;

    Ok(SkillMetadata {
        name,
        description,
        short_description,
        path: resolved_path,
        scope,
        content: contents,
        policy,
        resources,
        commands,
        workflow_defaults,
    })
}

fn parse_skill_resources(
    resources: Vec<SkillFrontmatterResource>,
    skill_dir: &Path,
) -> Result<Vec<SkillResource>, SkillParseError> {
    if resources.len() > MAX_STRUCTURED_ITEMS_PER_SKILL {
        return Err(SkillParseError::InvalidField {
            field: "resources",
            reason: format!("must contain at most {MAX_STRUCTURED_ITEMS_PER_SKILL} entries"),
        });
    }

    let mut parsed = Vec::with_capacity(resources.len());
    for resource in resources {
        let description = parse_optional_text(
            resource.description,
            MAX_STRUCTURED_PURPOSE_LEN,
            "resources.description",
        )?;
        let resolved_path = resolve_skill_relative_path(skill_dir, resource.path);

        if !resolved_path.exists() {
            tracing::warn!(
                "Resource path '{}' does not exist on disk.",
                resolved_path.display()
            );
        }

        let kind = match resource.kind {
            SkillFrontmatterResourceKind::Script => SkillResourceKind::Script,
            SkillFrontmatterResourceKind::Reference => SkillResourceKind::Reference,
            SkillFrontmatterResourceKind::Template => SkillResourceKind::Template,
            SkillFrontmatterResourceKind::Asset => SkillResourceKind::Asset,
        };

        parsed.push(SkillResource {
            path: resolved_path,
            kind,
            description,
        });
    }
    Ok(parsed)
}

fn parse_skill_commands(
    commands: Vec<SkillFrontmatterCommand>,
    skill_dir: &Path,
    declared_resources: &[SkillResource],
) -> Result<Vec<SkillCommand>, SkillParseError> {
    if commands.len() > MAX_STRUCTURED_ITEMS_PER_SKILL {
        return Err(SkillParseError::InvalidField {
            field: "commands",
            reason: format!("must contain at most {MAX_STRUCTURED_ITEMS_PER_SKILL} entries"),
        });
    }

    let mut parsed = Vec::with_capacity(commands.len());
    for command in commands {
        let name = sanitize_single_line(&command.name);
        validate_field(&name, MAX_STRUCTURED_ID_LEN, "commands.name")?;
        let purpose = sanitize_single_line(&command.purpose);
        validate_field(&purpose, MAX_STRUCTURED_PURPOSE_LEN, "commands.purpose")?;
        let resolved_resource_path = resolve_skill_relative_path(skill_dir, command.resource_path.clone());

        let resource_exists = declared_resources.iter().any(|r| r.path == resolved_resource_path);
        if !resource_exists {
            return Err(SkillParseError::InvalidField {
                field: "commands.resource_path",
                reason: format!(
                    "command '{}' references resource_path '{}' which is not declared in the resources section",
                    name, command.resource_path.display()
                ),
            });
        }

        let example_argv = if command.example_argv.is_empty() {
            Vec::new()
        } else {
            validate_argv_tokens(command.example_argv, "commands.example_argv")?
        };

        parsed.push(SkillCommand {
            name,
            resource_path: resolved_resource_path,
            example_argv,
            purpose,
        });
    }
    Ok(parsed)
}

fn parse_optional_text(
    value: Option<String>,
    max_len: usize,
    field: &'static str,
) -> Result<Option<String>, SkillParseError> {
    value
        .map(|value| sanitize_single_line(&value))
        .filter(|value| !value.is_empty())
        .map(|value| {
            validate_field(&value, max_len, field)?;
            Ok::<String, SkillParseError>(value)
        })
        .transpose()
}

fn parse_workflow_defaults(
    defaults: Vec<SkillFrontmatterWorkflowDefault>,
) -> Result<Vec<SkillWorkflowDefault>, SkillParseError> {
    if defaults.len() > MAX_STRUCTURED_ITEMS_PER_SKILL {
        return Err(SkillParseError::InvalidField {
            field: "workflow_defaults",
            reason: format!("must contain at most {MAX_STRUCTURED_ITEMS_PER_SKILL} entries"),
        });
    }

    let mut parsed = Vec::with_capacity(defaults.len());
    for default in defaults {
        let name = sanitize_single_line(&default.name);
        validate_field(&name, MAX_STRUCTURED_ID_LEN, "workflow_defaults.name")?;
        let value = sanitize_single_line(&default.value);
        validate_field(&value, MAX_STRUCTURED_VALUE_LEN, "workflow_defaults.value")?;
        let description = parse_optional_text(
            default.description,
            MAX_STRUCTURED_PURPOSE_LEN,
            "workflow_defaults.description",
        )?;
        parsed.push(SkillWorkflowDefault {
            name,
            value,
            description,
        });
    }
    Ok(parsed)
}

fn parse_skill_policy(
    policy: SkillFrontmatterPolicy,
    skill_dir: &Path,
) -> Result<SkillPolicy, SkillParseError> {
    if policy.command_policies.len() > MAX_COMMAND_POLICIES_PER_SKILL {
        return Err(SkillParseError::InvalidField {
            field: "policy.command_policies",
            reason: format!(
                "must contain at most {MAX_COMMAND_POLICIES_PER_SKILL} entries"
            ),
        });
    }

    let mut command_policies = Vec::with_capacity(policy.command_policies.len());
    for (index, command_policy) in policy.command_policies.into_iter().enumerate() {
        command_policies.push(parse_command_policy(command_policy, skill_dir, index)?);
    }

    Ok(SkillPolicy {
        allow_implicit_invocation: policy.allow_implicit_invocation,
        command_policies,
    })
}

fn parse_command_policy(
    policy: SkillFrontmatterCommandPolicy,
    skill_dir: &Path,
    index: usize,
) -> Result<SkillCommandPolicy, SkillParseError> {
    let id = sanitize_single_line(&policy.id);
    validate_field(&id, MAX_COMMAND_POLICY_ID_LEN, "policy.command_policies.id")?;
    let matcher = parse_command_matcher(policy.matcher, index)?;
    let message = policy
        .message
        .map(|message| sanitize_single_line(&message))
        .filter(|message| !message.is_empty())
        .map(|message| {
            validate_field(
                &message,
                MAX_COMMAND_POLICY_MESSAGE_LEN,
                "policy.command_policies.message",
            )?;
            Ok::<String, SkillParseError>(message)
        })
        .transpose()?;

    if policy.preferred.len() > MAX_COMMAND_POLICY_PREFERRED {
        return Err(SkillParseError::InvalidField {
            field: "policy.command_policies.preferred",
            reason: format!("must contain at most {MAX_COMMAND_POLICY_PREFERRED} entries"),
        });
    }

    let mut preferred = Vec::with_capacity(policy.preferred.len());
    for preferred_entry in policy.preferred {
        preferred.push(parse_command_policy_preferred(preferred_entry, skill_dir)?);
    }

    Ok(SkillCommandPolicy {
        id,
        matcher,
        action: match policy.action {
            SkillFrontmatterCommandPolicyAction::RequirePreferred => {
                SkillCommandPolicyAction::RequirePreferred
            }
            SkillFrontmatterCommandPolicyAction::RequireConfirm => {
                SkillCommandPolicyAction::RequireConfirm
            }
            SkillFrontmatterCommandPolicyAction::Reject => SkillCommandPolicyAction::Reject,
        },
        message,
        preferred,
    })
}

fn parse_command_matcher(
    matcher: SkillFrontmatterCommandMatcher,
    index: usize,
) -> Result<SkillCommandMatcher, SkillParseError> {
    let argv_exact = parse_match_argv(
        matcher.argv_exact,
        "policy.command_policies.match.argv_exact",
    )?;
    let argv_prefix = parse_match_argv(
        matcher.argv_prefix,
        "policy.command_policies.match.argv_prefix",
    )?;
    let shell_regex = matcher
        .shell_regex
        .map(|regex| sanitize_single_line(&regex))
        .filter(|regex| !regex.is_empty())
        .map(|regex| {
            validate_field(
                &regex,
                MAX_COMMAND_POLICY_REGEX_LEN,
                "policy.command_policies.match.shell_regex",
            )?;
            regex_lite::Regex::new(&regex).map_err(|err| SkillParseError::InvalidField {
                field: "policy.command_policies.match.shell_regex",
                reason: format!("invalid regex in entry {index}: {err}"),
            })?;
            Ok::<String, SkillParseError>(regex)
        })
        .transpose()?;

    let matcher_count = usize::from(argv_exact.is_some())
        + usize::from(argv_prefix.is_some())
        + usize::from(shell_regex.is_some());
    if matcher_count == 0 {
        return Err(SkillParseError::InvalidField {
            field: "policy.command_policies.match",
            reason: format!(
                "entry {index} must set one of argv_exact, argv_prefix, or shell_regex"
            ),
        });
    }
    if matcher_count > 1 {
        return Err(SkillParseError::InvalidField {
            field: "policy.command_policies.match",
            reason: format!(
                "entry {index} must set only one of argv_exact, argv_prefix, or shell_regex"
            ),
        });
    }

    Ok(SkillCommandMatcher {
        argv_exact,
        argv_prefix,
        shell_regex,
    })
}

fn parse_match_argv(
    argv: Option<Vec<String>>,
    field: &'static str,
) -> Result<Option<Vec<String>>, SkillParseError> {
    let Some(argv) = argv else {
        return Ok(None);
    };
    validate_argv_tokens(argv, field).map(Some)
}

fn validate_argv_tokens(
    argv: Vec<String>,
    field: &'static str,
) -> Result<Vec<String>, SkillParseError> {
    if argv.is_empty() || argv.len() > MAX_COMMAND_POLICY_ARGV_TOKENS {
        return Err(SkillParseError::InvalidField {
            field,
            reason: format!(
                "must contain between 1 and {MAX_COMMAND_POLICY_ARGV_TOKENS} tokens"
            ),
        });
    }

    let mut tokens = Vec::with_capacity(argv.len());
    for token in argv {
        let token = sanitize_single_line(&token);
        validate_field(&token, MAX_COMMAND_POLICY_TOKEN_LEN, field)?;
        tokens.push(token);
    }
    Ok(tokens)
}

fn parse_command_policy_preferred(
    preferred: SkillFrontmatterCommandPolicyPreferred,
    skill_dir: &Path,
) -> Result<SkillCommandPolicyPreferred, SkillParseError> {
    let purpose = preferred
        .purpose
        .map(|purpose| sanitize_single_line(&purpose))
        .filter(|purpose| !purpose.is_empty())
        .map(|purpose| {
            validate_field(
                &purpose,
                MAX_COMMAND_POLICY_PURPOSE_LEN,
                "policy.command_policies.preferred.purpose",
            )?;
            Ok::<String, SkillParseError>(purpose)
        })
        .transpose()?;
    let name = preferred
        .name
        .map(|name| sanitize_single_line(&name))
        .filter(|name| !name.is_empty())
        .map(|name| {
            validate_field(
                &name,
                MAX_NAME_LEN,
                "policy.command_policies.preferred.name",
            )?;
            Ok::<String, SkillParseError>(name)
        })
        .transpose()?;
    let path = preferred
        .path
        .map(|path| resolve_skill_relative_path(skill_dir, path));
    let example_argv = if preferred.example_argv.is_empty() {
        Vec::new()
    } else {
        validate_argv_tokens(
            preferred.example_argv,
            "policy.command_policies.preferred.example_argv",
        )?
    };

    Ok(SkillCommandPolicyPreferred {
        kind: match preferred.kind {
            SkillFrontmatterCommandPolicyPreferredKind::Script => {
                SkillCommandPolicyPreferredKind::Script
            }
            SkillFrontmatterCommandPolicyPreferredKind::Skill => {
                SkillCommandPolicyPreferredKind::Skill
            }
            SkillFrontmatterCommandPolicyPreferredKind::Command => {
                SkillCommandPolicyPreferredKind::Command
            }
        },
        path,
        name,
        example_argv,
        purpose,
    })
}

fn resolve_skill_relative_path(skill_dir: &Path, path: PathBuf) -> PathBuf {
    let path = if path.is_absolute() {
        path
    } else {
        skill_dir.join(path)
    };
    normalize_path(&path).unwrap_or(path)
}

fn sanitize_single_line(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn validate_field(
    value: &str,
    max_len: usize,
    field_name: &'static str,
) -> Result<(), SkillParseError> {
    if value.is_empty() {
        return Err(SkillParseError::MissingField(field_name));
    }
    if value.chars().count() > max_len {
        return Err(SkillParseError::InvalidField {
            field: field_name,
            reason: format!("exceeds maximum length of {max_len} characters"),
        });
    }
    Ok(())
}

fn extract_frontmatter(contents: &str) -> Option<String> {
    let mut lines = contents.lines();
    if !matches!(lines.next(), Some(line) if line.trim() == "---") {
        return None;
    }

    let mut frontmatter_lines: Vec<&str> = Vec::new();
    let mut found_closing = false;
    for line in lines.by_ref() {
        if line.trim() == "---" {
            found_closing = true;
            break;
        }
        frontmatter_lines.push(line);
    }

    if frontmatter_lines.is_empty() || !found_closing {
        return None;
    }

    Some(frontmatter_lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::config::ConfigOverrides;
    use crate::config::ConfigToml;
    use std::ffi::OsString;
    use std::process::Command;

    const AGENTS_DIR_NAME: &str = ".agents";

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.previous.as_ref() {
                Some(value) => {
                    // SAFETY: tests that mutate process env are serialised.
                    unsafe { std::env::set_var(self.key, value) }
                }
                None => {
                    // SAFETY: tests that mutate process env are serialised.
                    unsafe { std::env::remove_var(self.key) }
                }
            }
        }
    }

    fn set_env_var(key: &'static str, value: &Path) -> EnvVarGuard {
        let previous = std::env::var_os(key);
        // SAFETY: tests that mutate process env are serialised.
        unsafe { std::env::set_var(key, value) };
        EnvVarGuard { key, previous }
    }

    fn make_config_for_cwd(code_home: &Path, cwd: &Path) -> Config {
        Config::load_from_base_config_with_overrides(
            ConfigToml::default(),
            ConfigOverrides {
                cwd: Some(cwd.to_path_buf()),
                ..Default::default()
            },
            code_home.to_path_buf(),
        )
        .expect("build config")
    }

    fn mark_as_git_repo(dir: &Path) {
        let output = Command::new("git")
            .arg("init")
            .current_dir(dir)
            .output()
            .expect("run git init");
        assert!(
            output.status.success(),
            "git init failed: status={:?}",
            output.status.code()
        );
    }

    fn write_skill_at(skills_root: &Path, dir: &str, name: &str, description: &str) -> PathBuf {
        let skill_dir = skills_root.join(dir);
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        let skill_path = skill_dir.join(SKILLS_FILENAME);
        fs::write(
            &skill_path,
            format!(
                "---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n"
            ),
        )
        .expect("write skill file");
        skill_path
    }

    fn write_skill_with_short_description_at(
        skills_root: &Path,
        dir: &str,
        name: &str,
        description: &str,
        short_description: &str,
    ) -> PathBuf {
        let skill_dir = skills_root.join(dir);
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        let skill_path = skill_dir.join(SKILLS_FILENAME);
        fs::write(
            &skill_path,
            format!(
                "---\nname: {name}\ndescription: {description}\nmetadata:\n  short-description: {short_description}\n---\n\n# {name}\n"
            ),
        )
        .expect("write skill file");
        skill_path
    }

    fn write_manual_skill_at(
        skills_root: &Path,
        dir: &str,
        name: &str,
        description: &str,
    ) -> PathBuf {
        let skill_dir = skills_root.join(dir);
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        let skill_path = skill_dir.join(SKILLS_FILENAME);
        fs::write(
            &skill_path,
            format!(
                "---\nname: {name}\ndescription: {description}\npolicy:\n  allow_implicit_invocation: false\n---\n\n# {name}\n"
            ),
        )
        .expect("write skill file");
        skill_path
    }

    fn write_command_policy_skill_at(skills_root: &Path) -> PathBuf {
        let skill_dir = skills_root.join("github");
        fs::create_dir_all(skill_dir.join("scripts")).expect("create skill dir");
        let script_path = skill_dir.join("scripts").join("gh-pr.py");
        fs::write(&script_path, "#!/usr/bin/env python3\n").expect("write script");
        let skill_path = skill_dir.join(SKILLS_FILENAME);
        fs::write(
            &skill_path,
            "---\nname: github\ndescription: GitHub workflows\npolicy:\n  command_policies:\n    - id: prefer-pr-merge\n      match:\n        argv_prefix: [\"gh\", \"pr\", \"merge\"]\n      action: require_preferred\n      message: Raw gh pr merge bypasses helper flow.\n      preferred:\n        - kind: script\n          path: scripts/gh-pr.py\n          example_argv: [\"scripts/gh-pr.py\", \"merge\", \"<pr>\"]\n          purpose: Use helper script.\n        - kind: skill\n          name: github\n---\n\n# github\n",
        )
        .expect("write skill file");
        skill_path
    }

    fn write_structured_skill_at(skills_root: &Path) -> PathBuf {
        let skill_dir = skills_root.join("plan");
        fs::create_dir_all(skill_dir.join("scripts")).expect("create scripts dir");
        fs::create_dir_all(skill_dir.join("references")).expect("create references dir");
        fs::write(skill_dir.join("scripts").join("create_plan.py"), "").expect("write script");
        fs::write(skill_dir.join("scripts").join("list_plans.py"), "").expect("write script");
        fs::write(skill_dir.join("references").join("format.md"), "").expect("write reference");
        let skill_path = skill_dir.join(SKILLS_FILENAME);
        fs::write(
            &skill_path,
            r#"---
name: plan
description: Plan workflows
resources:
  - path: scripts/create_plan.py
    kind: script
    description: Script to create plan
  - path: scripts/list_plans.py
    kind: script
    description: Script to list plans
  - path: references/format.md
    kind: reference
    description: Plan body format.
commands:
  - name: create-plan
    resource_path: scripts/create_plan.py
    example_argv: ["python", "scripts/create_plan.py", "--name", "<name>"]
    purpose: Create a saved plan.
  - name: list-plans
    resource_path: scripts/list_plans.py
    example_argv: ["python", "scripts/list_plans.py"]
    purpose: List saved plans.
workflow_defaults:
  - name: save_location
    value: $CODEX_HOME/plans or ~/.codex/plans
    description: Keep plan files outside repositories.
---

# plan
"#,
        )
        .expect("write skill file");
        skill_path
    }

    fn normalized(path: &Path) -> PathBuf {
        normalize_path(path).unwrap_or_else(|_| path.to_path_buf())
    }

    #[test]
    fn loads_skill_policy_from_frontmatter() {
        let skills_root = tempfile::tempdir().expect("tempdir");
        let skill_path = write_manual_skill_at(
            skills_root.path(),
            "manual",
            "manual-skill",
            "Manual skill",
        );

        let outcome = load_skills_from_roots(vec![SkillRoot {
            path: skills_root.path().to_path_buf(),
            scope: SkillScope::User,
        }]);

        assert!(
            outcome.errors.is_empty(),
            "unexpected errors: {:?}",
            outcome.errors
        );
        assert_eq!(outcome.skills.len(), 1);
        let skill = &outcome.skills[0];
        assert_eq!(skill.path, normalized(&skill_path));
        assert_eq!(
            skill.policy,
            Some(SkillPolicy {
                allow_implicit_invocation: Some(false),
                command_policies: Vec::new(),
            })
        );
        assert!(!skill.allow_implicit_invocation());
    }

    #[test]
    fn loads_command_policy_from_frontmatter() {
        let skills_root = tempfile::tempdir().expect("tempdir");
        let skill_path = write_command_policy_skill_at(skills_root.path());

        let outcome = load_skills_from_roots(vec![SkillRoot {
            path: skills_root.path().to_path_buf(),
            scope: SkillScope::User,
        }]);

        assert!(
            outcome.errors.is_empty(),
            "unexpected errors: {:?}",
            outcome.errors
        );
        let skill = outcome.skills.first().expect("skill");
        let policy = skill.policy.as_ref().expect("policy");
        assert!(skill.allow_implicit_invocation());
        assert_eq!(policy.command_policies.len(), 1);
        let command_policy = &policy.command_policies[0];
        assert_eq!(command_policy.id, "prefer-pr-merge");
        assert_eq!(
            command_policy.matcher.argv_prefix.as_deref(),
            Some(&["gh".to_string(), "pr".to_string(), "merge".to_string()][..])
        );
        assert_eq!(
            command_policy.action,
            SkillCommandPolicyAction::RequirePreferred
        );
        assert_eq!(command_policy.preferred.len(), 2);
        assert_eq!(
            command_policy.preferred[0].path.as_deref(),
            Some(normalized(&skill_path).parent().unwrap().join("scripts/gh-pr.py").as_path())
        );
    }

    #[test]
    fn loads_structured_skill_workflow_fields_from_frontmatter() {
        let skills_root = tempfile::tempdir().expect("tempdir");
        let skill_path = write_structured_skill_at(skills_root.path());

        let outcome = load_skills_from_roots(vec![SkillRoot {
            path: skills_root.path().to_path_buf(),
            scope: SkillScope::User,
        }]);

        assert!(
            outcome.errors.is_empty(),
            "unexpected errors: {:?}",
            outcome.errors
        );
        let skill = outcome.skills.first().expect("skill");
        assert_eq!(skill.resources.len(), 3);
        assert_eq!(skill.resources[0].kind, SkillResourceKind::Script);
        assert_eq!(
            skill.resources[0].path,
            normalized(&skill_path).parent().unwrap().join("scripts/create_plan.py")
        );
        assert_eq!(skill.resources[1].kind, SkillResourceKind::Script);
        assert_eq!(
            skill.resources[1].path,
            normalized(&skill_path).parent().unwrap().join("scripts/list_plans.py")
        );
        assert_eq!(skill.resources[2].kind, SkillResourceKind::Reference);
        assert_eq!(
            skill.resources[2].path,
            normalized(&skill_path).parent().unwrap().join("references/format.md")
        );

        assert_eq!(skill.commands.len(), 2);
        assert_eq!(skill.commands[0].name, "create-plan");
        assert_eq!(
            skill.commands[0].resource_path,
            normalized(&skill_path).parent().unwrap().join("scripts/create_plan.py")
        );
        assert_eq!(
            skill.commands[0].example_argv,
            ["python", "scripts/create_plan.py", "--name", "<name>"]
        );
        assert_eq!(skill.commands[0].purpose, "Create a saved plan.");

        assert_eq!(skill.commands[1].name, "list-plans");
        assert_eq!(
            skill.commands[1].resource_path,
            normalized(&skill_path).parent().unwrap().join("scripts/list_plans.py")
        );
        assert_eq!(
            skill.commands[1].example_argv,
            ["python", "scripts/list_plans.py"]
        );
        assert_eq!(skill.commands[1].purpose, "List saved plans.");
        assert_eq!(skill.workflow_defaults.len(), 1);
        assert_eq!(skill.workflow_defaults[0].name, "save_location");
        assert_eq!(
            skill.workflow_defaults[0].value,
            "$CODEX_HOME/plans or ~/.codex/plans"
        );
        assert_eq!(
            skill.workflow_defaults[0].description.as_deref(),
            Some("Keep plan files outside repositories.")
        );
    }

    #[test]
    fn command_resource_path_must_be_declared_resource() {
        let skills_root = tempfile::tempdir().expect("tempdir");
        let skill_dir = skills_root.path().join("bad");
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        fs::write(
            skill_dir.join(SKILLS_FILENAME),
            r#"---
name: bad
description: Bad command resource
commands:
  - name: missing-resource
    resource_path: scripts/missing.py
    example_argv: ["python", "scripts/missing.py"]
    purpose: Run a missing helper.
---

# bad
"#,
        )
        .expect("write skill file");

        let outcome = load_skills_from_roots(vec![SkillRoot {
            path: skills_root.path().to_path_buf(),
            scope: SkillScope::User,
        }]);

        assert!(outcome.skills.is_empty());
        assert_eq!(outcome.errors.len(), 1);
        assert!(
            outcome.errors[0]
                .message
                .contains("is not declared in the resources section"),
            "unexpected error: {}",
            outcome.errors[0].message
        );
    }

    #[test]
    fn invalid_command_policy_reports_skill_error() {
        let skills_root = tempfile::tempdir().expect("tempdir");
        let skill_dir = skills_root.path().join("bad");
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        fs::write(
            skill_dir.join(SKILLS_FILENAME),
            "---\nname: bad\ndescription: Bad policy\npolicy:\n  command_policies:\n    - id: bad-regex\n      match:\n        shell_regex: \"[\"\n      action: require_preferred\n---\n\n# bad\n",
        )
        .expect("write skill file");

        let outcome = load_skills_from_roots(vec![SkillRoot {
            path: skills_root.path().to_path_buf(),
            scope: SkillScope::User,
        }]);

        assert!(outcome.skills.is_empty());
        assert_eq!(outcome.errors.len(), 1);
        assert!(
            outcome.errors[0].message.contains("invalid regex"),
            "unexpected error: {}",
            outcome.errors[0].message
        );
    }

    #[test]
    fn command_policy_rejects_multiple_matchers() {
        let skills_root = tempfile::tempdir().expect("tempdir");
        let skill_dir = skills_root.path().join("bad");
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        fs::write(
            skill_dir.join(SKILLS_FILENAME),
            "---\nname: bad\ndescription: Bad policy\npolicy:\n  command_policies:\n    - id: too-many\n      match:\n        argv_exact: [\"gh\", \"pr\", \"merge\"]\n        argv_prefix: [\"gh\", \"pr\"]\n      action: require_preferred\n---\n\n# bad\n",
        )
        .expect("write skill file");

        let outcome = load_skills_from_roots(vec![SkillRoot {
            path: skills_root.path().to_path_buf(),
            scope: SkillScope::User,
        }]);

        assert!(outcome.skills.is_empty());
        assert_eq!(outcome.errors.len(), 1);
        assert!(
            outcome.errors[0].message.contains("must set only one"),
            "unexpected error: {}",
            outcome.errors[0].message
        );
    }

    #[test]
    fn config_name_selector_disables_implicit_invocation() {
        let skills_root = tempfile::tempdir().expect("tempdir");
        write_skill_at(
            skills_root.path(),
            "manual",
            "manual-skill",
            "Manual skill",
        );

        let cfg = Config::load_from_base_config_with_overrides(
            ConfigToml {
                skills: Some(crate::config::SkillsToml {
                    config: vec![crate::config::SkillConfigToml {
                        name: Some("manual-skill".to_string()),
                        enabled: false,
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
            ConfigOverrides::default(),
            tempfile::tempdir().expect("code home").path().to_path_buf(),
        )
        .expect("config");
        let mut outcome = load_skills_from_roots(vec![SkillRoot {
            path: skills_root.path().to_path_buf(),
            scope: SkillScope::User,
        }]);

        apply_skill_config_rules(&mut outcome.skills, &cfg);

        assert_eq!(outcome.skills.len(), 1);
        assert!(!outcome.skills[0].allow_implicit_invocation());
    }

    #[test]
    fn config_path_selector_disables_implicit_invocation() {
        let skills_root = tempfile::tempdir().expect("tempdir");
        let skill_path = write_skill_at(
            skills_root.path(),
            "manual",
            "manual-skill",
            "Manual skill",
        );
        let normalized_skill_path = normalized(&skill_path);

        let cfg = Config::load_from_base_config_with_overrides(
            ConfigToml {
                skills: Some(crate::config::SkillsToml {
                    config: vec![crate::config::SkillConfigToml {
                        path: Some(skill_path),
                        enabled: false,
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
            ConfigOverrides::default(),
            tempfile::tempdir().expect("code home").path().to_path_buf(),
        )
        .expect("config");
        let mut outcome = load_skills_from_roots(vec![SkillRoot {
            path: skills_root.path().to_path_buf(),
            scope: SkillScope::User,
        }]);

        apply_skill_config_rules(&mut outcome.skills, &cfg);

        assert_eq!(outcome.skills.len(), 1);
        assert_eq!(outcome.skills[0].path, normalized_skill_path);
        assert!(!outcome.skills[0].allow_implicit_invocation());
    }

    #[test]
    fn later_skill_config_rule_can_re_enable_matching_skill() {
        let skills_root = tempfile::tempdir().expect("tempdir");
        write_skill_at(
            skills_root.path(),
            "enabled",
            "enabled-skill",
            "Enabled skill",
        );

        let cfg = Config::load_from_base_config_with_overrides(
            ConfigToml {
                skills: Some(crate::config::SkillsToml {
                    config: vec![
                        crate::config::SkillConfigToml {
                            name: Some("enabled-skill".to_string()),
                            enabled: false,
                            ..Default::default()
                        },
                        crate::config::SkillConfigToml {
                            name: Some("enabled-skill".to_string()),
                            enabled: true,
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                }),
                ..Default::default()
            },
            ConfigOverrides::default(),
            tempfile::tempdir().expect("code home").path().to_path_buf(),
        )
        .expect("config");
        let mut outcome = load_skills_from_roots(vec![SkillRoot {
            path: skills_root.path().to_path_buf(),
            scope: SkillScope::User,
        }]);

        apply_skill_config_rules(&mut outcome.skills, &cfg);

        assert_eq!(outcome.skills.len(), 1);
        assert!(outcome.skills[0].allow_implicit_invocation());
    }

    #[test]
    fn loads_optional_short_description_from_metadata_frontmatter() {
        let skills_root = tempfile::tempdir().expect("tempdir");
        let skill_path = write_skill_with_short_description_at(
            skills_root.path(),
            "compact",
            "compact-skill",
            "Long model-visible trigger description",
            "Compact UI summary",
        );

        let outcome = load_skills_from_roots(vec![SkillRoot {
            path: skills_root.path().to_path_buf(),
            scope: SkillScope::User,
        }]);

        assert!(
            outcome.errors.is_empty(),
            "unexpected errors: {:?}",
            outcome.errors
        );
        assert_eq!(outcome.skills.len(), 1);
        let skill = &outcome.skills[0];
        assert_eq!(skill.path, normalized(&skill_path));
        assert_eq!(skill.description, "Long model-visible trigger description");
        assert_eq!(skill.short_description.as_deref(), Some("Compact UI summary"));
    }

    #[test]
    fn ignores_empty_short_description_metadata() {
        let skills_root = tempfile::tempdir().expect("tempdir");
        write_skill_with_short_description_at(
            skills_root.path(),
            "empty-compact",
            "empty-compact-skill",
            "Full description",
            "   ",
        );

        let outcome = load_skills_from_roots(vec![SkillRoot {
            path: skills_root.path().to_path_buf(),
            scope: SkillScope::User,
        }]);

        assert!(
            outcome.errors.is_empty(),
            "unexpected errors: {:?}",
            outcome.errors
        );
        assert_eq!(outcome.skills.len(), 1);
        assert_eq!(outcome.skills[0].short_description, None);
    }

    #[test]
    fn rejects_too_long_short_description_metadata() {
        let skills_root = tempfile::tempdir().expect("tempdir");
        write_skill_with_short_description_at(
            skills_root.path(),
            "long-compact",
            "long-compact-skill",
            "Full description",
            &"x".repeat(MAX_SHORT_DESCRIPTION_LEN + 1),
        );

        let outcome = load_skills_from_roots(vec![SkillRoot {
            path: skills_root.path().to_path_buf(),
            scope: SkillScope::User,
        }]);

        assert!(outcome.skills.is_empty());
        assert_eq!(outcome.errors.len(), 1);
        assert!(
            outcome.errors[0]
                .message
                .contains("invalid metadata.short-description"),
            "unexpected error: {:?}",
            outcome.errors
        );
    }

    #[test]
    fn loads_skills_from_agents_dir_without_codex_dir() {
        let code_home = tempfile::tempdir().expect("tempdir");
        let repo_dir = tempfile::tempdir().expect("tempdir");
        mark_as_git_repo(repo_dir.path());

        let skill_path = write_skill_at(
            &repo_dir.path().join(AGENTS_DIR_NAME).join(SKILLS_DIR_NAME),
            "agents",
            "agents-skill",
            "from agents",
        );
        let cfg = make_config_for_cwd(code_home.path(), repo_dir.path());

        let outcome = load_skills(&cfg);
        assert!(
            outcome.errors.is_empty(),
            "unexpected errors: {:?}",
            outcome.errors
        );
        assert!(
            outcome.skills.iter().any(|skill| {
                skill.name == "agents-skill"
                    && skill.description == "from agents"
                    && skill.path == normalized(&skill_path)
                    && skill.scope == SkillScope::Repo
            }),
            "expected repo .agents skill in outcome: {:?}",
            outcome.skills
        );
    }

    #[test]
    #[serial_test::serial]
    fn loads_skills_from_home_agents_dir_for_user_scope() {
        let home_dir = tempfile::tempdir().expect("tempdir");
        let _home_guard = set_env_var("HOME", home_dir.path());
        let code_home = tempfile::tempdir().expect("tempdir");
        let cwd = tempfile::tempdir().expect("tempdir");

        let skill_path = write_skill_at(
            &home_dir.path().join(AGENTS_DIR_NAME).join(SKILLS_DIR_NAME),
            "home",
            "home-agents-skill",
            "from home agents",
        );

        let cfg = make_config_for_cwd(code_home.path(), cwd.path());

        let outcome = load_skills(&cfg);
        assert!(
            outcome.errors.is_empty(),
            "unexpected errors: {:?}",
            outcome.errors
        );
        assert!(
            outcome.skills.iter().any(|skill| {
                skill.name == "home-agents-skill"
                    && skill.description == "from home agents"
                    && skill.path == normalized(&skill_path)
                    && skill.scope == SkillScope::User
            }),
            "expected home .agents user skill in outcome: {:?}",
            outcome.skills
        );
    }

    #[cfg(unix)]
    #[test]
    fn follows_symlinked_subdir_for_user_scope() {
        let root_dir = tempfile::tempdir().expect("tempdir");
        let shared_dir = tempfile::tempdir().expect("tempdir");

        let skill_path = write_skill_at(
            shared_dir.path(),
            "shared",
            "symlinked-user-skill",
            "from symlink",
        );

        std::os::unix::fs::symlink(shared_dir.path(), root_dir.path().join("shared"))
            .expect("create symlink");

        let outcome = load_skills_from_roots(vec![SkillRoot {
            path: root_dir.path().to_path_buf(),
            scope: SkillScope::User,
        }]);

        assert!(
            outcome.errors.is_empty(),
            "unexpected errors: {:?}",
            outcome.errors
        );
        assert!(
            outcome.skills.iter().any(|skill| {
                skill.name == "symlinked-user-skill"
                    && skill.description == "from symlink"
                    && skill.path == normalized(&skill_path)
                    && skill.scope == SkillScope::User
            }),
            "expected symlinked user skill in outcome: {:?}",
            outcome.skills
        );
    }

    #[test]
    fn loads_skills_from_agents_dirs_between_cwd_and_repo_root() {
        let code_home = tempfile::tempdir().expect("tempdir");
        let repo_dir = tempfile::tempdir().expect("tempdir");
        mark_as_git_repo(repo_dir.path());

        let nested_dir = repo_dir.path().join("nested/inner");
        fs::create_dir_all(&nested_dir).expect("create nested dir");

        let root_skill_path = write_skill_at(
            &repo_dir.path().join(AGENTS_DIR_NAME).join(SKILLS_DIR_NAME),
            "root",
            "root-agents-skill",
            "from root agents",
        );
        let nested_skill_path = write_skill_at(
            &repo_dir
                .path()
                .join("nested")
                .join(AGENTS_DIR_NAME)
                .join(SKILLS_DIR_NAME),
            "nested",
            "nested-agents-skill",
            "from nested agents",
        );

        let cfg = make_config_for_cwd(code_home.path(), &nested_dir);

        let outcome = load_skills(&cfg);
        assert!(
            outcome.errors.is_empty(),
            "unexpected errors: {:?}",
            outcome.errors
        );
        assert!(
            outcome.skills.iter().any(|skill| {
                skill.name == "root-agents-skill"
                    && skill.path == normalized(&root_skill_path)
                    && skill.scope == SkillScope::Repo
            }),
            "expected root .agents skill in outcome: {:?}",
            outcome.skills
        );
        assert!(
            outcome.skills.iter().any(|skill| {
                skill.name == "nested-agents-skill"
                    && skill.path == normalized(&nested_skill_path)
                    && skill.scope == SkillScope::Repo
            }),
            "expected nested .agents skill in outcome: {:?}",
            outcome.skills
        );
    }

    #[test]
    fn discovers_skills_beyond_previous_depth_limit() {
        let skills_root = tempfile::tempdir().expect("tempdir");
        let deep_path = "a/b/c/d/e/f/g/h";
        let deep_skill_path = write_skill_at(
            skills_root.path(),
            deep_path,
            "deep-skill",
            "beyond old depth limit",
        );

        let outcome = load_skills_from_roots(vec![SkillRoot {
            path: skills_root.path().to_path_buf(),
            scope: SkillScope::Repo,
        }]);

        assert!(
            outcome.errors.is_empty(),
            "unexpected errors: {:?}",
            outcome.errors
        );
        assert!(
            outcome.skills.iter().any(|skill| {
                skill.name == "deep-skill"
                    && skill.path == normalized(&deep_skill_path)
                    && skill.scope == SkillScope::Repo
            }),
            "expected deep skill in outcome: {:?}",
            outcome.skills
        );
    }

    #[test]
    fn discovers_skills_beyond_previous_directory_cap() {
        let skills_root = tempfile::tempdir().expect("tempdir");
        let root = skills_root.path();

        for i in 0..2001 {
            let dir = format!("dir-{i}");
            write_skill_at(
                root,
                &dir,
                &format!("skill-{i}"),
                "past old directory cap",
            );
        }

        let outcome = load_skills_from_roots(vec![SkillRoot {
            path: root.to_path_buf(),
            scope: SkillScope::Repo,
        }]);

        assert!(
            outcome.errors.is_empty(),
            "unexpected errors: {:?}",
            outcome.errors
        );
        assert_eq!(outcome.skills.len(), 2001);
    }

    #[test]
    #[serial_test::serial]
    fn prefers_repo_agents_over_user_and_legacy_for_duplicate_name() {
        let home_dir = tempfile::tempdir().expect("tempdir");
        let _home_guard = set_env_var("HOME", home_dir.path());
        let code_home = tempfile::tempdir().expect("tempdir");
        let repo_dir = tempfile::tempdir().expect("tempdir");
        mark_as_git_repo(repo_dir.path());

        let nested_dir = repo_dir.path().join("nested/inner");
        fs::create_dir_all(&nested_dir).expect("create nested dir");

        let repo_agents_path = write_skill_at(
            &repo_dir.path().join(AGENTS_DIR_NAME).join(SKILLS_DIR_NAME),
            "dup",
            "collision-skill",
            "from repo agents",
        );
        write_skill_at(
            &repo_dir
                .path()
                .join(REPO_ROOT_CONFIG_DIR_NAME)
                .join(SKILLS_DIR_NAME),
            "dup",
            "collision-skill",
            "from repo codex",
        );
        write_skill_at(
            &home_dir.path().join(AGENTS_DIR_NAME).join(SKILLS_DIR_NAME),
            "dup",
            "collision-skill",
            "from home agents",
        );
        write_skill_at(
            &code_home.path().join(SKILLS_DIR_NAME),
            "dup",
            "collision-skill",
            "from legacy user",
        );

        let cfg = make_config_for_cwd(code_home.path(), &nested_dir);

        let outcome = load_skills(&cfg);
        assert!(
            outcome.errors.is_empty(),
            "unexpected errors: {:?}",
            outcome.errors
        );

        let matching: Vec<&SkillMetadata> = outcome
            .skills
            .iter()
            .filter(|skill| skill.name == "collision-skill")
            .collect();
        assert_eq!(matching.len(), 1, "expected one deduped collision skill");
        assert_eq!(matching[0].description, "from repo agents");
        assert_eq!(matching[0].scope, SkillScope::Repo);
        assert_eq!(matching[0].path, normalized(&repo_agents_path));
    }

    #[test]
    fn still_loads_legacy_user_skills_root() {
        let code_home = tempfile::tempdir().expect("tempdir");
        let cwd = tempfile::tempdir().expect("tempdir");

        let legacy_path = write_skill_at(
            &code_home.path().join(SKILLS_DIR_NAME),
            "legacy",
            "legacy-user-skill",
            "from legacy user",
        );

        let cfg = make_config_for_cwd(code_home.path(), cwd.path());
        let outcome = load_skills(&cfg);
        assert!(
            outcome.errors.is_empty(),
            "unexpected errors: {:?}",
            outcome.errors
        );
        assert!(
            outcome.skills.iter().any(|skill| {
                skill.name == "legacy-user-skill"
                    && skill.description == "from legacy user"
                    && skill.path == normalized(&legacy_path)
                    && skill.scope == SkillScope::User
            }),
            "expected legacy user skill in outcome: {:?}",
            outcome.skills
        );
    }

    #[test]
    fn admin_root_uses_expected_path() {
        let root = admin_skills_root();
        assert_eq!(root.path, PathBuf::from(ADMIN_SKILLS_ROOT));
        assert_eq!(root.scope, SkillScope::Admin);
    }

    #[test]
    fn loader_parses_resources_and_commands() {
        let code_home = tempfile::tempdir().expect("tempdir");
        let cwd = tempfile::tempdir().expect("tempdir");
        let skill_dir = code_home.path().join("skills").join("plan");
        fs::create_dir_all(&skill_dir).unwrap();

        fs::write(skill_dir.join("create_plan.py"), "print('hello')").unwrap();
        fs::write(skill_dir.join("template.md"), "# Plan Template").unwrap();

        fs::write(
            skill_dir.join(SKILLS_FILENAME),
            r#"---
name: plan
description: Generate a plan
resources:
  - path: create_plan.py
    kind: script
    description: Helper to create a plan
  - path: template.md
    kind: template
    description: Markdown template
commands:
  - name: create-plan
    resource_path: create_plan.py
    example_argv: ["python", "create_plan.py", "--name"]
    purpose: Create a plan file
---
Plan skill body
"#,
        ).unwrap();

        let cfg = make_config_for_cwd(code_home.path(), cwd.path());
        let outcome = load_skills(&cfg);

        assert!(outcome.errors.is_empty(), "unexpected errors: {:?}", outcome.errors);
        assert_eq!(outcome.skills.len(), 1);
        let skill = &outcome.skills[0];

        assert_eq!(skill.resources.len(), 2);
        assert_eq!(skill.resources[0].path, normalized(&skill_dir.join("create_plan.py")));
        assert_eq!(skill.resources[0].kind, SkillResourceKind::Script);
        assert_eq!(skill.resources[0].description.as_deref(), Some("Helper to create a plan"));

        assert_eq!(skill.commands.len(), 1);
        assert_eq!(skill.commands[0].name, "create-plan");
        assert_eq!(skill.commands[0].resource_path, normalized(&skill_dir.join("create_plan.py")));
        assert_eq!(skill.commands[0].example_argv, vec!["python", "create_plan.py", "--name"]);
        assert_eq!(skill.commands[0].purpose, "Create a plan file");
    }

    #[test]
    fn loader_fails_when_command_references_undeclared_resource() {
        let code_home = tempfile::tempdir().expect("tempdir");
        let cwd = tempfile::tempdir().expect("tempdir");
        let skill_dir = code_home.path().join("skills").join("plan");
        fs::create_dir_all(&skill_dir).unwrap();

        fs::write(
            skill_dir.join(SKILLS_FILENAME),
            r#"---
name: plan
description: Generate a plan
resources:
  - path: template.md
    kind: template
commands:
  - name: create-plan
    resource_path: create_plan.py
    purpose: Create a plan file
---
Plan skill body
"#,
        ).unwrap();

        let cfg = make_config_for_cwd(code_home.path(), cwd.path());
        let outcome = load_skills(&cfg);

        assert!(!outcome.errors.is_empty(), "expected validation failure");
        assert!(outcome.errors[0].message.contains("references resource_path"));
    }

    #[test]
    fn bundled_system_skills_expose_structured_workflow_metadata() {
        let code_home = tempfile::tempdir().expect("code home");
        let cfg = make_config_for_cwd(code_home.path(), code_home.path());
        install_system_skills(code_home.path()).expect("install system skills");

        let outcome = load_skills_from_roots(vec![system_skills_root(&cfg)]);

        assert!(
            outcome.errors.is_empty(),
            "unexpected errors: {:?}",
            outcome.errors
        );

        let plan = outcome
            .skills
            .iter()
            .find(|skill| skill.name == "plan")
            .expect("plan skill");
        assert_resource_command_counts(plan, 3, 3, 1);
        assert!(plan.commands.iter().any(|command| command.name == "create-plan"));

        let creator = outcome
            .skills
            .iter()
            .find(|skill| skill.name == "skill-creator")
            .expect("skill-creator skill");
        assert_resource_command_counts(creator, 3, 3, 1);
        assert!(creator.commands.iter().any(|command| command.name == "init-skill"));
        assert!(
            creator
                .workflow_defaults
                .iter()
                .any(|default| default.name == "resource_dirs")
        );

        let installer = outcome
            .skills
            .iter()
            .find(|skill| skill.name == "skill-installer")
            .expect("skill-installer skill");
        assert_resource_command_counts(installer, 3, 4, 4);
        assert!(
            installer
                .commands
                .iter()
                .any(|command| command.name == "install-skill-from-repo")
        );
        assert!(
            installer
                .workflow_defaults
                .iter()
                .any(|default| default.name == "default_method")
        );
    }

    fn assert_resource_command_counts(
        skill: &SkillMetadata,
        resources: usize,
        commands: usize,
        workflow_defaults: usize,
    ) {
        assert_eq!(skill.resources.len(), resources, "{} resources", skill.name);
        assert_eq!(skill.commands.len(), commands, "{} commands", skill.name);
        assert_eq!(
            skill.workflow_defaults.len(),
            workflow_defaults,
            "{} workflow defaults",
            skill.name
        );
    }
}
