use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use anyhow::bail;
use clap::Parser;
use serde::Deserialize;
use sha2::Digest;
use sha2::Sha256;

const DEFAULT_REPOSITORY: &str = "cbusillo/code";
const DEFAULT_CHANNEL: &str = "stable";
const COMMAND_NAME_ENV: &str = "CODE_COMMAND_NAME";

#[derive(Debug, Parser)]
pub struct UpdateCheckCommand {
    /// GitHub repository that owns the update manifest.
    #[arg(long = "repo", value_name = "OWNER/REPO")]
    pub repo: Option<String>,

    /// Release tag to inspect. Defaults to the latest GitHub release.
    #[arg(long = "tag", value_name = "TAG")]
    pub tag: Option<String>,
}

#[derive(Debug, Parser)]
pub struct UpdateCommand {
    /// GitHub repository that owns the update manifest.
    #[arg(long = "repo", value_name = "OWNER/REPO")]
    pub repo: Option<String>,

    /// Release tag to install. Defaults to the latest GitHub release.
    #[arg(long = "tag", value_name = "TAG")]
    pub tag: Option<String>,

    /// Confirm replacement of the current directly managed binary.
    #[arg(long = "yes", short = 'y', default_value_t = false)]
    pub yes: bool,
}

#[derive(Debug, Deserialize)]
struct UpdateManifest {
    schema_version: u64,
    version: String,
    channel: String,
    commit: String,
    published_at: String,
    platforms: std::collections::BTreeMap<String, PlatformAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct PlatformAsset {
    asset: String,
    url: String,
    sha256: String,
    size: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VersionOrdering {
    Older,
    Same,
    Newer,
    Unknown,
}

pub async fn run_update_check(args: UpdateCheckCommand) -> anyhow::Result<()> {
    let report = fetch_update_report(args.repo.as_deref(), args.tag.as_deref()).await?;
    let identity = RuntimeIdentity::detect(None);
    print_update_report(&report, &identity);
    Ok(())
}

pub async fn run_update(args: UpdateCommand) -> anyhow::Result<()> {
    let report = fetch_update_report(args.repo.as_deref(), args.tag.as_deref()).await?;
    let exe = env::current_exe().context("failed to resolve current executable")?;
    let identity = RuntimeIdentity::detect(Some(&exe));
    print_update_report(&report, &identity);

    if report.ordering != VersionOrdering::Newer {
        println!("No update needed.");
        return Ok(());
    }

    let install_target = resolve_install_target(&exe);
    let install = detect_install_source_for_path(&install_target);
    println!("command:        {}", identity.command_name);
    println!("install target: {}", install_target.display());
    println!("install source: {}", install.description());
    if !install.can_self_update() {
        bail!(
            "refusing self-update for {} install; install the release manually instead",
            install.description()
        );
    }

    if !args.yes {
        bail!("pass --yes to replace the current binary after checksum verification");
    }

    #[cfg(target_family = "windows")]
    {
        let _ = install_target;
        bail!("self-update is not implemented for Windows yet");
    }

    #[cfg(target_family = "unix")]
    {
        install_direct_binary(&report.asset, &install_target).await?;
        if install_target == exe {
            println!("Updated {} to {}", install_target.display(), report.manifest.version);
        } else {
            println!(
                "Updated {} to {} (launched via {})",
                install_target.display(),
                report.manifest.version,
                exe.display()
            );
        }
        Ok(())
    }
}

struct UpdateReport {
    manifest: UpdateManifest,
    asset: PlatformAsset,
    current_version: String,
    current_target: String,
    ordering: VersionOrdering,
}

async fn fetch_update_report(repo: Option<&str>, tag: Option<&str>) -> anyhow::Result<UpdateReport> {
    let runtime_repo = env::var("GITHUB_REPOSITORY").ok();
    let repo = repo
        .or(runtime_repo.as_deref())
        .unwrap_or(DEFAULT_REPOSITORY);
    let tag = match tag {
        Some(tag) => normalize_tag(tag),
        None => latest_release_tag(repo).await?,
    };
    let url = format!(
        "https://github.com/{repo}/releases/download/{tag}/update-manifest.json"
    );
    let manifest: UpdateManifest = http_client("code-update-check/1")?
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to fetch {url}"))?
        .error_for_status()
        .with_context(|| format!("update manifest request failed: {url}"))?
        .json()
        .await
        .context("failed to parse update manifest")?;

    if manifest.schema_version != 1 {
        bail!("unsupported update manifest schema_version {}", manifest.schema_version);
    }
    if manifest.channel != DEFAULT_CHANNEL {
        bail!("unsupported update channel '{}'; expected stable", manifest.channel);
    }

    let target = current_target()?;
    let asset = manifest
        .platforms
        .get(&target)
        .cloned()
        .with_context(|| format!("manifest has no asset for current target {target}"))?;

    let current_version = code_version::version().to_string();
    let ordering = compare_versions(&current_version, &manifest.version);

    Ok(UpdateReport {
        manifest,
        asset,
        current_version,
        current_target: target,
        ordering,
    })
}

async fn latest_release_tag(repo: &str) -> anyhow::Result<String> {
    #[derive(Deserialize)]
    struct LatestRelease {
        tag_name: String,
    }

    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let release: LatestRelease = http_client("code-update-check/1")?
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to fetch {url}"))?
        .error_for_status()
        .with_context(|| format!("latest release request failed: {url}"))?
        .json()
        .await
        .context("failed to parse latest release response")?;

    Ok(release.tag_name)
}

fn print_update_report(report: &UpdateReport, identity: &RuntimeIdentity) {
    println!("product:         {}", code_version::LAB_BUILD_NAME);
    println!("repository:      {}", code_version::LAB_REPOSITORY);
    println!("command:         {}", identity.command_name);
    println!("current version: {}", report.current_version);
    println!("latest version:  {}", report.manifest.version);
    println!("channel:         {}", report.manifest.channel);
    println!("published at:    {}", report.manifest.published_at);
    println!("commit:          {}", report.manifest.commit);
    println!("target:          {}", report.current_target);
    println!("asset:           {}", report.asset.asset);
    println!("url:             {}", report.asset.url);
    println!("sha256:          {}", report.asset.sha256);
    println!("size:            {} bytes", report.asset.size);

    match report.ordering {
        VersionOrdering::Newer => println!("status:          update available"),
        VersionOrdering::Same => println!("status:          up to date"),
        VersionOrdering::Older => println!("status:          installed version is newer"),
        VersionOrdering::Unknown => println!("status:          unable to compare versions"),
    }
}

struct RuntimeIdentity {
    command_name: String,
}

impl RuntimeIdentity {
    fn detect(exe: Option<&Path>) -> Self {
        let command_name = env::var(COMMAND_NAME_ENV)
            .ok()
            .and_then(|name| valid_command_name(&name))
            .or_else(|| exe.and_then(command_name_from_path))
            .or_else(|| env::args_os().next().and_then(|arg| command_name_from_path(Path::new(&arg))))
            .unwrap_or_else(|| "code".to_string());

        Self { command_name }
    }
}

fn command_name_from_path(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(valid_command_name)
}

fn valid_command_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.contains(std::path::MAIN_SEPARATOR) {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn http_client(user_agent: &str) -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(user_agent)
        .timeout(Duration::from_secs(30))
        .build()?)
}

#[cfg(target_family = "unix")]
async fn install_direct_binary(asset: &PlatformAsset, exe: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let response = http_client("code-update/1")?
        .get(&asset.url)
        .send()
        .await
        .with_context(|| format!("failed to download {}", asset.url))?
        .error_for_status()
        .with_context(|| format!("asset download failed: {}", asset.url))?;
    let bytes = response.bytes().await.context("failed to read asset bytes")?;
    let actual = sha256_hex(&bytes);
    if !actual.eq_ignore_ascii_case(&asset.sha256) {
        bail!("asset checksum mismatch: expected {}, got {actual}", asset.sha256);
    }

    let parent = exe
        .parent()
        .context("current executable has no parent directory")?;
    let dir = tempfile::Builder::new()
        .prefix(".code-update-")
        .tempdir_in(parent)
        .context("failed to create update temp dir")?;
    let archive = dir.path().join(&asset.asset);
    fs::write(&archive, &bytes).context("failed to write downloaded asset")?;

    let extracted = dir.path().join("code-new");
    if asset.asset.ends_with(".tar.gz") {
        extract_tar_gz_binary(&asset.asset, &archive, &extracted)?;
    } else {
        bail!("self-update currently supports .tar.gz Unix assets only");
    }

    fs::set_permissions(&extracted, fs::Permissions::from_mode(0o755))
        .context("failed to mark staged binary executable")?;
    fs::rename(&extracted, exe).context("failed to install staged binary")
}

fn extract_tar_gz_binary(asset_name: &str, archive: &Path, extracted: &Path) -> anyhow::Result<()> {
    use flate2::read::GzDecoder;

    let expected_name = asset_name
        .strip_suffix(".tar.gz")
        .context("tarball asset name did not end with .tar.gz")?;
    let file = fs::File::open(archive).context("failed to open downloaded archive")?;
    let gz = GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    for entry in archive.entries().context("failed to read downloaded archive")? {
        let mut entry = entry.context("failed to read downloaded archive entry")?;
        let path = entry
            .path()
            .context("failed to read downloaded archive entry path")?
            .into_owned();
        let is_expected_binary = path.components().count() == 1
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == expected_name)
            && entry.header().entry_type().is_file();
        if is_expected_binary {
            entry
                .unpack(extracted)
                .context("failed to stage extracted binary")?;
            return Ok(());
        }
    }

    bail!("downloaded archive did not contain expected binary {expected_name}")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn normalize_tag(tag: &str) -> String {
    if tag.starts_with('v') {
        tag.to_string()
    } else {
        format!("v{tag}")
    }
}

fn current_target() -> anyhow::Result<String> {
    let os = env::consts::OS;
    let arch = env::consts::ARCH;
    match (os, arch) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-musl".to_string()),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-musl".to_string()),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin".to_string()),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin".to_string()),
        ("windows", _) => Ok("x86_64-pc-windows-msvc".to_string()),
        _ => bail!("unsupported platform: {os}/{arch}"),
    }
}

#[derive(Debug)]
enum InstallSource {
    Direct,
    Homebrew,
    Npm,
    Cargo,
    Unknown(PathBuf),
}

impl InstallSource {
    fn can_self_update(&self) -> bool {
        matches!(self, InstallSource::Direct)
    }

    fn description(&self) -> String {
        match self {
            InstallSource::Direct => "direct binary".to_string(),
            InstallSource::Homebrew => "Homebrew".to_string(),
            InstallSource::Npm => "npm/pnpm/bun".to_string(),
            InstallSource::Cargo => "cargo".to_string(),
            InstallSource::Unknown(path) => format!("unknown ({})", path.display()),
        }
    }
}

fn resolve_install_target(exe: &Path) -> PathBuf {
    fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf())
}

fn detect_install_source_for_path(exe: &Path) -> InstallSource {
    let path = exe.to_string_lossy();
    if path.contains("/Cellar/") || path.contains("/Homebrew/") || path.contains("/homebrew/") {
        return InstallSource::Homebrew;
    }
    if path.contains("/node_modules/") || path.contains("/.bun/") {
        return InstallSource::Npm;
    }
    if path.contains("/.cargo/bin/") {
        return InstallSource::Cargo;
    }
    if path.contains("/.code/bin/")
        || path.contains("/.local/bin/")
        || path.contains("/usr/local/bin/")
        || path.contains("/code-rs/target/release/")
    {
        return InstallSource::Direct;
    }

    InstallSource::Unknown(exe.to_path_buf())
}

fn compare_versions(current: &str, latest: &str) -> VersionOrdering {
    let Some(current) = parse_version_triplet(current) else {
        return VersionOrdering::Unknown;
    };
    let Some(latest) = parse_version_triplet(latest) else {
        return VersionOrdering::Unknown;
    };
    match current.cmp(&latest) {
        std::cmp::Ordering::Less => VersionOrdering::Newer,
        std::cmp::Ordering::Equal => VersionOrdering::Same,
        std::cmp::Ordering::Greater => VersionOrdering::Older,
    }
}

fn parse_version_triplet(version: &str) -> Option<(u64, u64, u64)> {
    let trimmed = version.trim().trim_start_matches('v');
    let core = trimmed
        .split_once(['-', '+'])
        .map_or(trimmed, |(version, _)| version);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn normalize_tag_adds_v_prefix_when_missing() {
        assert_eq!(normalize_tag("0.6.101"), "v0.6.101");
        assert_eq!(normalize_tag("v0.6.101"), "v0.6.101");
    }

    #[test]
    fn compare_versions_detects_update_states() {
        assert_eq!(compare_versions("0.6.100", "v0.6.101"), VersionOrdering::Newer);
        assert_eq!(compare_versions("v0.6.101", "0.6.101"), VersionOrdering::Same);
        assert_eq!(compare_versions("0.6.102", "v0.6.101"), VersionOrdering::Older);
        assert_eq!(compare_versions("0.6", "v0.6.101"), VersionOrdering::Unknown);
    }

    #[test]
    fn sha256_hex_matches_known_value() {
        assert_eq!(
            sha256_hex(b"code"),
            "5694d08a2e53ffcae0c3103e5ad6f6076abd960eb1f8a56577040bc1028f702b"
        );
    }

    #[test]
    fn extract_tar_gz_binary_requires_exact_top_level_binary() -> anyhow::Result<()> {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Cursor;

        let dir = tempfile::tempdir()?;
        let archive_path = dir.path().join("code-aarch64-apple-darwin.tar.gz");
        let archive_file = fs::File::create(&archive_path)?;
        let encoder = GzEncoder::new(archive_file, Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o755);
        header.set_cksum();
        archive.append_data(
            &mut header,
            "code-aarch64-apple-darwin",
            Cursor::new(b"new"),
        )?;
        let encoder = archive.into_inner()?;
        encoder.finish()?;

        let extracted = dir.path().join("code-new");
        extract_tar_gz_binary(
            "code-aarch64-apple-darwin.tar.gz",
            &archive_path,
            &extracted,
        )?;

        assert_eq!(fs::read(&extracted)?, b"new");
        Ok(())
    }

    #[test]
    fn extract_tar_gz_binary_rejects_nested_binary() -> anyhow::Result<()> {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Cursor;

        let dir = tempfile::tempdir()?;
        let archive_path = dir.path().join("code-aarch64-apple-darwin.tar.gz");
        let archive_file = fs::File::create(&archive_path)?;
        let encoder = GzEncoder::new(archive_file, Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o755);
        header.set_cksum();
        archive.append_data(
            &mut header,
            "nested/code-aarch64-apple-darwin",
            Cursor::new(b"new"),
        )?;
        let encoder = archive.into_inner()?;
        encoder.finish()?;

        let extracted = dir.path().join("code-new");
        let err = extract_tar_gz_binary(
            "code-aarch64-apple-darwin.tar.gz",
            &archive_path,
            &extracted,
        )
        .expect_err("nested payload should be rejected");

        assert!(err.to_string().contains("expected binary"));
        assert!(!extracted.exists());
        Ok(())
    }

    #[test]
    fn detect_install_source_classifies_managed_paths() {
        assert!(matches!(
            detect_install_source_for_path(Path::new(
                "/opt/homebrew/Cellar/code/0.6.101/bin/code"
            )),
            InstallSource::Homebrew
        ));
        assert!(matches!(
            detect_install_source_for_path(Path::new(
                "/Users/me/.npm-global/lib/node_modules/@just-every/code/bin/code"
            )),
            InstallSource::Npm
        ));
        assert!(matches!(
            detect_install_source_for_path(Path::new("/Users/me/.cargo/bin/code")),
            InstallSource::Cargo
        ));
        assert!(matches!(
            detect_install_source_for_path(Path::new("/Users/me/.local/bin/code")),
            InstallSource::Direct
        ));
        assert!(matches!(
            detect_install_source_for_path(Path::new("/usr/local/bin/chris-code")),
            InstallSource::Direct
        ));
    }

    #[test]
    fn runtime_identity_prefers_command_name_env() {
        let _lock = ENV_TEST_LOCK.lock().unwrap();
        let _reset = EnvReset::capture(COMMAND_NAME_ENV);
        unsafe {
            env::set_var(COMMAND_NAME_ENV, "chris-code");
        }

        let identity = RuntimeIdentity::detect(Some(Path::new("/usr/local/bin/code")));

        assert_eq!(identity.command_name, "chris-code");
    }

    #[test]
    fn runtime_identity_falls_back_to_exe_name() {
        let _lock = ENV_TEST_LOCK.lock().unwrap();
        let _reset = EnvReset::capture(COMMAND_NAME_ENV);
        unsafe {
            env::remove_var(COMMAND_NAME_ENV);
        }

        let identity = RuntimeIdentity::detect(Some(Path::new("/usr/local/bin/chris-code")));

        assert_eq!(identity.command_name, "chris-code");
    }

    struct EnvReset {
        key: &'static str,
        value: Option<String>,
    }

    impl EnvReset {
        fn capture(key: &'static str) -> Self {
            Self {
                key,
                value: env::var(key).ok(),
            }
        }
    }

    impl Drop for EnvReset {
        fn drop(&mut self) {
            unsafe {
                if let Some(value) = &self.value {
                    env::set_var(self.key, value);
                } else {
                    env::remove_var(self.key);
                }
            }
        }
    }

    #[test]
    fn detect_install_source_refuses_build_cache_binaries() {
        assert!(matches!(
            detect_install_source_for_path(Path::new(
                "/Users/me/Developer/code/.code/working/_target-cache/code/main/code-rs/dev-fast/code"
            )),
            InstallSource::Unknown(_)
        ));
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn resolve_install_target_follows_symlinks() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("code-target");
        let link = dir.path().join("code-link");
        fs::write(&target, b"code")?;
        std::os::unix::fs::symlink(&target, &link)?;

        assert_eq!(resolve_install_target(&link), fs::canonicalize(target)?);
        Ok(())
    }
}
