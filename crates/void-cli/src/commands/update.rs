//! Self-update: download and install the latest GitHub release binary.

use std::path::Path;
use std::time::Duration;

use clap::Args;
use sha2::{Digest, Sha256};

use crate::commands::setup::prompt::confirm_default_yes;

const GITHUB_API_BASE: &str = "https://api.github.com";
const CHECKSUMS_ASSET_NAME: &str = "checksums.txt";

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Only check whether a newer version is available; do not install it
    #[arg(long)]
    pub check: bool,
    /// Install without prompting for confirmation
    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[derive(Debug, serde::Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, serde::Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

/// `owner/repo`, parsed from this crate's `repository` URL so it can't drift
/// from the actual GitHub project.
fn repo_slug() -> &'static str {
    env!("CARGO_PKG_REPOSITORY")
        .trim_end_matches('/')
        .rsplit("github.com/")
        .next()
        .unwrap_or("MaximeGaudin/void")
}

fn http_client(timeout: Duration) -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("void-cli/", env!("CARGO_PKG_VERSION")))
        .timeout(timeout)
        .build()?)
}

async fn fetch_latest_release(timeout: Duration) -> anyhow::Result<GithubRelease> {
    let url = format!("{GITHUB_API_BASE}/repos/{}/releases/latest", repo_slug());
    let release = http_client(timeout)?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json::<GithubRelease>()
        .await?;
    Ok(release)
}

/// Best-effort "a newer version exists" check for `void doctor`. Never blocks
/// long or fails the caller — a network hiccup just means no note is printed.
pub async fn check_for_update() -> Option<String> {
    check_newer_than(env!("CARGO_PKG_VERSION")).await
}

/// Same check against an arbitrary "current" version, for checking a remote
/// host's `void` (over cached `void remote status` output) rather than this
/// local build.
pub async fn check_newer_than(current: &str) -> Option<String> {
    let release = fetch_latest_release(Duration::from_secs(2)).await.ok()?;
    let latest = release.tag_name.trim_start_matches('v').to_string();
    is_newer(&latest, current).then_some(latest)
}

/// Parses a `major.minor.patch` version, ignoring any trailing pre-release or
/// build metadata. Unparseable input can't be compared, so callers treat it
/// as "not newer" rather than nagging on garbled version strings.
fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// Whether `candidate` is a strictly newer release than `current`. A locally
/// built binary that is *ahead* of the latest published release (e.g. built
/// from `main` before tagging) must not be reported as outdated, so this is
/// a real version compare, not a string inequality.
fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_semver(candidate), parse_semver(current)) {
        (Some(c), Some(cur)) => c > cur,
        _ => false,
    }
}

fn platform_asset_name() -> Option<&'static str> {
    asset_name_for(std::env::consts::OS, std::env::consts::ARCH)
}

/// Maps a (OS, arch) pair to the matching artifact name from `release.yml`'s
/// build matrix. Kept as a pure function so every mapping is unit-testable
/// regardless of which platform runs the test.
fn asset_name_for(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some("void-darwin-arm64.tar.gz"),
        ("macos", "x86_64") => Some("void-darwin-amd64.tar.gz"),
        ("linux", "x86_64") => Some("void-linux-amd64.tar.gz"),
        ("linux", "aarch64") => Some("void-linux-arm64.tar.gz"),
        ("windows", "x86_64") => Some("void-windows-amd64.zip"),
        _ => None,
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Find `asset_name`'s expected hash in a `sha256sum`-style checksums file
/// (`<hash>  <filename>` per line, GNU or BSD `*`-binary-marker form).
fn find_checksum(checksums: &str, asset_name: &str) -> Option<String> {
    checksums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        (name == asset_name).then(|| hash.to_lowercase())
    })
}

fn extract_archive(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    let status = std::process::Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .status()
        .map_err(|e| {
            anyhow::anyhow!("failed to run `tar` to extract {}: {e}", archive.display())
        })?;
    if !status.success() {
        anyhow::bail!("`tar` failed to extract {}", archive.display());
    }
    Ok(())
}

/// Atomically replace the running executable with `new_bin`.
///
/// Unix allows renaming over a running process's file (it keeps the old
/// inode alive for the process using it); Windows does not allow overwriting
/// an open executable's *contents*, but does allow renaming it out of the
/// way first, which is the standard self-update trick on that platform.
fn install_binary(new_bin: &Path) -> anyhow::Result<()> {
    let current_exe = std::env::current_exe()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = current_exe
            .parent()
            .ok_or_else(|| anyhow::anyhow!("cannot resolve install directory"))?;
        let tmp = dir.join(format!(".void.tmp.{}", std::process::id()));
        std::fs::copy(new_bin, &tmp)?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        if cfg!(target_os = "macos") {
            let _ = std::process::Command::new("xattr")
                .arg("-cr")
                .arg(&tmp)
                .status();
        }
        std::fs::rename(&tmp, &current_exe)?;
    }

    #[cfg(windows)]
    {
        let old = current_exe.with_extension("exe.old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&current_exe, &old)?;
        std::fs::copy(new_bin, &current_exe)?;
        let _ = std::fs::remove_file(&old);
    }

    Ok(())
}

pub async fn run(args: &UpdateArgs) -> anyhow::Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    eprintln!("Current version: {current}");

    let release = fetch_latest_release(Duration::from_secs(10)).await?;
    let latest = release.tag_name.trim_start_matches('v').to_string();

    if latest == current {
        eprintln!("Already up to date.");
        return Ok(());
    }
    if !is_newer(&latest, current) {
        eprintln!(
            "Running {current}, which is newer than the latest published release ({latest}). Nothing to do."
        );
        return Ok(());
    }

    eprintln!("New version available: {latest}");
    if args.check {
        eprintln!("Run `void update` to install it.");
        return Ok(());
    }

    if !args.yes && !confirm_default_yes(&format!("Update void {current} -> {latest}?")) {
        eprintln!("Update cancelled.");
        return Ok(());
    }

    let asset_name = platform_asset_name().ok_or_else(|| {
        anyhow::anyhow!(
            "no prebuilt binary for this platform ({}-{})",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| anyhow::anyhow!("release {latest} has no asset named {asset_name}"))?;

    eprintln!("Downloading {asset_name}...");
    let client = http_client(Duration::from_secs(120))?;
    let archive_bytes = client
        .get(&asset.browser_download_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    match release
        .assets
        .iter()
        .find(|a| a.name == CHECKSUMS_ASSET_NAME)
    {
        Some(checksums_asset) => {
            let checksums = client
                .get(&checksums_asset.browser_download_url)
                .send()
                .await?
                .error_for_status()?
                .text()
                .await?;
            let expected = find_checksum(&checksums, asset_name).ok_or_else(|| {
                anyhow::anyhow!("{CHECKSUMS_ASSET_NAME} has no entry for {asset_name}")
            })?;
            let mut hasher = Sha256::new();
            hasher.update(&archive_bytes);
            let actual = hex_encode(&hasher.finalize());
            if actual != expected {
                anyhow::bail!(
                    "checksum mismatch for {asset_name}: expected {expected}, got {actual}"
                );
            }
            eprintln!("Checksum verified.");
        }
        None => {
            eprintln!(
                "[warn] release has no {CHECKSUMS_ASSET_NAME} — installing without verification"
            );
        }
    }

    let work_dir = std::env::temp_dir().join(format!("void-update-{}", std::process::id()));
    std::fs::create_dir_all(&work_dir)?;
    let archive_path = work_dir.join(asset_name);
    std::fs::write(&archive_path, &archive_bytes)?;
    extract_archive(&archive_path, &work_dir)?;

    let bin_name = if cfg!(windows) { "void.exe" } else { "void" };
    let extracted_bin = work_dir.join(bin_name);
    if !extracted_bin.exists() {
        anyhow::bail!("extracted archive did not contain {bin_name}");
    }

    let daemon_was_running = crate::context::store_path().join("LOCK").exists();
    if daemon_was_running {
        crate::commands::sync::stop_daemon()?;
    }

    install_binary(&extracted_bin)?;
    std::fs::remove_dir_all(&work_dir).ok();

    eprintln!("Updated to void {latest}.");

    if daemon_was_running {
        eprintln!("Restarting sync daemon...");
        let sync_args = crate::commands::sync::SyncArgs {
            connectors: None,
            daemon: false,
            restart: false,
            clear: false,
            clear_connector: None,
            stop: false,
            status: false,
            allow_broken: false,
            daemon_inner: false,
        };
        crate::commands::sync::daemonize(&sync_args, false)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_slug_parses_github_url() {
        assert_eq!(repo_slug(), "MaximeGaudin/void");
    }

    #[test]
    fn is_newer_only_true_for_a_strictly_greater_version() {
        assert!(is_newer("0.13.0", "0.12.1"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.12.1", "0.13.0"));
        assert!(!is_newer("0.13.0", "0.13.0"));
    }

    #[test]
    fn is_newer_ignores_pre_release_and_build_metadata() {
        assert!(is_newer("0.13.0", "0.12.1-rc1"));
        assert!(!is_newer("0.13.0+abc1234", "0.13.0"));
    }

    #[test]
    fn is_newer_treats_unparseable_versions_as_not_newer() {
        assert!(!is_newer("not-a-version", "0.12.1"));
        assert!(!is_newer("0.13.0", "not-a-version"));
    }

    #[test]
    fn asset_name_matches_release_yml_matrix() {
        assert_eq!(
            asset_name_for("macos", "aarch64"),
            Some("void-darwin-arm64.tar.gz")
        );
        assert_eq!(
            asset_name_for("macos", "x86_64"),
            Some("void-darwin-amd64.tar.gz")
        );
        assert_eq!(
            asset_name_for("linux", "x86_64"),
            Some("void-linux-amd64.tar.gz")
        );
        assert_eq!(
            asset_name_for("linux", "aarch64"),
            Some("void-linux-arm64.tar.gz")
        );
        assert_eq!(
            asset_name_for("windows", "x86_64"),
            Some("void-windows-amd64.zip")
        );
        assert_eq!(asset_name_for("freebsd", "x86_64"), None);
    }

    #[test]
    fn find_checksum_matches_gnu_and_bsd_formats() {
        let checksums = "abc123  void-linux-amd64.tar.gz\ndef456 *void-darwin-arm64.tar.gz\n";
        assert_eq!(
            find_checksum(checksums, "void-linux-amd64.tar.gz").as_deref(),
            Some("abc123")
        );
        assert_eq!(
            find_checksum(checksums, "void-darwin-arm64.tar.gz").as_deref(),
            Some("def456")
        );
        assert_eq!(find_checksum(checksums, "void-windows-amd64.zip"), None);
    }

    #[test]
    fn hex_encode_matches_known_sha256() {
        let mut hasher = Sha256::new();
        hasher.update(b"");
        assert_eq!(
            hex_encode(&hasher.finalize()),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
