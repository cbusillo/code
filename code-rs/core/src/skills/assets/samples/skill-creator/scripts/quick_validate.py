#!/usr/bin/env python3
"""
Quick validation script for skills - minimal version
"""

import sys
from pathlib import Path

import yaml

MAX_STRUCTURED_ITEMS_PER_SKILL = 64
MAX_STRUCTURED_ID_LEN = 96
MAX_STRUCTURED_PURPOSE_LEN = 256
MAX_STRUCTURED_VALUE_LEN = 512
MAX_COMMAND_ARGV_TOKENS = 32
MAX_COMMAND_TOKEN_LEN = 160
ALLOWED_TOP_LEVEL_KEYS = {
    "name",
    "description",
    "metadata",
    "policy",
    "resources",
    "commands",
    "workflow_defaults",
}
ALLOWED_RESOURCE_KINDS = {"script", "reference", "template", "asset"}
ALLOWED_COMMAND_SOURCES = {"skill", "repo", "external"}


def validate_skill(skill_path):
    """Basic validation of a skill"""
    skill_path = Path(skill_path)

    # Check SKILL.md exists
    skill_md = skill_path / "SKILL.md"
    if not skill_md.exists():
        return False, f"SKILL.md not found in {skill_path}"

    # Read and parse frontmatter
    content = skill_md.read_text()
    if not content.startswith("---"):
        return False, "SKILL.md must start with YAML frontmatter (---)"

    try:
        parts = content.split("---", 2)
        if len(parts) < 3:
            return False, "SKILL.md frontmatter must be closed with ---"
        frontmatter = yaml.safe_load(parts[1])
    except yaml.YAMLError as e:
        return False, f"Invalid YAML in frontmatter: {e}"

    # Validate required fields
    if not frontmatter or "name" not in frontmatter:
        return False, "Missing required field: name"
    if "description" not in frontmatter:
        return False, "Missing required field: description"

    unexpected_keys = set(frontmatter) - ALLOWED_TOP_LEVEL_KEYS
    if unexpected_keys:
        return False, f"Unexpected frontmatter key(s): {', '.join(sorted(unexpected_keys))}"

    name = frontmatter.get("name", "")
    if not isinstance(name, str) or not name.strip():
        return False, "Name must be a non-empty string"
    if not all(c.islower() or c.isdigit() or c == "-" for c in name):
        return False, "Name must be lowercase with only letters, digits, and hyphens"
    if name.startswith("-") or name.endswith("-") or "--" in name:
        return (
            False,
            f"Name '{name}' cannot start/end with hyphen or contain consecutive hyphens",
        )
    if len(name) > 64:
        return False, f"Name is too long ({len(name)} characters). Maximum is 64 characters."

    description = frontmatter.get("description", "")
    if not isinstance(description, str):
        return False, "Description must be a string"
    if not description.strip():
        return False, "Description must be non-empty"
    if len(description) > 1024:
        return (
            False,
            f"Description is too long ({len(description)} characters). Maximum is 1024 characters.",
        )

    structured_error = validate_structured_metadata(frontmatter, skill_path)
    if structured_error:
        return False, structured_error

    return True, "Skill validation passed"


def validate_structured_metadata(frontmatter, skill_path):
    resource_paths, resource_error = validate_resources(frontmatter.get("resources"), skill_path)
    if resource_error:
        return resource_error

    command_error = validate_commands(frontmatter.get("commands"), resource_paths, skill_path)
    if command_error:
        return command_error

    return validate_workflow_defaults(frontmatter.get("workflow_defaults"))


def validate_resources(resources, skill_path):
    if resources is None:
        return set(), None
    if not isinstance(resources, list):
        return set(), "resources must be a list"
    if len(resources) > MAX_STRUCTURED_ITEMS_PER_SKILL:
        return set(), f"resources must contain at most {MAX_STRUCTURED_ITEMS_PER_SKILL} entries"

    paths = set()
    for index, resource in enumerate(resources):
        field = f"resources[{index}]"
        if not isinstance(resource, dict):
            return paths, f"{field} must be a YAML dictionary"
        unexpected = set(resource) - {"path", "kind", "description"}
        if unexpected:
            return paths, f"Unexpected key(s) in {field}: {', '.join(sorted(unexpected))}"

        raw_path = resource.get("path")
        path_error = validate_relative_path(raw_path, f"{field}.path")
        if path_error:
            return paths, path_error
        assert isinstance(raw_path, str)
        raw_path = raw_path.strip()
        if raw_path in paths:
            return paths, f"{field}.path duplicates {raw_path}"
        if not (skill_path / raw_path).exists():
            return paths, f"{field}.path points to missing {raw_path}"
        paths.add(raw_path)

        if resource.get("kind") not in ALLOWED_RESOURCE_KINDS:
            allowed = ", ".join(sorted(ALLOWED_RESOURCE_KINDS))
            return paths, f"{field}.kind must be one of: {allowed}"
        desc_error = validate_optional_text(
            resource.get("description"),
            MAX_STRUCTURED_PURPOSE_LEN,
            f"{field}.description",
        )
        if desc_error:
            return paths, desc_error
    return paths, None


def validate_commands(commands, resource_paths, skill_path):
    if commands is None:
        return None
    if not isinstance(commands, list):
        return "commands must be a list"
    if len(commands) > MAX_STRUCTURED_ITEMS_PER_SKILL:
        return f"commands must contain at most {MAX_STRUCTURED_ITEMS_PER_SKILL} entries"

    names = set()
    for index, command in enumerate(commands):
        field = f"commands[{index}]"
        if not isinstance(command, dict):
            return f"{field} must be a YAML dictionary"
        unexpected = set(command) - {"name", "source", "resource_path", "example_argv", "purpose"}
        if unexpected:
            return f"Unexpected key(s) in {field}: {', '.join(sorted(unexpected))}"

        name = command.get("name")
        text_error = validate_required_text(name, MAX_STRUCTURED_ID_LEN, f"{field}.name")
        if text_error:
            return text_error
        assert isinstance(name, str)
        name = name.strip()
        if not all(c.islower() or c.isdigit() or c == "-" for c in name):
            return f"{field}.name must be lowercase with only letters, digits, and hyphens"
        if name in names:
            return f"{field}.name duplicates {name}"
        names.add(name)

        source = command.get("source", "skill")
        if source not in ALLOWED_COMMAND_SOURCES:
            allowed = ", ".join(sorted(ALLOWED_COMMAND_SOURCES))
            return f"{field}.source must be one of: {allowed}"

        resource_path = command.get("resource_path")
        if source == "skill":
            path_error = validate_relative_path(resource_path, f"{field}.resource_path")
            if path_error:
                return path_error
            assert isinstance(resource_path, str)
            resource_path = resource_path.strip()
            if resource_path not in resource_paths:
                return f"{field}.resource_path must be listed in resources"
            if not (skill_path / resource_path).is_file():
                return f"{field}.resource_path points to missing file {resource_path}"
        elif resource_path is not None:
            return f"{field}.resource_path is only allowed for source: skill"

        argv_error = validate_example_argv(command.get("example_argv"), f"{field}.example_argv")
        if argv_error:
            return argv_error
        purpose_error = validate_required_text(
            command.get("purpose"),
            MAX_STRUCTURED_PURPOSE_LEN,
            f"{field}.purpose",
        )
        if purpose_error:
            return purpose_error
    return None


def validate_workflow_defaults(defaults):
    if defaults is None:
        return None
    if not isinstance(defaults, list):
        return "workflow_defaults must be a list"
    if len(defaults) > MAX_STRUCTURED_ITEMS_PER_SKILL:
        return f"workflow_defaults must contain at most {MAX_STRUCTURED_ITEMS_PER_SKILL} entries"
    for index, default in enumerate(defaults):
        field = f"workflow_defaults[{index}]"
        if not isinstance(default, dict):
            return f"{field} must be a YAML dictionary"
        unexpected = set(default) - {"name", "value", "description"}
        if unexpected:
            return f"Unexpected key(s) in {field}: {', '.join(sorted(unexpected))}"
        name_error = validate_required_text(default.get("name"), MAX_STRUCTURED_ID_LEN, f"{field}.name")
        if name_error:
            return name_error
        value_error = validate_required_text(default.get("value"), MAX_STRUCTURED_VALUE_LEN, f"{field}.value")
        if value_error:
            return value_error
        desc_error = validate_optional_text(default.get("description"), MAX_STRUCTURED_PURPOSE_LEN, f"{field}.description")
        if desc_error:
            return desc_error
    return None


def validate_required_text(value, max_length, field):
    if not isinstance(value, str) or not value.strip():
        return f"{field} must be a non-empty string"
    if len(value.strip()) > max_length:
        return f"{field} is too long ({len(value.strip())} characters). Maximum is {max_length} characters."
    return None


def validate_optional_text(value, max_length, field):
    if value is None:
        return None
    return validate_required_text(value, max_length, field)


def validate_example_argv(argv, field):
    if not isinstance(argv, list) or not argv:
        return f"{field} must be a non-empty list"
    if len(argv) > MAX_COMMAND_ARGV_TOKENS:
        return f"{field} must contain at most {MAX_COMMAND_ARGV_TOKENS} tokens"
    for index, token in enumerate(argv):
        token_error = validate_required_text(token, MAX_COMMAND_TOKEN_LEN, f"{field}[{index}]")
        if token_error:
            return token_error
    return None


def validate_relative_path(value, field):
    if not isinstance(value, str) or not value.strip():
        return f"{field} must be a non-empty string"
    path = Path(value.strip())
    if path.is_absolute() or ".." in path.parts:
        return f"{field} must be relative and must not contain '..'"
    return None


def main():
    if len(sys.argv) < 2:
        print("Usage: python quick_validate.py <skill_directory>")
        sys.exit(1)

    valid, message = validate_skill(sys.argv[1])
    if valid:
        print(f"✅ {message}")
        sys.exit(0)
    else:
        print(f"❌ {message}")
        sys.exit(1)


if __name__ == "__main__":
    main()
