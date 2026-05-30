use crate::skills::model::SkillCommandMatcher;
use crate::skills::model::SkillCommandPolicy;
use crate::skills::model::SkillCommandPolicyAction;
use crate::skills::model::SkillCommandPolicyPreferred;
use crate::skills::model::SkillCommandPolicyPreferredKind;
use crate::skills::model::SkillMetadata;
use regex_lite::Regex;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) struct SkillCommandPolicyRuntime {
    policies: Vec<CompiledSkillCommandPolicy>,
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledSkillCommandPolicy {
    order: usize,
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
    related: Vec<SingleSkillCommandPolicyMatch<'a>>,
}

#[derive(Debug, Clone)]
struct SingleSkillCommandPolicyMatch<'a> {
    policy: &'a CompiledSkillCommandPolicy,
    matched_command: String,
    specificity: MatchSpecificity,
    matcher_width: usize,
}

impl SkillCommandPolicyRuntime {
    pub(crate) fn from_skills(skills: &[SkillMetadata]) -> Self {
        let mut policies = Vec::new();
        for skill in skills {
            let Some(policy) = skill.policy.as_ref() else {
                continue;
            };
            for command_policy in &policy.command_policies {
                if let Some(compiled) = compile_policy(skill, command_policy, policies.len()) {
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
        let mut matches = Vec::new();
        for policy in &self.policies {
            if has_confirm_prefix && policy.action == SkillCommandPolicyAction::RequireConfirm {
                continue;
            }
            let Some(candidate) = policy_matches(policy, &normalized) else {
                continue;
            };
            matches.push(candidate);
        }
        matches.sort_by(compare_policy_matches);
        let primary = matches.first()?.clone();
        Some(SkillCommandPolicyMatch {
            policy: primary.policy,
            matched_command: primary.matched_command,
            related: matches,
        })
    }
}

fn compare_policy_matches(
    left: &SingleSkillCommandPolicyMatch<'_>,
    right: &SingleSkillCommandPolicyMatch<'_>,
) -> std::cmp::Ordering {
    specificity_rank(left.specificity)
        .cmp(&specificity_rank(right.specificity))
        .then_with(|| right.matcher_width.cmp(&left.matcher_width))
        .then_with(|| left.policy.order.cmp(&right.policy.order))
        .then_with(|| left.policy.skill_name.cmp(&right.policy.skill_name))
        .then_with(|| left.policy.skill_path.cmp(&right.policy.skill_path))
        .then_with(|| left.policy.id.cmp(&right.policy.id))
}

impl SkillCommandPolicyMatch<'_> {
    pub(crate) fn guidance(&self, original_label: &str, original_value: &str) -> String {
        let policy = self.policy;
        let message = policy_message(policy);
        let mut lines = vec![
            format!(
                "Command policy matched skill `{}` ({}) at {}.",
                policy.skill_name,
                policy.id,
                policy.skill_path.display()
            ),
            "Selected by matcher specificity, matcher width, then stable skill load order.".to_string(),
            String::new(),
            message.to_string(),
            String::new(),
        ];

        let preferred = self.unique_preferred_actions();
        if !preferred.is_empty() {
            lines.push("Preferred actions:".to_string());
            for preferred in preferred {
                lines.push(format_preferred(preferred));
            }
            lines.push(String::new());
        }

        let related = self.related_policy_lines();
        if !related.is_empty() {
            lines.push("Also matched policies:".to_string());
            lines.extend(related);
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

    fn unique_preferred_actions(&self) -> Vec<&SkillCommandPolicyPreferred> {
        let mut seen = BTreeSet::new();
        let mut preferred = Vec::new();
        for policy_match in &self.related {
            for action in &policy_match.policy.preferred {
                if seen.insert(preferred_key(action)) {
                    preferred.push(action);
                }
            }
        }
        preferred
    }

    fn related_policy_lines(&self) -> Vec<String> {
        self.related
            .iter()
            .skip(1)
            .map(|policy_match| {
                let action = describe_action(policy_match.policy.action);
                let message = policy_message(policy_match.policy);
                format!(
                    "- skill `{}` ({}) at {}; action: {}; warning: {}; matched_command: {}",
                    policy_match.policy.skill_name,
                    policy_match.policy.id,
                    policy_match.policy.skill_path.display(),
                    action,
                    message,
                    policy_match.matched_command
                )
            })
            .collect()
    }
}

fn policy_message(policy: &CompiledSkillCommandPolicy) -> &str {
    policy.message.as_deref().unwrap_or(match policy.action {
        SkillCommandPolicyAction::RequirePreferred => {
            "A loaded skill owns this workflow; use one of the preferred actions instead."
        }
        SkillCommandPolicyAction::RequireConfirm => {
            "A loaded skill requires explicit confirmation before this raw command runs."
        }
        SkillCommandPolicyAction::Reject => "A loaded skill rejects this raw command shape.",
    })
}

fn describe_action(action: SkillCommandPolicyAction) -> &'static str {
    match action {
        SkillCommandPolicyAction::RequirePreferred => "require_preferred",
        SkillCommandPolicyAction::RequireConfirm => "require_confirm",
        SkillCommandPolicyAction::Reject => "reject",
    }
}

fn compile_policy(
    skill: &SkillMetadata,
    policy: &SkillCommandPolicy,
    order: usize,
) -> Option<CompiledSkillCommandPolicy> {
    let shell_regex = policy
        .matcher
        .shell_regex
        .as_ref()
        .and_then(|pattern| Regex::new(pattern).ok());
    Some(CompiledSkillCommandPolicy {
        order,
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
) -> Option<SingleSkillCommandPolicyMatch<'a>> {
    if let Some(argv_exact) = policy.matcher.argv_exact.as_ref() {
        for argv in normalized.argv_candidates() {
            if argv == argv_exact.as_slice() {
                return Some(SingleSkillCommandPolicyMatch {
                    policy,
                    matched_command: argv.join(" "),
                    specificity: MatchSpecificity::Exact,
                    matcher_width: argv_exact.len(),
                });
            }
        }
    }

    if let Some(argv_prefix) = policy.matcher.argv_prefix.as_ref() {
        for argv in normalized.argv_candidates() {
            if argv.starts_with(argv_prefix) {
                return Some(SingleSkillCommandPolicyMatch {
                    policy,
                    matched_command: argv.join(" "),
                    specificity: MatchSpecificity::Prefix,
                    matcher_width: argv_prefix.len(),
                });
            }
        }
    }

    if let Some(regex) = policy.shell_regex.as_ref() {
        for text in normalized.text_candidates() {
            if regex.is_match(&text) {
                return Some(SingleSkillCommandPolicyMatch {
                    policy,
                    matched_command: text.clone(),
                    specificity: MatchSpecificity::Regex,
                    matcher_width: 0,
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

fn preferred_key(preferred: &SkillCommandPolicyPreferred) -> String {
    let kind = match preferred.kind {
        SkillCommandPolicyPreferredKind::Script => "script",
        SkillCommandPolicyPreferredKind::Skill => "skill",
        SkillCommandPolicyPreferredKind::Command => "command",
    };
    let path = preferred
        .path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = preferred.name.as_deref().unwrap_or_default();
    let example = preferred.example_argv.join("\u{1f}");
    format!("{kind}\u{1f}{path}\u{1f}{name}\u{1f}{example}")
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
        runtime_from_skills(vec![skill("github", vec![policy], false)])
    }

    fn runtime_from_skills(skills: Vec<SkillMetadata>) -> SkillCommandPolicyRuntime {
        SkillCommandPolicyRuntime::from_skills(&skills)
    }

    fn skill(
        name: &str,
        command_policies: Vec<SkillCommandPolicy>,
        allow_implicit_invocation: bool,
    ) -> SkillMetadata {
        SkillMetadata {
            name: name.to_string(),
            description: format!("{name} workflows"),
            short_description: None,
            path: PathBuf::from(format!("/tmp/{name}/SKILL.md")),
            scope: SkillScope::User,
            content: String::new(),
            policy: Some(SkillPolicy {
                allow_implicit_invocation: Some(allow_implicit_invocation),
                command_policies,
            }),
        }
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

    fn policy_with_id(id: &str, matcher: SkillCommandMatcher) -> SkillCommandPolicy {
        let mut policy = policy(matcher);
        policy.id = id.to_string();
        policy
    }

    fn script_preferred(path: &str, purpose: &str) -> SkillCommandPolicyPreferred {
        SkillCommandPolicyPreferred {
            kind: SkillCommandPolicyPreferredKind::Script,
            path: Some(PathBuf::from(path)),
            name: None,
            example_argv: vec![path.to_string()],
            purpose: Some(purpose.to_string()),
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
    fn overlap_prefers_exact_over_prefix_and_includes_related_guidance() {
        let prefix_policy = policy_with_id(
            "prefix-policy",
            SkillCommandMatcher {
                argv_prefix: Some(vec!["gh".to_string(), "pr".to_string()]),
                ..Default::default()
            },
        );
        let exact_policy = policy_with_id(
            "exact-policy",
            SkillCommandMatcher {
                argv_exact: Some(vec!["gh".to_string(), "pr".to_string(), "merge".to_string()]),
                ..Default::default()
            },
        );
        let runtime = runtime_from_skills(vec![
            skill("github", vec![prefix_policy], true),
            skill("merge-helper", vec![exact_policy], false),
        ]);

        let guidance = runtime
            .check(&["gh".to_string(), "pr".to_string(), "merge".to_string()], false)
            .expect("match")
            .guidance("original_argv", "[\"gh\", \"pr\", \"merge\"]");

        assert!(guidance.contains("Command policy matched skill `merge-helper` (exact-policy)"));
        assert!(guidance.contains("Also matched policies:"));
        assert!(guidance.contains("skill `github` (prefix-policy)"));
    }

    #[test]
    fn overlap_prefers_prefix_over_regex() {
        let regex_policy = policy_with_id(
            "regex-policy",
            SkillCommandMatcher {
                shell_regex: Some("^gh\\s+pr\\s+merge\\b".to_string()),
                ..Default::default()
            },
        );
        let prefix_policy = policy_with_id(
            "prefix-policy",
            SkillCommandMatcher {
                argv_prefix: Some(vec!["gh".to_string(), "pr".to_string(), "merge".to_string()]),
                ..Default::default()
            },
        );
        let runtime = runtime_from_skills(vec![skill(
            "github",
            vec![regex_policy, prefix_policy],
            true,
        )]);

        let matched = runtime
            .check(
                &[
                    "bash".to_string(),
                    "-lc".to_string(),
                    "gh pr merge 244".to_string(),
                ],
                false,
            )
            .expect("match");

        assert_eq!(matched.policy.id, "prefix-policy");
    }

    #[test]
    fn equal_specificity_uses_stable_loader_order() {
        let first_policy = policy_with_id(
            "first-policy",
            SkillCommandMatcher {
                argv_prefix: Some(vec!["gh".to_string(), "issue".to_string()]),
                ..Default::default()
            },
        );
        let second_policy = policy_with_id(
            "second-policy",
            SkillCommandMatcher {
                argv_prefix: Some(vec!["gh".to_string(), "issue".to_string()]),
                ..Default::default()
            },
        );
        let runtime = runtime_from_skills(vec![
            skill("first", vec![first_policy], true),
            skill("second", vec![second_policy], true),
        ]);

        let guidance = runtime
            .check(&["gh".to_string(), "issue".to_string(), "list".to_string()], false)
            .expect("match")
            .guidance("original_argv", "[\"gh\", \"issue\", \"list\"]");

        assert!(guidance.contains("Command policy matched skill `first` (first-policy)"));
        assert!(guidance.contains("skill `second` (second-policy)"));
    }

    #[test]
    fn longer_prefix_beats_shorter_prefix() {
        let broad_policy = policy_with_id(
            "broad-policy",
            SkillCommandMatcher {
                argv_prefix: Some(vec!["gh".to_string(), "pr".to_string()]),
                ..Default::default()
            },
        );
        let narrow_policy = policy_with_id(
            "narrow-policy",
            SkillCommandMatcher {
                argv_prefix: Some(vec!["gh".to_string(), "pr".to_string(), "merge".to_string()]),
                ..Default::default()
            },
        );
        let runtime = runtime_from_skills(vec![skill(
            "github",
            vec![broad_policy, narrow_policy],
            true,
        )]);

        let matched = runtime
            .check(
                &[
                    "gh".to_string(),
                    "pr".to_string(),
                    "merge".to_string(),
                    "246".to_string(),
                ],
                false,
            )
            .expect("match");

        assert_eq!(matched.policy.id, "narrow-policy");
    }

    #[test]
    fn related_policy_lines_include_action_and_warning() {
        let preferred_policy = policy_with_id(
            "preferred-policy",
            SkillCommandMatcher {
                argv_exact: Some(vec!["gh".to_string(), "pr".to_string(), "merge".to_string()]),
                ..Default::default()
            },
        );
        let mut reject_policy = policy_with_id(
            "reject-policy",
            SkillCommandMatcher {
                argv_prefix: Some(vec!["gh".to_string(), "pr".to_string()]),
                ..Default::default()
            },
        );
        reject_policy.action = SkillCommandPolicyAction::Reject;
        reject_policy.message = Some("This raw PR command family is blocked.".to_string());
        let runtime = runtime_from_skills(vec![
            skill("github", vec![preferred_policy], true),
            skill("policy", vec![reject_policy], true),
        ]);

        let guidance = runtime
            .check(&["gh".to_string(), "pr".to_string(), "merge".to_string()], false)
            .expect("match")
            .guidance("original_argv", "[\"gh\", \"pr\", \"merge\"]");

        assert!(guidance.contains("Command policy matched skill `github` (preferred-policy)"));
        assert!(guidance.contains("action: reject"));
        assert!(guidance.contains("warning: This raw PR command family is blocked."));
    }

    #[test]
    fn overlap_deduplicates_preferred_actions() {
        let preferred = script_preferred("/tmp/github/scripts/gh-pr.py", "merge safely");
        let mut first_policy = policy_with_id(
            "first-policy",
            SkillCommandMatcher {
                argv_prefix: Some(vec!["gh".to_string(), "pr".to_string(), "merge".to_string()]),
                ..Default::default()
            },
        );
        first_policy.preferred = vec![preferred.clone()];
        let mut second_policy = policy_with_id(
            "second-policy",
            SkillCommandMatcher {
                shell_regex: Some("^gh\\s+pr\\s+merge\\b".to_string()),
                ..Default::default()
            },
        );
        second_policy.preferred = vec![preferred];
        let runtime = runtime_from_skills(vec![
            skill("github", vec![first_policy], true),
            skill("github-plan", vec![second_policy], true),
        ]);

        let guidance = runtime
            .check(
                &[
                    "bash".to_string(),
                    "-lc".to_string(),
                    "gh pr merge 244".to_string(),
                ],
                false,
            )
            .expect("match")
            .guidance("original_script", "gh pr merge 244");

        assert_eq!(guidance.matches("- script;").count(), 1);
        assert!(guidance.contains("/tmp/github/scripts/gh-pr.py"));
        assert!(guidance.contains("skill `github-plan` (second-policy)"));
    }

    #[test]
    fn manual_only_policy_participates_in_overlap_guidance() {
        let broad_policy = policy_with_id(
            "broad-policy",
            SkillCommandMatcher {
                argv_prefix: Some(vec!["gh".to_string()]),
                ..Default::default()
            },
        );
        let manual_policy = policy_with_id(
            "manual-policy",
            SkillCommandMatcher {
                argv_exact: Some(vec!["gh".to_string(), "issue".to_string(), "close".to_string()]),
                ..Default::default()
            },
        );
        let runtime = runtime_from_skills(vec![
            skill("github", vec![broad_policy], true),
            skill("manual-github", vec![manual_policy], false),
        ]);

        let matched = runtime
            .check(&["gh".to_string(), "issue".to_string(), "close".to_string()], false)
            .expect("match");

        assert_eq!(matched.policy.skill_name, "manual-github");
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
