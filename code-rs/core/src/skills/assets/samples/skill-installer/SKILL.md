---
name: skill-installer
description: Install Every Code skills into $CODE_HOME/skills from a curated list or a GitHub repo path. Use when a user asks to list installable skills, install a curated skill, or install a skill from another repo (including private repos).
metadata:
  short-description: Install curated skills from openai/skills or other repos
resources:
  - path: scripts/list-curated-skills.py
    kind: script
    description: List curated skills with installed annotations.
  - path: scripts/install-skill-from-github.py
    kind: script
    description: Install skills from GitHub repo paths or GitHub tree URLs.
  - path: scripts/github_utils.py
    kind: script
    description: Shared GitHub API and download helper code for installer scripts.
commands:
  - name: list-curated-skills
    resource_path: scripts/list-curated-skills.py
    example_argv: ["python", "scripts/list-curated-skills.py"]
    purpose: Print curated installable skills with installed annotations.
  - name: list-curated-skills-json
    resource_path: scripts/list-curated-skills.py
    example_argv: ["python", "scripts/list-curated-skills.py", "--format", "json"]
    purpose: Print curated installable skills as JSON.
  - name: install-skill-from-repo
    resource_path: scripts/install-skill-from-github.py
    example_argv: ["python", "scripts/install-skill-from-github.py", "--repo", "<owner>/<repo>", "--path", "<path/to/skill>"]
    purpose: Install one or more skills from GitHub repo paths.
  - name: install-skill-from-url
    resource_path: scripts/install-skill-from-github.py
    example_argv: ["python", "scripts/install-skill-from-github.py", "--url", "https://github.com/<owner>/<repo>/tree/<ref>/<path>"]
    purpose: Install a skill from a GitHub tree URL.
workflow_defaults:
  - name: install_location
    value: $CODE_HOME/skills or ~/.code/skills
    description: Default destination for installed skills.
  - name: curated_source
    value: https://github.com/openai/skills/tree/main/skills/.curated
    description: Default curated skill source.
  - name: default_ref
    value: main
    description: Default Git reference for GitHub skill installs.
  - name: default_method
    value: auto
    description: Default installer method before download/git fallback decisions.
---

# Skill Installer

Helps install skills. By default these are from https://github.com/openai/skills/tree/main/skills/.curated, but users can also provide other locations.

Use the structured commands in this skill's frontmatter based on the task:
- List curated skills when the user asks what is available, or if the user uses this skill without specifying what to do.
- Install from the curated list when the user provides a skill name.
- Install from another repo when the user provides a GitHub repo/path (including private repos).

Install skills with the helper scripts.

## Communication

When listing curated skills, output approximately as follows, depending on the context of the user's request:
"""
Skills from {repo}:
1. skill-1
2. skill-2 (already installed)
3. ...
Which ones would you like installed?
"""

After installing a skill, tell the user: "Restart Every Code to pick up new skills."

## Script Execution

All of the structured commands use network, so when running in the sandbox,
request escalation when running them.

## Behavior and Options

- Defaults to direct download for public GitHub repos.
- If download fails with auth/permission errors, falls back to git sparse checkout.
- Aborts if the destination skill directory already exists.
- Installs into `$CODE_HOME/skills/<skill-name>` (defaults to `~/.code/skills`; legacy `$CODEX_HOME`/`~/.codex` is still honored when `CODE_HOME` is unset).
- Multiple `--path` values install multiple skills in one run, each named from the path basename unless `--name` is supplied.
- Options: `--ref <ref>` (default `main`), `--dest <path>`, `--method auto|download|git`.

## Notes

- Curated listing is fetched from `https://github.com/openai/skills/tree/main/skills/.curated` via the GitHub API. If it is unavailable, explain the error and exit.
- Private GitHub repos can be accessed via existing git credentials or optional `GITHUB_TOKEN`/`GH_TOKEN` for download.
- Git fallback tries HTTPS first, then SSH.
- The skills at https://github.com/openai/skills/tree/main/skills/.system are preinstalled, so no need to help users install those. If they ask, just explain this. If they insist, you can download and overwrite.
- Installed annotations come from `$CODE_HOME/skills`.
