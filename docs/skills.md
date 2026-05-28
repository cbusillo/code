# Skills

> **Warning:** This is an experimental and non-stable feature. If you depend on it, please expect breaking changes over the coming weeks and understand that there is currently no guarantee that this works well. Use at your own risk!

Every Code can automatically discover reusable "skills" you keep on disk. A skill is a small bundle with a name, description, and optional body of instructions you can open when needed. Every Code injects only implicitly invokable skill metadata into runtime context; the body stays on disk.

## Enable skills

Skills are behind the experimental `skills` feature flag and are enabled by default.

- Disable in config (preferred): add the following to `$CODE_HOME/config.toml` (usually `~/.code/config.toml`) and restart Every Code:

  ```toml
  [features]
  skills = false
  ```

- Override for a single run when disabled in config: launch Every Code with `code --enable skills`.

## Where skills live

- Discovery roots (highest precedence first):
  - Repo: `.agents/skills/**/SKILL.md` from the current working directory up to the repo root.
  - Repo (legacy): `.codex/skills/**/SKILL.md` from the current working directory up to the repo root.
  - User agents: `$HOME/.agents/skills/**/SKILL.md`.
  - User Code home: `$CODE_HOME/skills/**/SKILL.md` (usually `~/.code/skills`; legacy `$CODEX_HOME`/`~/.codex` is still read for compatibility).
  - System: bundled skills under `$CODE_HOME/skills/.system/**/SKILL.md`.
  - Admin (optional): `/etc/codex/skills/**/SKILL.md`.
- Discovery is recursive and only files named exactly `SKILL.md` count.
- Hidden entries are skipped.
- Symlinked directories are followed for repo, user, and admin roots. Symlinked files are ignored.
- Duplicate skill names are resolved by root precedence above (first match wins).
- Skills are rendered by name, then path for stability.

## File format

- YAML frontmatter + body.
  - Required:
    - `name` (non-empty, <=64 chars, sanitized to one line)
    - `description` (non-empty, <=1024 chars, sanitized to one line)
  - Optional:
    - `metadata.short-description` (non-empty when present, <=160 chars)
    - `policy.allow_implicit_invocation` (`true` by default; set `false` for manual-only skills)
  - The body can contain any Markdown; it is not injected into context until the skill is opened.

## Loading and rendering

- Loaded once at startup.
- If valid implicitly invokable skills exist, Every Code appends a runtime-only `## Skills` section after `AGENTS.md`, one bullet per skill: `- <name>: <description> (file: /absolute/path/to/SKILL.md)`.
- If no valid skills exist, the section is omitted. On-disk files are never modified.

## Using skills

- Mention a skill by name in a message using `$<skill-name>`.
- In the TUI, you can also use `/skills` to browse and insert skills.

## Validation and errors

- Invalid skills (missing/invalid YAML, empty/over-length fields) trigger a blocking, dismissible startup modal in the TUI that lists each path and error. Errors are also logged. You can dismiss to continue (invalid skills are ignored) or exit. Fix SKILL.md files and restart to clear the modal.

## Create a skill

1. Create `~/.code/skills/<skill-name>/`.
2. Add `SKILL.md`:

   ```markdown
   ---
   name: your-skill-name
   description: what it does and when to use it
   metadata:
     short-description: short picker label (optional)
   policy:
     allow_implicit_invocation: true
   ---

   # Optional body
   Add instructions, references, examples, or scripts (kept on disk).
   ```

3. Keep frontmatter fields within their limits; avoid newlines in single-line fields.
4. Restart Every Code to load the new skill.

## Example

Create `~/.code/skills/pdf-processing/SKILL.md`:

```markdown
---
name: pdf-processing
description: Extract text and tables from PDFs; use when PDFs, forms, or document extraction are mentioned.
metadata:
  short-description: PDF extraction helper
---

# PDF Processing
- Use pdfplumber to extract text.
- For form filling, see FORMS.md.
```
