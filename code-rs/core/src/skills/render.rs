use crate::skills::model::SkillMetadata;

pub fn render_skills_section(skills: &[SkillMetadata]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }

    let mut lines: Vec<String> = Vec::new();
    lines.push("## Skills".to_string());
    lines.push("A skill is a set of local instructions to follow that is stored in a `SKILL.md` file. Below is the list of skills that can be used. Each entry includes a name, description, and file path so you can open the source for full instructions when using a specific skill.".to_string());
    lines.push("### Available skills".to_string());

    for skill in skills {
        let path_str = skill.path.to_string_lossy().replace('\\', "/");
        let name = skill.name.as_str();
        let description = skill.description.as_str();
        lines.push(format!("- {name}: {description} (file: {path_str})"));
    }

    lines.push("### How to use skills".to_string());
    lines.push(
        r###"- Discovery: The list above is the skills available in this session (name + description + file path). Skill bodies live on disk at the listed paths.
- Trigger rules: If the user names a skill (with `$SkillName` or plain text) OR the task clearly matches a skill's description shown above, you must use that skill for that turn. Multiple mentions mean use them all. Do not carry skills across turns unless re-mentioned.
- Mandatory triggers: If a skill description says it MUST be used, treat that as a hard requirement in the described context. Open its `SKILL.md` before taking other investigative or implementation actions for that turn.
- Delegated triggers: If a skill description tells you to use another named skill for a subdomain, find that delegated skill in the Available Skills list above and open its `SKILL.md` before taking actions in that subdomain.
- Missing/blocked: If a named skill isn't in the list or the path can't be read, say so briefly and continue with the best fallback.
- How to use a skill (progressive disclosure):
  1) Open the selected skill's `SKILL.md`. Read only enough to follow the workflow.
  2) If `SKILL.md` points to extra folders such as `references/`, load only the specific files needed for the request; don't bulk-load everything.
  3) If `scripts/` exist, prefer running or patching them instead of retyping large code blocks.
  4) If `assets/` or templates exist, reuse them instead of recreating from scratch.
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
    use crate::skills::model::SkillScope;
    use std::path::PathBuf;

    #[test]
    fn rendered_skill_guidance_includes_binding_trigger_rules() {
        let rendered = render_skills_section(&[SkillMetadata {
            name: "github".to_string(),
            description:
                "Use for repository work. For durable planning, use github-plan.".to_string(),
            path: PathBuf::from("/skills/github/SKILL.md"),
            scope: SkillScope::User,
            content: String::new(),
        }, SkillMetadata {
            name: "chronicle".to_string(),
            description: "Use when recent work is ambiguous. This skill MUST be used.".to_string(),
            path: PathBuf::from("/skills/chronicle/SKILL.md"),
            scope: SkillScope::User,
            content: String::new(),
        }])
        .expect("skills section should render");

        assert!(rendered.contains("hard requirement in the described context"));
        assert!(rendered.contains("Open its `SKILL.md` before taking other investigative"));
        assert!(rendered.contains("If a skill description tells you to use another named skill"));
        assert!(rendered.contains("find that delegated skill in the Available Skills list"));
        assert!(!rendered.contains("After deciding to use a skill"));
    }
}
