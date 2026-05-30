use crate::skills::model::SkillMetadata;
use crate::skills::command_policy::render_command_policy_summary;

pub fn render_skills_section(skills: &[SkillMetadata]) -> Option<String> {
    let implicit_skills: Vec<&SkillMetadata> = skills
        .iter()
        .filter(|skill| skill.allow_implicit_invocation())
        .collect();
    let manual_skills: Vec<&SkillMetadata> = skills
        .iter()
        .filter(|skill| !skill.allow_implicit_invocation())
        .collect();

    if implicit_skills.is_empty() && manual_skills.is_empty() {
        return None;
    }

    let mut lines: Vec<String> = Vec::new();
    lines.push("## Skills".to_string());
    lines.push("A skill is a set of local instructions to follow that is stored in a `SKILL.md` file. Below are the implicitly invokable skills whose descriptions can trigger use. Skill bodies live on disk and should be opened only when the trigger rules say to use the skill.".to_string());

    if !implicit_skills.is_empty() {
        lines.push("### Available skills".to_string());

        for skill in implicit_skills {
            let path_str = skill.path.to_string_lossy().replace('\\', "/");
            let name = skill.name.as_str();
            let description = skill.description.as_str();
            lines.push(format!("- {name}: {description} (file: {path_str})"));
            lines.extend(render_command_policy_summary(skill));
        }
    }

    if !manual_skills.is_empty() {
        lines.push("### Manual-only skills".to_string());
        lines.push("These skills are discoverable but not implicitly invokable. Use them only when the user explicitly names them with `$<skill-name>` or when the skill body has already been injected into the conversation.".to_string());
        for skill in manual_skills {
            let name = skill.name.as_str();
            lines.push(format!("- {name}"));
        }
    }

    lines.push("### How to use skills".to_string());
    lines.push(
        r###"- Discovery: The "Available skills" list is for implicit routing; descriptions there can trigger skill use. The "Manual-only skills" list is name-only so the user can explicitly invoke those skills without adding their instructions to every turn.
- Trigger rules: If the user names an available skill in plain text, or the task clearly matches an available skill's description, you must use that skill for that turn. If the user explicitly names any skill with `$<skill-name>`, use it for that turn when the skill body is available or can be opened. Multiple mentions mean use them all. Do not carry skills across turns unless re-mentioned.
- Missing/blocked: If an explicitly named skill body is already injected in the conversation, follow it even if the skill is not in the available-skills list. If a named skill cannot be opened or its body is unavailable, say so briefly and continue with the best fallback.
- How to use a skill (progressive disclosure):
  1) After deciding to use a skill, open its `SKILL.md`. Read only enough to follow the workflow.
  2) When `SKILL.md` references bundled skill resources or scripts with relative paths such as `scripts/foo.py`, resolve them relative to the directory containing that `SKILL.md` first, and only consider other paths if needed.
  3) If `SKILL.md` points to extra folders such as `references/`, load only the specific files needed for the request; don't bulk-load everything.
  4) If `scripts/` exist, prefer running or patching them instead of retyping large code blocks.
  5) If `assets/` or templates exist, reuse them instead of recreating from scratch.
- Coordination and sequencing:
  - If multiple skills apply, choose the minimal set that covers the request and state the order you'll use them.
  - Announce which skill(s) you're using and why (one short line). If you skip an obvious skill, say why.
- Context hygiene:
  - Keep context small: summarize long sections instead of pasting them; only load extra files when needed.
  - Avoid deep reference-chasing: prefer opening only files directly linked from `SKILL.md` unless you're blocked.
  - When variants exist (frameworks, providers, domains), pick only the relevant reference file(s) and note that choice.
- Safety and fallback: If a skill can't be applied cleanly (missing files, unclear instructions), state the issue, pick the next-best approach, and continue."###
            .to_string(),
    );

    Some(lines.join("\n"))
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
    use std::path::PathBuf;

    fn skill(name: &str, allow_implicit_invocation: Option<bool>) -> SkillMetadata {
        SkillMetadata {
            name: name.to_string(),
            description: format!("{name} description"),
            short_description: None,
            path: PathBuf::from(format!("/tmp/{name}/SKILL.md")),
            scope: SkillScope::User,
            content: String::new(),
            policy: allow_implicit_invocation.map(|allow_implicit_invocation| SkillPolicy {
                allow_implicit_invocation: Some(allow_implicit_invocation),
                command_policies: Vec::new(),
            }),
        }
    }

    #[test]
    fn render_skills_section_omits_manual_only_skills() {
        let rendered = render_skills_section(&[
            skill("implicit", None),
            skill("manual", Some(false)),
        ])
        .expect("implicit skill should render");

        assert!(rendered.contains("- implicit: implicit description"));
        assert!(!rendered.contains("- manual: manual description"));
        assert!(rendered.contains("### Manual-only skills"));
        assert!(rendered.contains("- manual"));
        assert!(!rendered.contains("manual description"));
    }

    #[test]
    fn render_skills_section_lists_only_manual_skill_names() {
        let rendered = render_skills_section(&[skill("manual", Some(false))]);

        let rendered = rendered.expect("manual skill names should render");
        assert!(!rendered.contains("### Available skills"));
        assert!(rendered.contains("### Manual-only skills"));
        assert!(rendered.contains("- manual"));
        assert!(!rendered.contains("manual description"));
    }

    #[test]
    fn render_skills_section_avoids_exhaustive_missing_skill_language() {
        let rendered = render_skills_section(&[
            skill("implicit", None),
            skill("manual", Some(false)),
        ])
        .expect("skills should render");

        assert!(!rendered.contains("the skills available in this session"));
        assert!(!rendered.contains("isn't in the list"));
        assert!(rendered.contains("If an explicitly named skill body is already injected"));
    }

    #[test]
    fn render_skills_section_uses_full_description_for_model_context() {
        let mut skill = skill("compact", None);
        skill.description = "full trigger description".to_string();
        skill.short_description = Some("compact UI summary".to_string());

        let rendered = render_skills_section(&[skill]).expect("skill should render");

        assert!(rendered.contains("- compact: full trigger description"));
        assert!(!rendered.contains("compact UI summary"));
    }

    #[test]
    fn render_skills_section_includes_command_policy_guidance() {
        let mut skill = skill("github", None);
        skill.policy = Some(SkillPolicy {
            allow_implicit_invocation: None,
            command_policies: vec![SkillCommandPolicy {
                id: "prefer-pr-merge".to_string(),
                matcher: SkillCommandMatcher {
                    argv_prefix: Some(vec![
                        "gh".to_string(),
                        "pr".to_string(),
                        "merge".to_string(),
                    ]),
                    ..Default::default()
                },
                action: SkillCommandPolicyAction::RequirePreferred,
                message: Some("use helper".to_string()),
                preferred: vec![SkillCommandPolicyPreferred {
                    kind: SkillCommandPolicyPreferredKind::Script,
                    path: Some(PathBuf::from("/tmp/github/scripts/gh-pr.py")),
                    name: None,
                    example_argv: vec!["scripts/gh-pr.py".to_string(), "merge".to_string()],
                    purpose: Some("merge through helper".to_string()),
                }],
            }],
        });

        let rendered = render_skills_section(&[skill]).expect("skill should render");

        assert!(rendered.contains("`gh pr merge ...`: use - script"));
        assert!(rendered.contains("scripts/gh-pr.py"));
    }

    #[test]
    fn render_skills_section_resolves_relative_paths_from_skill_dir() {
        let rendered = render_skills_section(&[skill("helper", None)])
            .expect("implicit skill should render");

        assert!(rendered.contains(
            "When `SKILL.md` references bundled skill resources or scripts with relative paths such as `scripts/foo.py`, resolve them relative to the directory containing that `SKILL.md` first, and only consider other paths if needed."
        ));
    }
}
