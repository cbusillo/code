use crate::exec::ExecToolCallOutput;
#[cfg(test)]
use codex_apply_patch::ApplyPatchAction;
#[cfg(test)]
use codex_apply_patch::ApplyPatchFileChange;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Stdio;
use std::time::Duration;

const DEFAULT_VALIDATION_TIMEOUT: Duration = Duration::from_secs(8);
const TYPECHECK_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_FINDINGS: usize = 12;
const MAX_OUTPUT_LINES: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidationFinding {
    tool: String,
    file: Option<PathBuf>,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidationSummary {
    findings: Vec<ValidationFinding>,
    checks_run: Vec<String>,
}

pub(crate) fn append_validation_summary(
    output: &mut ExecToolCallOutput,
    summary: &ValidationSummary,
) {
    let rendered = render_validation_summary(summary);
    if rendered.is_empty() {
        return;
    }

    append_section(&mut output.stdout.text, &rendered);
    append_section(&mut output.aggregated_output.text, &rendered);
}

#[cfg(test)]
pub(crate) fn run_apply_patch_validation(action: &ApplyPatchAction) -> Option<ValidationSummary> {
    run_post_apply_validation(action.cwd.clone(), changed_files(action))
}

pub(crate) fn run_post_apply_validation(
    cwd: PathBuf,
    changed_files: Vec<PathBuf>,
) -> Option<ValidationSummary> {
    if changed_files.is_empty() {
        return None;
    }

    let cwd = cwd.as_path();
    let python_env = python_runtime_env(cwd);
    let mut findings = Vec::new();
    let mut checks_run = BTreeSet::new();

    run_structural_checks(&changed_files, &mut findings, &mut checks_run);

    run_external_validator(
        cwd,
        "prettier",
        &["--check"],
        filter_files(&changed_files, is_prettier_path),
        None,
        &mut findings,
        &mut checks_run,
    );

    let markdown_files = filter_files(&changed_files, is_markdown_path);
    if !markdown_files.is_empty() {
        let before = checks_run.len();
        run_external_validator(
            cwd,
            "markdownlint",
            &[],
            markdown_files.clone(),
            None,
            &mut findings,
            &mut checks_run,
        );
        if checks_run.len() == before {
            run_external_validator(
                cwd,
                "markdownlint-cli2",
                &[],
                markdown_files,
                None,
                &mut findings,
                &mut checks_run,
            );
        }
    }

    run_external_validator(
        cwd,
        "shellcheck",
        &["-f", "gcc"],
        filter_files(&changed_files, |path| is_shell_script(path)),
        None,
        &mut findings,
        &mut checks_run,
    );
    run_external_validator(
        cwd,
        "shfmt",
        &["-d"],
        filter_files(&changed_files, |path| is_shell_script(path)),
        None,
        &mut findings,
        &mut checks_run,
    );

    let python_files = filter_files(&changed_files, |path| {
        path.extension().and_then(|ext| ext.to_str()) == Some("py")
    });
    run_external_validator(
        cwd,
        "ruff",
        &["check"],
        python_files.clone(),
        python_env.as_ref(),
        &mut findings,
        &mut checks_run,
    );
    run_external_validator(
        cwd,
        "mypy",
        &[],
        python_files.clone(),
        python_env.as_ref(),
        &mut findings,
        &mut checks_run,
    );
    run_external_validator(
        cwd,
        "pyright",
        &[],
        python_files,
        python_env.as_ref(),
        &mut findings,
        &mut checks_run,
    );

    if checks_run.is_empty() && findings.is_empty() {
        return None;
    }

    findings.truncate(MAX_FINDINGS);
    Some(ValidationSummary {
        findings,
        checks_run: checks_run.into_iter().collect(),
    })
}

fn append_section(existing: &mut String, section: &str) {
    if existing.trim().is_empty() {
        existing.push_str(section);
        return;
    }
    existing.push_str("\n\n");
    existing.push_str(section);
}

fn render_validation_summary(summary: &ValidationSummary) -> String {
    let mut lines = Vec::new();
    if summary.findings.is_empty() {
        lines.push("Validate New Code: no issues".to_string());
    } else {
        lines.push(format!(
            "Validate New Code: {} issue(s)",
            summary.findings.len()
        ));
        for finding in &summary.findings {
            let mut location = String::new();
            if let Some(file) = &finding.file {
                location = format!(" {}", file.display());
            }
            lines.push(format!(
                "- {}{}: {}",
                finding.tool, location, finding.message
            ));
        }
    }

    if !summary.checks_run.is_empty() {
        lines.push(format!("Checks run: {}", summary.checks_run.join(", ")));
    }

    lines.join("\n")
}

#[cfg(test)]
fn changed_files(action: &ApplyPatchAction) -> Vec<PathBuf> {
    action
        .changes()
        .iter()
        .filter_map(|(path, change)| match change {
            ApplyPatchFileChange::Add { .. } => Some(path.clone()),
            ApplyPatchFileChange::Update { move_path, .. } => {
                Some(move_path.clone().unwrap_or_else(|| path.clone()))
            }
            ApplyPatchFileChange::Delete { .. } => None,
        })
        .collect()
}

fn run_structural_checks(
    changed_files: &[PathBuf],
    findings: &mut Vec<ValidationFinding>,
    checks_run: &mut BTreeSet<String>,
) {
    for path in changed_files {
        let Some(contents) = fs::read_to_string(path).ok() else {
            continue;
        };
        match path.extension().and_then(|ext| ext.to_str()).unwrap_or_default() {
            "json" => {
                checks_run.insert("json-parse".to_string());
                if let Err(err) = serde_json::from_str::<serde_json::Value>(&contents) {
                    findings.push(ValidationFinding {
                        tool: "json-parse".to_string(),
                        file: Some(path.clone()),
                        message: format!("invalid JSON: {err}"),
                    });
                }
            }
            "toml" => {
                checks_run.insert("toml-parse".to_string());
                if let Err(err) = toml::from_str::<toml::Value>(&contents) {
                    findings.push(ValidationFinding {
                        tool: "toml-parse".to_string(),
                        file: Some(path.clone()),
                        message: format!("invalid TOML: {err}"),
                    });
                }
            }
            "yml" | "yaml" => {
                checks_run.insert("yaml-parse".to_string());
                if let Err(err) = serde_yaml::from_str::<serde_yaml::Value>(&contents) {
                    findings.push(ValidationFinding {
                        tool: "yaml-parse".to_string(),
                        file: Some(path.clone()),
                        message: format!("invalid YAML: {err}"),
                    });
                }
            }
            _ => {}
        }
    }
}

fn run_external_validator(
    cwd: &Path,
    tool: &str,
    base_args: &[&str],
    files: Vec<PathBuf>,
    env: Option<&ValidationEnv>,
    findings: &mut Vec<ValidationFinding>,
    checks_run: &mut BTreeSet<String>,
) {
    if files.is_empty() {
        return;
    }

    let Some(executable) = resolve_executable(tool, env.and_then(|value| value.path.as_deref())) else {
        return;
    };

    checks_run.insert(tool.to_string());
    let workdir = validator_workdir(cwd, tool, &files);
    let timeout = if matches!(tool, "mypy" | "pyright") {
        TYPECHECK_TIMEOUT
    } else {
        DEFAULT_VALIDATION_TIMEOUT
    };

    let mut command = Command::new(executable);
    command.current_dir(&workdir);
    command.args(base_args);
    for path in &files {
        if let Ok(relative) = path.strip_prefix(&workdir) {
            command.arg(relative);
        } else {
            command.arg(path);
        }
    }
    if let Some(env) = env {
        command.env("PATH", &env.path_value);
        command.env("VIRTUAL_ENV", &env.virtualenv_value);
    }

    match run_with_timeout(command, timeout) {
        Some(output) if output.status.success() => {}
        Some(output) => {
            let lines = collect_output_lines(&output.stdout, &output.stderr);
            if lines.is_empty() {
                findings.push(ValidationFinding {
                    tool: tool.to_string(),
                    file: None,
                    message: format!("{tool} failed with no output"),
                });
            } else {
                for line in lines.into_iter().take(MAX_OUTPUT_LINES) {
                    findings.push(ValidationFinding {
                        tool: tool.to_string(),
                        file: None,
                        message: line,
                    });
                }
            }
        }
        None => {
            findings.push(ValidationFinding {
                tool: tool.to_string(),
                file: None,
                message: format!("{tool} timed out after {}s", timeout.as_secs()),
            });
        }
    }
}

fn filter_files(
    changed_files: &[PathBuf],
    predicate: impl Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    changed_files
        .iter()
        .filter(|path| predicate(path))
        .cloned()
        .collect()
}

fn is_prettier_path(path: &Path) -> bool {
    let prettier_exts = [
        "js", "jsx", "ts", "tsx", "json", "css", "scss", "less", "html", "yml", "yaml",
        "md", "mdx",
    ];
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| prettier_exts.contains(&ext))
    {
        return true;
    }

    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                ".prettierrc"
                    | ".prettierrc.json"
                    | ".prettierrc.json5"
                    | ".prettierrc.yml"
                    | ".prettierrc.yaml"
                    | ".prettierrc.js"
                    | ".prettierrc.cjs"
                    | ".prettierrc.mjs"
                    | ".prettierrc.ts"
                    | "prettier.config.js"
                    | "prettier.config.cjs"
                    | "prettier.config.mjs"
                    | "prettier.config.ts"
            )
        })
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("md")
}

fn is_shell_script(path: &Path) -> bool {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("sh") => true,
        _ => fs::read(path)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .is_some_and(|contents| contents.starts_with("#!/")),
    }
}

fn validator_workdir(cwd: &Path, tool: &str, files: &[PathBuf]) -> PathBuf {
    let config_candidates: &[&str] = match tool {
        "prettier" => &[
            ".prettierrc",
            ".prettierrc.json",
            ".prettierrc.json5",
            ".prettierrc.yml",
            ".prettierrc.yaml",
            ".prettierrc.js",
            ".prettierrc.cjs",
            ".prettierrc.mjs",
            ".prettierrc.ts",
            "prettier.config.js",
            "prettier.config.cjs",
            "prettier.config.mjs",
            "prettier.config.ts",
        ],
        "markdownlint" | "markdownlint-cli2" => &[
            ".markdownlint.json",
            ".markdownlint.yaml",
            ".markdownlint.yml",
            ".markdownlint-cli2.jsonc",
        ],
        "ruff" => &["ruff.toml", ".ruff.toml", "pyproject.toml"],
        "mypy" => &["mypy.ini", ".mypy.ini", "pyproject.toml"],
        "pyright" => &["pyrightconfig.json", "pyproject.toml"],
        _ => &[],
    };

    if let Some(dir) = find_nearest_config_dir(files, config_candidates) {
        return dir;
    }

    common_ancestor(files).unwrap_or_else(|| cwd.to_path_buf())
}

fn find_nearest_config_dir(files: &[PathBuf], candidates: &[&str]) -> Option<PathBuf> {
    for file in files {
        let mut current = file.parent().map(Path::to_path_buf);
        while let Some(dir) = current {
            for candidate in candidates {
                if dir.join(candidate).exists() {
                    return Some(dir);
                }
            }
            current = dir.parent().map(Path::to_path_buf);
        }
    }
    None
}

fn common_ancestor(files: &[PathBuf]) -> Option<PathBuf> {
    let mut components = files
        .first()?
        .parent()?
        .ancestors()
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    components.reverse();
    let mut best = None;
    for candidate in components {
        if files.iter().all(|path| path.starts_with(&candidate)) {
            best = Some(candidate);
        }
    }
    best
}

fn resolve_executable(tool: &str, path_override: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let paths = path_override
        .map(OsString::from)
        .or_else(|| std::env::var_os("PATH"))?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(tool);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn run_with_timeout(mut command: Command, timeout: Duration) -> Option<CommandCapture> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let Ok(mut child) = command.spawn() else {
        return None;
    };

    let start = std::time::Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let stdout = child.wait_with_output().ok()?;
            return Some(CommandCapture {
                status,
                stdout: stdout.stdout,
                stderr: stdout.stderr,
            });
        }

        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }

        std::thread::sleep(Duration::from_millis(40));
    }
}

fn collect_output_lines(stdout: &[u8], stderr: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    lines.extend(String::from_utf8_lossy(stdout).lines().map(str::to_string));
    lines.extend(String::from_utf8_lossy(stderr).lines().map(str::to_string));
    lines.retain(|line| !line.trim().is_empty());
    lines
}

#[derive(Debug, Clone)]
struct ValidationEnv {
    path: Option<OsString>,
    path_value: String,
    virtualenv_value: String,
}

fn python_runtime_env(cwd: &Path) -> Option<ValidationEnv> {
    let virtualenv_root = find_virtualenv_root(cwd)?;
    let bin_dir = virtualenv_bin_dir(&virtualenv_root)?;
    let path = prepend_path(bin_dir.as_path());
    Some(ValidationEnv {
        path: Some(OsString::from(&path)),
        path_value: path,
        virtualenv_value: virtualenv_root.to_string_lossy().to_string(),
    })
}

fn find_virtualenv_root(cwd: &Path) -> Option<PathBuf> {
    for dir in cwd.ancestors() {
        for name in [".venv", "venv"] {
            let candidate = dir.join(name);
            if virtualenv_bin_dir(&candidate).is_some() {
                return Some(candidate);
            }
        }
    }
    None
}

fn virtualenv_bin_dir(root: &Path) -> Option<PathBuf> {
    let unix_bin = root.join("bin");
    if unix_bin.is_dir() {
        return Some(unix_bin);
    }
    let windows_bin = root.join("Scripts");
    if windows_bin.is_dir() {
        return Some(windows_bin);
    }
    None
}

fn prepend_path(bin_dir: &Path) -> String {
    let existing = std::env::var_os("PATH");
    let mut parts = vec![bin_dir.to_path_buf()];
    if let Some(existing) = existing {
        parts.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(parts)
        .unwrap_or_else(|_| bin_dir.as_os_str().to_os_string())
        .to_string_lossy()
        .to_string()
}

struct CommandCapture {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::StreamOutput;
    use codex_apply_patch::ApplyPatchAction;
    use pretty_assertions::assert_eq;
    use serial_test::serial;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    #[test]
    fn append_validation_summary_appends_to_exec_output() {
        let mut output = ExecToolCallOutput {
            stdout: StreamOutput::new("Success. Updated the following files:\nA file.ts".to_string()),
            aggregated_output: StreamOutput::new(
                "Success. Updated the following files:\nA file.ts".to_string(),
            ),
            ..ExecToolCallOutput::default()
        };
        let summary = ValidationSummary {
            findings: Vec::new(),
            checks_run: vec!["prettier".to_string()],
        };

        append_validation_summary(&mut output, &summary);

        assert!(output.stdout.text.contains("Validate New Code: no issues"));
        assert!(output.aggregated_output.text.contains("Checks run: prettier"));
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn run_apply_patch_validation_runs_prettier_from_repo_root() {
        let repo = TempDir::new().expect("tempdir");
        let bin_dir = repo.path().join("bin");
        fs::create_dir_all(&bin_dir).expect("create bin dir");
        write_shell_tool(
            &bin_dir.join("prettier"),
            "#!/bin/sh\n[ -f .prettierrc ] || { echo missing prettier config; exit 1; }\nexit 0\n",
        );
        fs::write(repo.path().join(".prettierrc"), "{}\n").expect("write config");

        let path_guard = ScopedEnvVar::set("PATH", Some(bin_dir.into_os_string()));
        let action = ApplyPatchAction::new_add_for_test(
            &repo.path().join("src/index.ts"),
            "const x = 1;\n".to_string(),
        );
        fs::create_dir_all(repo.path().join("src")).expect("create src dir");
        fs::write(repo.path().join("src/index.ts"), "const x = 1;\n").expect("write file");

        let summary = run_apply_patch_validation(&action).expect("summary");
        drop(path_guard);

        assert!(summary.findings.is_empty(), "unexpected findings: {summary:?}");
        assert!(summary.checks_run.contains(&"prettier".to_string()));
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn run_apply_patch_validation_prefers_project_virtualenv_python_tools() {
        let repo = TempDir::new().expect("tempdir");
        let src_dir = repo.path().join("pkg");
        let venv_bin = repo.path().join(".venv/bin");
        fs::create_dir_all(&src_dir).expect("create src dir");
        fs::create_dir_all(&venv_bin).expect("create venv bin");
        write_shell_tool(
            &venv_bin.join("ruff"),
            "#!/bin/sh\nprintf 'ruff-from-venv %s\\n' \"$VIRTUAL_ENV\"\nexit 1\n",
        );

        let action = ApplyPatchAction::new_add_for_test(
            &src_dir.join("check.py"),
            "print('hi')\n".to_string(),
        );
        fs::write(src_dir.join("check.py"), "print('hi')\n").expect("write file");

        let summary = run_apply_patch_validation(&action).expect("summary");

        assert!(summary.checks_run.contains(&"ruff".to_string()));
        assert!(summary.findings.iter().any(|finding| {
            finding.tool == "ruff"
                && finding
                    .message
                    .contains(repo.path().join(".venv").to_string_lossy().as_ref())
        }));
    }

    #[test]
    fn render_validation_summary_includes_findings_and_checks() {
        let summary = ValidationSummary {
            findings: vec![ValidationFinding {
                tool: "prettier".to_string(),
                file: Some(PathBuf::from("src/index.ts")),
                message: "line too long".to_string(),
            }],
            checks_run: vec!["prettier".to_string()],
        };

        assert_eq!(
            render_validation_summary(&summary),
            "Validate New Code: 1 issue(s)\n- prettier src/index.ts: line too long\nChecks run: prettier"
        );
    }

    #[cfg(unix)]
    fn write_shell_tool(path: &Path, script: &str) {
        fs::write(path, script).expect("write tool");
        let mut permissions = fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod");
    }

    #[cfg(unix)]
    struct ScopedEnvVar {
        key: &'static str,
        previous: Option<OsString>,
    }

    #[cfg(unix)]
    impl ScopedEnvVar {
        fn set(key: &'static str, value: Option<OsString>) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                match &value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
            Self { key, previous }
        }
    }

    #[cfg(unix)]
    impl Drop for ScopedEnvVar {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(previous) => std::env::set_var(self.key, previous),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }
}
