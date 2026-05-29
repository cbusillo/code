//! Utility to compute the current Git diff for the working directory.
//!
//! The implementation mirrors the behaviour of the TypeScript version in
//! `codex-cli`: it returns the diff for tracked changes as well as any
//! untracked files. When the current directory is not inside a Git
//! repository, the function returns `Ok((false, String::new()))`.

use std::env;
use std::io;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

const DISABLE_FSMONITOR_CONFIG: &str = "core.fsmonitor=false";
const DISABLE_HOOKS_CONFIG: &str = if cfg!(windows) {
    "core.hooksPath=NUL"
} else {
    "core.hooksPath=/dev/null"
};
const EXECUTABLE_FILTER_CONFIG_PATTERN: &str = r"^filter\..*\.(clean|process)$";
const SAFE_DIFF_ARGS: &[&str] = &[
    "diff",
    "--no-textconv",
    "--no-ext-diff",
    "--submodule=short",
    "--color",
];

/// Return value of [`get_git_diff`].
///
/// * `bool` – Whether the current working directory is inside a Git repo.
/// * `String` – The concatenated diff (may be empty).
pub(crate) async fn get_git_diff() -> io::Result<(bool, String)> {
    get_git_diff_in_dir(env::current_dir()?).await
}

async fn get_git_diff_in_dir(cwd: impl AsRef<Path>) -> io::Result<(bool, String)> {
    let cwd = cwd.as_ref();

    // First check if we are inside a Git repository.
    if !inside_git_repo(cwd).await? {
        return Ok((false, String::new()));
    }

    // Keep `/diff` informational: repository configuration must not select
    // executable diff helpers.
    let diff_config_overrides = diff_filter_config_overrides(cwd).await?;
    let diff_config_overrides = diff_filter_config_overrides_with_submodules(
        cwd,
        diff_config_overrides,
    )
    .await?;

    // Run tracked diff and untracked file listing in parallel.
    let (tracked_diff_res, untracked_output_res) = tokio::join!(
        run_git_capture_diff_with_config(cwd, &diff_config_overrides, SAFE_DIFF_ARGS),
        run_git_capture_stdout(cwd, &["ls-files", "--others", "--exclude-standard"]),
    );
    let tracked_diff = tracked_diff_res?;
    let untracked_output = untracked_output_res?;

    let mut untracked_diff = String::new();
    let null_device: &Path = if cfg!(windows) {
        Path::new("NUL")
    } else {
        Path::new("/dev/null")
    };

    let null_path = null_device.to_str().unwrap_or("/dev/null").to_string();
    let mut join_set: tokio::task::JoinSet<io::Result<String>> = tokio::task::JoinSet::new();
    for file in untracked_output
        .split('\n')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let null_path = null_path.clone();
        let file = file.to_string();
        let diff_config_overrides = diff_config_overrides.clone();
        let cwd = cwd.to_path_buf();
        join_set.spawn(async move {
            let mut args = SAFE_DIFF_ARGS.to_vec();
            args.extend(["--no-index", "--", &null_path, &file]);
            run_git_capture_diff_with_config(&cwd, &diff_config_overrides, &args).await
        });
    }
    while let Some(res) = join_set.join_next().await {
        match res {
            Ok(Ok(diff)) => untracked_diff.push_str(&diff),
            Ok(Err(err)) if err.kind() == io::ErrorKind::NotFound => {}
            Ok(Err(err)) => return Err(err),
            Err(_) => {}
        }
    }

    Ok((true, format!("{tracked_diff}{untracked_diff}")))
}

/// Helper that executes `git` with the given `args` and returns `stdout` as a
/// UTF-8 string. Any non-zero exit status is considered an *error*.
async fn run_git_capture_stdout(cwd: &Path, args: &[&str]) -> io::Result<String> {
    let output = run_git_command(cwd, &[], args).await?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(io::Error::other(format!(
            "git {:?} failed with status {}",
            args, output.status
        )))
    }
}

/// Like [`run_git_capture_stdout`] but treats exit status 1 as success and
/// returns stdout. Git returns 1 for diffs when differences are present.
async fn run_git_capture_diff_with_config(
    cwd: &Path,
    config_overrides: &[(String, String)],
    args: &[&str],
) -> io::Result<String> {
    let output = run_git_command(cwd, config_overrides, args).await?;

    if output.status.success() || output.status.code() == Some(1) {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(io::Error::other(format!(
            "git {:?} failed with status {}",
            args, output.status
        )))
    }
}

/// Return Git configuration overrides that prevent configured filter drivers
/// from executing while generating diffs.
async fn diff_filter_config_overrides(cwd: &Path) -> io::Result<Vec<(String, String)>> {
    let args = [
        "config",
        "--null",
        "--name-only",
        "--get-regexp",
        EXECUTABLE_FILTER_CONFIG_PATTERN,
    ];
    let output = run_git_command(cwd, &[], &args).await?;
    if !(output.status.success() || output.status.code() == Some(1)) {
        return Err(io::Error::other(format!(
            "git {:?} failed with status {}",
            args, output.status
        )));
    }

    let mut drivers = String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter_map(|key| {
            key.strip_suffix(".clean")
                .or_else(|| key.strip_suffix(".process"))
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    drivers.sort();
    drivers.dedup();

    Ok(drivers
        .into_iter()
        .flat_map(|driver| {
            [
                (format!("{driver}.clean"), String::new()),
                (format!("{driver}.process"), String::new()),
                (format!("{driver}.required"), "false".to_string()),
            ]
        })
        .collect())
}

async fn diff_filter_config_overrides_with_submodules(
    cwd: &Path,
    mut overrides: Vec<(String, String)>,
) -> io::Result<Vec<(String, String)>> {
    let output = run_git_command(
        cwd,
        &[],
        &["submodule", "foreach", "--quiet", "--recursive", "pwd"],
    )
    .await?;
    if !(output.status.success() || output.status.code() == Some(1)) {
        return Err(io::Error::other(format!(
            "git submodule foreach failed with status {}",
            output.status
        )));
    }

    for path in String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        overrides.extend(diff_filter_config_overrides(Path::new(path)).await?);
    }
    overrides.sort();
    overrides.dedup();
    Ok(overrides)
}

async fn run_git_command(
    cwd: &Path,
    config_overrides: &[(String, String)],
    args: &[&str],
) -> io::Result<std::process::Output> {
    let mut command = Command::new("git");
    command
        .args([
            "-c",
            DISABLE_FSMONITOR_CONFIG,
            "-c",
            DISABLE_HOOKS_CONFIG,
        ])
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    if !config_overrides.is_empty() {
        command.env("GIT_CONFIG_COUNT", config_overrides.len().to_string());
        for (index, (key, value)) in config_overrides.iter().enumerate() {
            command
                .env(format!("GIT_CONFIG_KEY_{index}"), key)
                .env(format!("GIT_CONFIG_VALUE_{index}"), value);
        }
    }

    command.output().await
}

/// Determine if the current directory is inside a Git repository.
async fn inside_git_repo(cwd: &Path) -> io::Result<bool> {
    let status = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(cwd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    match status {
        Ok(s) if s.success() => Ok(true),
        Ok(_) => Ok(false),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false), // git not installed
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::process::Command as ProcessCommand;

    #[tokio::test]
    async fn diff_filter_config_overrides_disable_clean_and_process_filters() {
        let tempdir = tempfile::tempdir().expect("create temp directory");
        let repo = tempdir.path();
        run_git_setup(repo, &["init", "-q"]);
        run_git_setup(repo, &["config", "filter.evil.clean", "helper"]);
        run_git_setup(repo, &["config", "filter.evil.process", "helper"]);
        run_git_setup(repo, &["config", "filter.evil.required", "true"]);

        let overrides = diff_filter_config_overrides(repo)
            .await
            .expect("read filter overrides");

        assert!(
            overrides.contains(&("filter.evil.clean".to_string(), String::new())),
            "missing clean override: {overrides:?}"
        );
        assert!(
            overrides.contains(&("filter.evil.process".to_string(), String::new())),
            "missing process override: {overrides:?}"
        );
        assert!(
            overrides.contains(&(
                "filter.evil.required".to_string(),
                "false".to_string()
            )),
            "missing required override: {overrides:?}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn get_git_diff_does_not_execute_configured_filters_fsmonitor_or_hooks() {
        let tempdir = tempfile::tempdir().expect("create temp directory");
        let repo = tempdir.path().join("repo");
        fs::create_dir(&repo).expect("create test repository directory");
        run_git_setup(&repo, &["init", "-q"]);
        run_git_setup(&repo, &["config", "user.name", "test"]);
        run_git_setup(&repo, &["config", "user.email", "test@example.com"]);
        fs::write(repo.join(".gitattributes"), "*.txt filter=x=y\n")
            .expect("write attributes");
        fs::write(repo.join("tracked.txt"), "before\n").expect("write tracked file");
        fs::write(repo.join("unchanged.txt"), "unchanged\n").expect("write unchanged file");
        run_git_setup(&repo, &["add", ".gitattributes", "tracked.txt", "unchanged.txt"]);
        run_git_setup(&repo, &["commit", "-qm", "initial"]);

        let filter_helper = tempdir.path().join("filter-helper.sh");
        let fsmonitor_helper = tempdir.path().join("fsmonitor-helper.sh");
        let hooks_dir = tempdir.path().join("hooks");
        let hook_helper = hooks_dir.join("post-index-change");
        fs::create_dir(&hooks_dir).expect("create hooks directory");
        write_marker_helper(&filter_helper);
        write_marker_helper(&fsmonitor_helper);
        write_marker_helper(&hook_helper);
        run_git_setup(
            &repo,
            &[
                "config",
                "filter.x=y.clean",
                filter_helper.to_str().expect("filter helper path"),
            ],
        );
        run_git_setup(
            &repo,
            &[
                "config",
                "filter.x=y.process",
                filter_helper.to_str().expect("filter helper path"),
            ],
        );
        run_git_setup(&repo, &["config", "filter.x=y.required", "true"]);
        run_git_setup(
            &repo,
            &[
                "config",
                "core.fsmonitor",
                fsmonitor_helper.to_str().expect("fsmonitor helper path"),
            ],
        );
        run_git_setup(
            &repo,
            &[
                "config",
                "core.hooksPath",
                hooks_dir.to_str().expect("hooks directory path"),
            ],
        );
        std::thread::sleep(std::time::Duration::from_secs(1));
        fs::write(repo.join("unchanged.txt"), "unchanged\n").expect("refresh unchanged file");
        fs::write(repo.join("tracked.txt"), "after\n").expect("modify tracked file");

        let result = get_git_diff_in_dir(&repo)
            .await
            .expect("generate diff without invoking helpers");

        assert!(result.1.contains("before"));
        assert!(result.1.contains("after"));
        assert!(!filter_helper.with_extension("sh.ran").exists());
        assert!(!fsmonitor_helper.with_extension("sh.ran").exists());
        assert!(!hook_helper.with_extension("sh.ran").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn get_git_diff_does_not_execute_helpers_while_checking_dirty_submodules() {
        let tempdir = tempfile::tempdir().expect("create temp directory");
        let grandchild = tempdir.path().join("grandchild");
        let child = tempdir.path().join("child");
        let repo = tempdir.path().join("repo");
        fs::create_dir(&grandchild).expect("create grandchild repository directory");
        fs::create_dir(&child).expect("create child repository directory");
        fs::create_dir(&repo).expect("create parent repository directory");
        run_git_setup(&grandchild, &["init", "-q"]);
        run_git_setup(&grandchild, &["config", "user.name", "test"]);
        run_git_setup(&grandchild, &["config", "user.email", "test@example.com"]);
        fs::write(grandchild.join(".gitattributes"), "*.txt filter=nested\n")
            .expect("write grandchild attributes");
        fs::write(grandchild.join("nested.txt"), "before\n")
            .expect("write grandchild tracked file");
        run_git_setup(&grandchild, &["add", ".gitattributes", "nested.txt"]);
        run_git_setup(&grandchild, &["commit", "-qm", "initial"]);

        run_git_setup(&child, &["init", "-q"]);
        run_git_setup(&child, &["config", "user.name", "test"]);
        run_git_setup(&child, &["config", "user.email", "test@example.com"]);
        fs::write(child.join(".gitattributes"), "*.txt filter=evil\n")
            .expect("write child attributes");
        fs::write(child.join("tracked.txt"), "before\n").expect("write child tracked file");
        run_git_setup(&child, &["add", ".gitattributes", "tracked.txt"]);
        run_git_setup(&child, &["commit", "-qm", "initial"]);
        run_git_setup(
            &child,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                grandchild.to_str().expect("grandchild repository path"),
                "grandchild",
            ],
        );
        run_git_setup(&child, &["commit", "-qm", "add nested submodule"]);

        run_git_setup(&repo, &["init", "-q"]);
        run_git_setup(&repo, &["config", "user.name", "test"]);
        run_git_setup(&repo, &["config", "user.email", "test@example.com"]);
        run_git_setup(
            &repo,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                child.to_str().expect("child repository path"),
                "child",
            ],
        );
        run_git_setup(&repo, &["commit", "-qm", "add submodule"]);

        let helper = tempdir.path().join("submodule-helper.sh");
        let nested_helper = tempdir.path().join("nested-submodule-helper.sh");
        write_marker_helper(&helper);
        write_marker_helper(&nested_helper);
        let checkout = repo.join("child");
        run_git_setup(
            &checkout,
            &[
                "config",
                "filter.evil.clean",
                helper.to_str().expect("submodule helper path"),
            ],
        );
        run_git_setup(&checkout, &["config", "filter.evil.required", "true"]);
        let nested_checkout = checkout.join("grandchild");
        run_git_setup(
            &nested_checkout,
            &[
                "config",
                "filter.nested.clean",
                nested_helper.to_str().expect("nested submodule helper path"),
            ],
        );
        run_git_setup(
            &nested_checkout,
            &["config", "filter.nested.required", "true"],
        );
        std::thread::sleep(std::time::Duration::from_secs(1));
        fs::write(checkout.join("tracked.txt"), "after\n").expect("modify child tracked file");
        fs::write(nested_checkout.join("nested.txt"), "after\n")
            .expect("modify grandchild tracked file");

        let result = get_git_diff_in_dir(&repo)
            .await
            .expect("generate diff without executing submodule helpers");

        assert!(
            result.1.contains("-dirty"),
            "dirty submodule marker should be preserved: {}",
            result.1
        );
        assert!(!helper.with_extension("sh.ran").exists());
        assert!(!nested_helper.with_extension("sh.ran").exists());
    }

    fn run_git_setup(cwd: &Path, args: &[&str]) {
        let status = ProcessCommand::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .expect("run git setup command");
        assert!(status.success(), "git setup command failed: {args:?}");
    }

    #[cfg(unix)]
    fn write_marker_helper(path: &Path) {
        fs::write(path, "#!/bin/sh\nprintf ran >> \"$0.ran\"\nexit 1\n")
            .expect("write helper script");
        let mut permissions = fs::metadata(path)
            .expect("read helper metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("make helper executable");
    }
}
