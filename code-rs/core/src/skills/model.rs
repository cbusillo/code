use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillScope {
    Repo,
    User,
    System,
    Admin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub short_description: Option<String>,
    pub path: PathBuf,
    pub scope: SkillScope,
    pub content: String,
    pub policy: Option<SkillPolicy>,
    pub resources: Vec<SkillResource>,
    pub commands: Vec<SkillCommand>,
    pub workflow_defaults: Vec<SkillWorkflowDefault>,
}

impl SkillMetadata {
    pub fn allow_implicit_invocation(&self) -> bool {
        self.policy
            .as_ref()
            .and_then(|policy| policy.allow_implicit_invocation)
            .unwrap_or(true)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillResource {
    pub path: PathBuf,
    pub kind: SkillResourceKind,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillResourceKind {
    Script,
    Reference,
    Template,
    Asset,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCommand {
    pub name: String,
    pub resource_path: PathBuf,
    pub example_argv: Vec<String>,
    pub purpose: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillWorkflowDefault {
    pub name: String,
    pub value: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkillPolicy {
    pub allow_implicit_invocation: Option<bool>,
    pub command_policies: Vec<SkillCommandPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCommandPolicy {
    pub id: String,
    pub matcher: SkillCommandMatcher,
    pub action: SkillCommandPolicyAction,
    pub message: Option<String>,
    pub preferred: Vec<SkillCommandPolicyPreferred>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkillCommandMatcher {
    pub argv_exact: Option<Vec<String>>,
    pub argv_prefix: Option<Vec<String>>,
    pub shell_regex: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillCommandPolicyAction {
    RequirePreferred,
    RequireConfirm,
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCommandPolicyPreferred {
    pub kind: SkillCommandPolicyPreferredKind,
    pub path: Option<PathBuf>,
    pub name: Option<String>,
    pub example_argv: Vec<String>,
    pub purpose: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillCommandPolicyPreferredKind {
    Script,
    Skill,
    Command,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillError {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct SkillLoadOutcome {
    pub skills: Vec<SkillMetadata>,
    pub errors: Vec<SkillError>,
}
