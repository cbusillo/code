use crate::skills::model::SkillCommandMatcher;
use crate::skills::model::SkillCommandPolicy;
use crate::skills::model::SkillCommandPolicyAction;
use crate::skills::model::SkillCommandPolicyPreferred;
use crate::skills::model::SkillCommandPolicyPreferredKind;
use crate::skills::model::SkillMetadata;
use regex_lite::Regex;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) struct SkillCommandPolicyRuntime {
    policies: Vec<CompiledSkillCommandPolicy>,
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledSkillCommandPolicy {
    pub(crate) skill_name: String,
    pub(crate) skill_path: PathBuf,
    pub(crate) id: String,
    pub(crate) matcher: SkillCommandMatcher,
    pub(crate) shell_regex: Option<Regex>,
    pub(crate) action: SkillCommandPolicyAction,
    pub(crate) message: Option<String>,
    pub(crate) preferred: Vec<SkillCommandPolicyPreferred>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchSpecificity {
    Exact,
    Prefix,
    Regex,
}

#[derive(Debug, Clone)]
pub(crate) struct SkillCommandPolicyMatch<'a> {
    pub(crate) policy: &'a CompiledSkillCommandPolicy,
    matched_command: String,
    specificity: MatchSpecificity,
}

impl SkillCommandPolicyRuntime {
    pub(crate) fn from_skills(skills: &[SkillMetadata]) -> Self {
        let mut policies = Vec::new();
        for skill in skills {
            let Some(policy) = skill.policy.as_ref() else {
                continue;
            };
            for command_policy in &policy.command_policies {
                if let Some(compiled) = compile_policy(skill, command_policy) {
                    policies.push(compiled);
                }
            }
        }
        Self { policies }
    }

    pub(crate) fn check<'a>(
        &'a self,
        command: &[String],
        has_confirm_prefix: bool,
    ) -> Option<SkillCommandPolicyMatch<'a>> {
        if self.policies.is_empty() {
            return None;
        }
        let normalized = NormalizedCommand::from_command(command);
        let mut best: Option<SkillCommandPolicyMatch<'a>> = None;
        for policy in &self.policies {
            if has_confirm_prefix && policy.action == SkillCommandPolicyAction::RequireConfirm {
                continue;
            }
            let Some(candidate) = policy_matches(policy, &normalized) else {
                continue;
            };
            if best
                .as_ref()
                .is_none_or(|current| candidate.is_more_specific_than(current))
            {
                best = Some(candidate);
            }
        }
        best
    }
}

impl<'a> SkillCommandPolicyMatch<'a> {
    fn is_more_specific_than(&self, other: &Self) -> bool {
        specificity_rank(self.specificity) < specificity_rank(other.specificity)
    }

    pub(crate) fn guidance(&self, original_label: &str, original_value: &str) -> String {
        let policy = self.policy;
        let message = policy.message.as_deref().unwrap_or(match policy.action {
            SkillCommandPolicyAction::RequirePreferred => {
                "A loaded skill owns this workflow; use one of the preferred actions instead."
            }
            SkillCommandPolicyAction::RequireConfirm => {
                "A loaded skill requires explicit confirmation before this raw command runs."
            }
            SkillCommandPolicyAction::Reject => {
                "A loaded skill rejects this raw command shape."
            }
        });
        let mut lines = vec![
            format!(
                "Command policy matched skill `{}` ({}) at {}.",
                policy.skill_name,
                policy.id,
                policy.skill_path.display()
            ),
            String::new(),
            message.to_string(),
            String::new(),
        ];

        if !policy.preferred.is_empty() {
            lines.push("Preferred actions:".to_string());
            for preferred in &policy.preferred {
                lines.push(format_preferred(preferred));
            }
            lines.push(String::new());
        }

        if policy.action == SkillCommandPolicyAction::RequireConfirm {
            lines.push("Resend with `confirm:` only if the user explicitly asked for this raw command.".to_string());
            lines.push(String::new());
        }

        lines.push(format!("matched_command: {}", self.matched_command));
        lines.push(format!("{original_label}: {original_value}"));
        lines.join("\n")
    }
}

fn compile_policy(
    skill: &SkillMetadata,
    policy: &SkillCommandPolicy,
) -> Option<CompiledSkillCommandPolicy> {
    let shell_regex = policy
        .matcher
        .shell_regex
        .as_ref()
        .and_then(|pattern| Regex::new(pattern).ok());
    Some(CompiledSkillCommandPolicy {
        skill_name: skill.name.clone(),
        skill_path: skill.path.clone(),
        id: policy.id.clone(),
        matcher: policy.matcher.clone(),
        shell_regex,
        action: policy.action,
        message: policy.message.clone(),
        preferred: policy.preferred.clone(),
    })
}

fn policy_matches<'a>(
    policy: &'a CompiledSkillCommandPolicy,
    normalized: &NormalizedCommand,
) -> Option<SkillCommandPolicyMatch<'a>> {
    if let Some(argv_exact) = policy.matcher.argv_exact.as_ref() {
        for argv in normalized.argv_candidates() {
            if argv == argv_exact.as_slice() {
                return Some(SkillCommandPolicyMatch {
                    policy,
                    matched_command: argv.join(" "),
                    specificity: MatchSpecificity::Exact,
                });
            }
        }
    }

    if let Some(argv_prefix) = policy.matcher.argv_prefix.as_ref() {
        for argv in normalized.argv_candidates() {
            if argv.starts_with(argv_prefix) {
                return Some(SkillCommandPolicyMatch {
                    policy,
                    matched_command: argv.join(" "),
                    specificity: MatchSpecificity::Prefix,
                });
            }
        }
    }

    if let Some(regex) = policy.shell_regex.as_ref() {
        for text in normalized.text_candidates() {
            if regex.is_match(&text) {
                return Some(SkillCommandPolicyMatch {
                    policy,
                    matched_command: text.clone(),
                    specificity: MatchSpecificity::Regex,
                });
            }
        }
    }

    None
}

fn specificity_rank(specificity: MatchSpecificity) -> u8 {
    match specificity {
        MatchSpecificity::Exact => 0,
        MatchSpecificity::Prefix => 1,
        MatchSpecificity::Regex => 2,
    }
}

#[derive(Debug)]
struct NormalizedCommand {
    original_argv: Vec<String>,
    shell_script: Option<String>,
    shell_argv: Option<Vec<String>>,
}

impl NormalizedCommand {
    fn from_command(command: &[String]) -> Self {
        let shell_script = extract_shell_script(command).map(|(_, script)| script.to_string());
        let shell_argv = shell_script
            .as_ref()
            .and_then(|script| shlex::split(script));
        Self {
            original_argv: command.to_vec(),
            shell_script,
            shell_argv,
        }
    }

    fn argv_candidates(&self) -> Vec<&[String]> {
        let mut candidates = Vec::new();
        if let Some(shell_argv) = self.shell_argv.as_ref() {
            candidates.push(shell_argv.as_slice());
        }
        candidates.push(self.original_argv.as_slice());
        candidates
    }

    fn text_candidates(&self) -> Vec<String> {
        let mut candidates = Vec::new();
        if let Some(shell_script) = self.shell_script.as_ref() {
            candidates.push(shell_script.clone());
        }
        candidates.push(self.original_argv.join(" "));
        candidates
    }
}

fn extract_shell_script(command: &[String]) -> Option<(usize, &str)> {
    if command.len() < 3 {
        return None;
    }
    let shell = command.first()?.rsplit('/').next()?;
    if !matches!(shell, "bash" | "sh" | "zsh" | "fish") {
        return None;
    }
    for (index, arg) in command.iter().enumerate().skip(1) {
        if matches!(arg.as_str(), "-c" | "-lc" | "-ic") {
            return command.get(index + 1).map(|script| (index + 1, script.as_str()));
        }
        if !arg.starts_with('-') {
            break;
        }
    }
    None
}

fn format_preferred(preferred: &SkillCommandPolicyPreferred) -> String {
    let kind = match preferred.kind {
        SkillCommandPolicyPreferredKind::Script => "script",
        SkillCommandPolicyPreferredKind::Skill => "skill",
        SkillCommandPolicyPreferredKind::Command => "command",
    };
    let mut parts = vec![format!("- {kind}")];
    if let Some(name) = preferred.name.as_deref() {
        parts.push(format!("name: {name}"));
    }
    if let Some(path) = preferred.path.as_deref() {
        parts.push(format!("path: {}", path.display()));
    }
    if !preferred.example_argv.is_empty() {
        parts.push(format!("example: {}", preferred.example_argv.join(" ")));
    }
    if let Some(purpose) = preferred.purpose.as_deref() {
        parts.push(format!("purpose: {purpose}"));
    }
    parts.join("; ")
}

pub(crate) fn render_command_policy_summary(skill: &SkillMetadata) -> Vec<String> {
    let mut lines = Vec::new();
    let Some(policy) = skill.policy.as_ref() else {
        return lines;
    };
    for command_policy in &policy.command_policies {
        let matcher = describe_matcher(&command_policy.matcher);
        let preferred = command_policy
            .preferred
            .first()
            .map(format_preferred)
            .unwrap_or_else(|| "preferred action declared in skill body".to_string());
        lines.push(format!(
            "  - `{matcher}`: use {preferred} instead."
        ));
    }
    lines
}

fn describe_matcher(matcher: &SkillCommandMatcher) -> String {
    if let Some(argv_exact) = matcher.argv_exact.as_ref() {
        return argv_exact.join(" ");
    }
    if let Some(argv_prefix) = matcher.argv_prefix.as_ref() {
        return format!("{} ...", argv_prefix.join(" "));
    }
    if let Some(shell_regex) = matcher.shell_regex.as_ref() {
        return format!("/{shell_regex}/");
    }
    "<invalid matcher>".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::model::SkillCommandMatcher;
    use crate::skills::model::SkillCommandPolicy;
    use crate::skills::model::SkillCommandPolicyAction;
    use crate::skills::model::SkillCommandPolicyPreferred;
    use crate::skills::model::SkillCommandPolicyPreferredKind;
    use crate::skills::model::SkillPolicy;
    use crate::skills::model::SkillScope;

    fn runtime(policy: SkillCommandPolicy) -> SkillCommandPolicyRuntime {
        SkillCommandPolicyRuntime::from_skills(&[SkillMetadata {
            name: "github".to_string(),
            description: "GitHub workflows".to_string(),
            short_description: None,
            path: PathBuf::from("/tmp/github/SKILL.md"),
            scope: SkillScope::User,
            content: String::new(),
            policy: Some(SkillPolicy {
                allow_implicit_invocation: Some(false),
                command_policies: vec![policy],
            }),
        }])
    }

    fn policy(matcher: SkillCommandMatcher) -> SkillCommandPolicy {
        SkillCommandPolicy {
            id: "prefer-helper".to_string(),
            matcher,
            action: SkillCommandPolicyAction::RequirePreferred,
            message: Some("use the helper".to_string()),
            preferred: vec![SkillCommandPolicyPreferred {
                kind: SkillCommandPolicyPreferredKind::Script,
                path: Some(PathBuf::from("/tmp/github/scripts/gh-pr.py")),
                name: None,
                example_argv: vec!["scripts/gh-pr.py".to_string(), "merge".to_string()],
                purpose: Some("merge safely".to_string()),
            }],
        }
    }

    #[test]
    fn argv_prefix_matches_direct_command() {
        let runtime = runtime(policy(SkillCommandMatcher {
            argv_prefix: Some(vec!["gh".to_string(), "pr".to_string(), "merge".to_string()]),
            ..Default::default()
        }));

        let matched = runtime
            .check(
                &["gh".to_string(), "pr".to_string(), "merge".to_string(), "234".to_string()],
                false,
            )
            .expect("match");

        assert_eq!(matched.policy.id, "prefer-helper");
    }

    #[test]
    fn argv_prefix_matches_shell_script() {
        let runtime = runtime(policy(SkillCommandMatcher {
            argv_prefix: Some(vec!["gh".to_string(), "pr".to_string(), "merge".to_string()]),
            ..Default::default()
        }));

        let matched = runtime
            .check(
                &[
                    "bash".to_string(),
                    "-lc".to_string(),
                    "gh pr merge 234 --delete-branch".to_string(),
                ],
                false,
            )
            .expect("match");

        assert_eq!(matched.matched_command, "gh pr merge 234 --delete-branch");
    }

    #[test]
    fn shell_regex_matches_script_text() {
        let runtime = runtime(policy(SkillCommandMatcher {
            shell_regex: Some("^gh\\s+pr\\s+merge\\b".to_string()),
            ..Default::default()
        }));

        assert!(
            runtime
                .check(
                    &[
                        "bash".to_string(),
                        "-lc".to_string(),
                        "gh pr merge 234".to_string(),
                    ],
                    false,
                )
                .is_some()
        );
    }

    #[test]
    fn require_confirm_is_bypassed_by_confirm_prefix() {
        let mut command_policy = policy(SkillCommandMatcher {
            argv_prefix: Some(vec!["gh".to_string(), "pr".to_string(), "merge".to_string()]),
            ..Default::default()
        });
        command_policy.action = SkillCommandPolicyAction::RequireConfirm;
        let runtime = runtime(command_policy);

        assert!(
            runtime
                .check(
                    &["gh".to_string(), "pr".to_string(), "merge".to_string()],
                    true,
                )
                .is_none()
        );
    }

    #[test]
    fn guidance_includes_preferred_action() {
        let runtime = runtime(policy(SkillCommandMatcher {
            argv_prefix: Some(vec!["gh".to_string(), "pr".to_string(), "merge".to_string()]),
            ..Default::default()
        }));

        let guidance = runtime
            .check(&["gh".to_string(), "pr".to_string(), "merge".to_string()], false)
            .expect("match")
            .guidance("original_argv", "[\"gh\", \"pr\", \"merge\"]");

        assert!(guidance.contains("Command policy matched skill `github`"));
        assert!(guidance.contains("scripts/gh-pr.py"));
        assert!(guidance.contains("merge safely"));
    }
}
