//! Helpers for applying unified diffs using the system `git` binary.
//!
//! The entry point is [`apply_git_patch`], which writes a diff to a temporary
//! file, shells out to `git apply` with the right flags, and then parses the
//! command’s output into structured details. Callers can opt into dry-run
//! mode via [`ApplyGitRequest::preflight`] and inspect the resulting paths to
//! learn what would change before applying for real.

use once_cell::sync::Lazy;
use regex::Regex;
use std::io;
use std::path::Path;
use std::path::PathBuf;

/// Parameters for invoking [`apply_git_patch`].
#[derive(Debug, Clone)]
pub struct ApplyGitRequest {
    pub cwd: PathBuf,
    pub diff: String,
    pub revert: bool,
    pub preflight: bool,
}

/// Result of running [`apply_git_patch`], including paths gleaned from stdout/stderr.
#[derive(Debug, Clone)]
pub struct ApplyGitResult {
    pub exit_code: i32,
    pub applied_paths: Vec<String>,
    pub skipped_paths: Vec<String>,
    pub conflicted_paths: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    pub cmd_for_log: String,
}

/// Apply a unified diff to the target repository by shelling out to `git apply`.
///
/// When [`ApplyGitRequest::preflight`] is `true`, this behaves like `git apply --check` and
/// leaves the working tree untouched while still parsing the command output for diagnostics.
pub fn apply_git_patch(req: &ApplyGitRequest) -> io::Result<ApplyGitResult> {
    let git_root = resolve_git_root(&req.cwd)?;

    // Write unified diff into a temporary file
    let (tmpdir, patch_path) = write_temp_patch(&req.diff)?;
    // Keep tmpdir alive until function end to ensure the file exists
    let _guard = tmpdir;

    if req.revert && !req.preflight {
        // Stage WT paths first to avoid index mismatch on revert.
        stage_paths(&git_root, &req.diff)?;
    }

    // Build git args
    let mut args: Vec<String> = vec!["apply".into(), "--3way".into()];
    if req.revert {
        args.push("-R".into());
    }

    // Optional: additional git config via env knob (defaults OFF)
    let mut cfg_parts: Vec<String> = Vec::new();
    if let Ok(cfg) = std::env::var("CODEX_APPLY_GIT_CFG") {
        for pair in cfg.split(',') {
            let p = pair.trim();
            if p.is_empty() || !p.contains('=') {
                continue;
            }
            cfg_parts.push("-c".into());
            cfg_parts.push(p.to_string());
        }
    }

    args.push(patch_path.to_string_lossy().to_string());

    // Optional preflight: dry-run only; do not modify working tree
    if req.preflight {
        let mut check_args = vec!["apply".to_string(), "--check".to_string()];
        if req.revert {
            check_args.push("-R".to_string());
        }
        check_args.push(patch_path.to_string_lossy().to_string());
        let rendered = render_command_for_log(&git_root, &cfg_parts, &check_args);
        let (c_code, c_out, c_err) = run_git(&git_root, &cfg_parts, &check_args)?;
        let (mut applied_paths, mut skipped_paths, mut conflicted_paths) =
            parse_git_apply_output(&c_out, &c_err);
        applied_paths.sort();
        applied_paths.dedup();
        skipped_paths.sort();
        skipped_paths.dedup();
        conflicted_paths.sort();
        conflicted_paths.dedup();
        return Ok(ApplyGitResult {
            exit_code: c_code,
            applied_paths,
            skipped_paths,
            conflicted_paths,
            stdout: c_out,
            stderr: c_err,
            cmd_for_log: rendered,
        });
    }

    let cmd_for_log = render_command_for_log(&git_root, &cfg_parts, &args);
    let (code, stdout, stderr) = run_git(&git_root, &cfg_parts, &args)?;

    let (mut applied_paths, mut skipped_paths, mut conflicted_paths) =
        parse_git_apply_output(&stdout, &stderr);
    applied_paths.sort();
    applied_paths.dedup();
    skipped_paths.sort();
    skipped_paths.dedup();
    conflicted_paths.sort();
    conflicted_paths.dedup();

    Ok(ApplyGitResult {
        exit_code: code,
        applied_paths,
        skipped_paths,
        conflicted_paths,
        stdout,
        stderr,
        cmd_for_log,
    })
}

fn resolve_git_root(cwd: &Path) -> io::Result<PathBuf> {
    let out = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .current_dir(cwd)
        .output()?;
    let code = out.status.code().unwrap_or(-1);
    if code != 0 {
        return Err(io::Error::other(format!(
            "not a git repository (exit {}): {}",
            code,
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(PathBuf::from(root))
}

fn write_temp_patch(diff: &str) -> io::Result<(tempfile::TempDir, PathBuf)> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("patch.diff");
    std::fs::write(&path, diff)?;
    Ok((dir, path))
}

fn run_git(cwd: &Path, git_cfg: &[String], args: &[String]) -> io::Result<(i32, String, String)> {
    let mut cmd = std::process::Command::new("git");
    for p in git_cfg {
        cmd.arg(p);
    }
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.current_dir(cwd).output()?;
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    Ok((code, stdout, stderr))
}

fn quote_shell(s: &str) -> String {
    let simple = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.:/@%+".contains(c));
    if simple {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

fn render_command_for_log(cwd: &Path, git_cfg: &[String], args: &[String]) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push("git".to_string());
    for a in git_cfg {
        parts.push(quote_shell(a));
    }
    for a in args {
        parts.push(quote_shell(a));
    }
    format!(
        "(cd {} && {})",
        quote_shell(&cwd.display().to_string()),
        parts.join(" ")
    )
}

/// Collect every path referenced by the diff headers inside `diff --git` sections.
pub fn extract_paths_from_patch(diff_text: &str) -> Vec<String> {
    let mut set = std::collections::BTreeSet::new();
    for raw_line in diff_text.lines() {
        let line = raw_line.trim();
        let Some(rest) = line.strip_prefix("diff --git ") else {
            continue;
        };
        let Some((a, b)) = parse_diff_git_paths(rest) else {
            continue;
        };
        if let Some(a) = normalize_diff_path(&a, "a/") {
            set.insert(a);
        }
        if let Some(b) = normalize_diff_path(&b, "b/") {
            set.insert(b);
        }
    }
    set.into_iter().collect()
}

fn parse_diff_git_paths(line: &str) -> Option<(String, String)> {
    let mut chars = line.chars().peekable();
    let first = read_diff_git_token(&mut chars)?;
    let second = read_diff_git_token(&mut chars)?;
    Some((first, second))
}

fn read_diff_git_token<I>(chars: &mut std::iter::Peekable<I>) -> Option<String>
where
    I: Iterator<Item = char>,
{
    while let Some(ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }

    let ch = chars.next()?;
    let mut token = String::new();
    if ch == '"' {
        while let Some(next) = chars.next() {
            if next == '"' {
                break;
            }
            if next == '\\' {
                if let Some(escaped) = chars.next() {
                    token.push(escaped);
                }
            } else {
                token.push(next);
            }
        }
    } else {
        token.push(ch);
        while let Some(next) = chars.peek() {
            if next.is_whitespace() {
                break;
            }
            token.push(*next);
            chars.next();
        }
    }
    if token.is_empty() { None } else { Some(token) }
}

fn normalize_diff_path(path: &str, prefix: &str) -> Option<String> {
    let normalized = path.strip_prefix(prefix).unwrap_or(path);
    if normalized == "/dev/null" {
        None
    } else {
        Some(normalized.to_string())
    }
}

/// Stage all paths touched by the diff before applying a reverse patch.
pub fn stage_paths(git_root: &Path, diff_text: &str) -> io::Result<()> {
    let paths = extract_paths_from_patch(diff_text);
    if paths.is_empty() {
        return Ok(());
    }

    let mut cmd = std::process::Command::new("git");
    cmd.arg("add");
    cmd.arg("--");
    for path in &paths {
        cmd.arg(path);
    }
    let out = cmd.current_dir(git_root).output()?;
    let code = out.status.code().unwrap_or(-1);
    if code != 0 {
        return Err(io::Error::other(format!(
            "git add failed (exit {}): {}",
            code,
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(())
}

static RE_APPLIED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^Applied patch (?P<path>.+)$").expect("valid regex")
});
static RE_SKIPPED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^Skipped patch (?P<path>.+)\.$").expect("valid regex")
});
static RE_FAILED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^error: patch failed: (?P<path>.+):(?P<line>\d+)$").expect("valid regex")
});
static RE_CONFLICT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^CONFLICT \(content\): Merge conflict in (?P<path>.+)$")
        .expect("valid regex")
});
static RE_ALREADY_APPLIED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^error: patch failed: (?P<path>.+):(?P<line>\d+)\nerror: (?P<path2>.+): patch does not apply$")
        .expect("valid regex")
});

/// Parse `git apply` stdout/stderr and return lists of applied, skipped, and conflicted paths.
pub fn parse_git_apply_output(stdout: &str, stderr: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut applied = Vec::new();
    let mut skipped = Vec::new();
    let mut conflicted = Vec::new();

    for line in stdout.lines().chain(stderr.lines()) {
        if let Some(caps) = RE_APPLIED.captures(line) {
            applied.push(caps["path"].to_string());
        }
        if let Some(caps) = RE_SKIPPED.captures(line) {
            skipped.push(caps["path"].to_string());
        }
        if let Some(caps) = RE_FAILED.captures(line) {
            conflicted.push(caps["path"].to_string());
        }
        if let Some(caps) = RE_CONFLICT.captures(line) {
            conflicted.push(caps["path"].to_string());
        }
        if let Some(caps) = RE_ALREADY_APPLIED.captures(line) {
            skipped.push(caps["path"].to_string());
            skipped.push(caps["path2"].to_string());
        }
    }

    (applied, skipped, conflicted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn extracts_paths_from_patch() {
        let patch = r#"
diff --git a/foo.txt b/foo.txt
index 0000000..1111111 100644
--- a/foo.txt
+++ b/foo.txt
@@ -0,0 +1 @@
+hello
diff --git a/bar.txt b/bar.txt
index 0000000..1111111 100644
--- a/bar.txt
+++ b/bar.txt
@@ -0,0 +1 @@
+world
"#;
        let mut paths = extract_paths_from_patch(patch);
        paths.sort();
        assert_eq!(paths, vec!["bar.txt".to_string(), "foo.txt".to_string()]);
    }

    #[test]
    fn parses_apply_output() {
        let stdout = "Applied patch foo.txt\n";
        let stderr = "Skipped patch bar.txt.\n";
        let (applied, skipped, conflicted) = parse_git_apply_output(stdout, stderr);
        assert_eq!(applied, vec!["foo.txt".to_string()]);
        assert_eq!(skipped, vec!["bar.txt".to_string()]);
        assert!(conflicted.is_empty());
    }
}
