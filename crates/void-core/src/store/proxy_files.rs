use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::config::expand_tilde;
use crate::error::ConfigError;

use super::remote::{fetch_remote_file, SshTarget};

/// Upload local paths referenced by `--file` before SSH proxy; pull `--out` / `--output`
/// downloads back to the local machine after a successful proxied command.
#[derive(Debug, Clone)]
pub struct ProxyFileTransferPlan {
    pub args: Vec<String>,
    pub uploads: Vec<StagedUpload>,
    pub download: Option<StagedDownload>,
}

#[derive(Debug, Clone)]
pub struct StagedUpload {
    pub local_path: PathBuf,
    pub remote_path: String,
}

#[derive(Debug, Clone)]
pub struct StagedDownload {
    pub remote_path: String,
    pub local_out: PathBuf,
}

pub fn plan_proxy_file_transfer(
    remote_store_path: &str,
    args: &[String],
) -> Result<ProxyFileTransferPlan, ConfigError> {
    let staging_dir = format!("{remote_store_path}/staging");
    let mut rewritten = args.to_vec();
    let mut uploads = Vec::new();
    let mut download = None;

    let mut i = 0;
    while i < rewritten.len() {
        match rewritten[i].as_str() {
            "--file" => {
                if let Some(path) = take_flag_value(&rewritten, i) {
                    let local_path = expand_tilde(path);
                    if !local_path.is_file() {
                        return Err(ConfigError::Remote(format!(
                            "local file not found for --file: {}",
                            local_path.display()
                        )));
                    }
                    let remote_path = staging_path(&staging_dir, &local_path);
                    rewritten[i + 1] = remote_path.clone();
                    uploads.push(StagedUpload {
                        local_path,
                        remote_path,
                    });
                    i += 2;
                    continue;
                }
            }
            "--out" if is_download_proxy_command(args) => {
                if let Some(path) = take_flag_value(&rewritten, i) {
                    let local_out = expand_tilde(path);
                    let remote_path = staging_path(&staging_dir, &local_out);
                    download = Some(StagedDownload {
                        remote_path: remote_path.clone(),
                        local_out,
                    });
                    rewritten[i + 1] = remote_path;
                    i += 2;
                    continue;
                }
            }
            "--output"
                if is_drive_download_proxy_command(args)
                    && !args.contains(&"--stdout".to_string()) =>
            {
                if let Some(path) = take_flag_value(&rewritten, i) {
                    let local_out = expand_tilde(path);
                    let remote_path = staging_path(&staging_dir, &local_out);
                    download = Some(StagedDownload {
                        remote_path: remote_path.clone(),
                        local_out,
                    });
                    rewritten[i + 1] = remote_path;
                    i += 2;
                    continue;
                }
            }
            "-o" if is_drive_download_proxy_command(args)
                && !args.contains(&"--stdout".to_string()) =>
            {
                if let Some(path) = take_flag_value(&rewritten, i) {
                    let local_out = expand_tilde(path);
                    let remote_path = staging_path(&staging_dir, &local_out);
                    download = Some(StagedDownload {
                        remote_path: remote_path.clone(),
                        local_out,
                    });
                    rewritten[i + 1] = remote_path;
                    i += 2;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }

    Ok(ProxyFileTransferPlan {
        args: rewritten,
        uploads,
        download,
    })
}

/// Rewrite staged `~` paths to absolute remote paths for the proxied `void` command.
pub fn resolve_staged_paths_for_remote(
    ssh: &SshTarget,
    plan: &mut ProxyFileTransferPlan,
) -> Result<(), ConfigError> {
    for upload in &plan.uploads {
        let resolved = resolve_remote_path(ssh, &upload.remote_path)?;
        replace_flag_value(&mut plan.args, "--file", &upload.remote_path, &resolved);
    }
    if let Some(download) = &plan.download {
        let resolved = resolve_remote_path(ssh, &download.remote_path)?;
        for flag in ["--out", "--output", "-o"] {
            replace_flag_value(&mut plan.args, flag, &download.remote_path, &resolved);
        }
    }
    Ok(())
}

pub fn execute_proxy_uploads(
    ssh: &SshTarget,
    remote_store_path: &str,
    uploads: &[StagedUpload],
) -> Result<(), ConfigError> {
    if uploads.is_empty() {
        return Ok(());
    }

    let staging_dir = format!("{remote_store_path}/staging");
    ensure_remote_dir(ssh, &staging_dir)?;

    for upload in uploads {
        ssh.scp_to(&upload.local_path, &upload.remote_path)?;
    }
    Ok(())
}

/// Create the remote staging directory ahead of a staged download.
/// Uploads create it themselves in [`execute_proxy_uploads`]; download-only
/// commands need it to exist before the proxied `void` writes its `--out` file.
pub fn ensure_remote_staging(ssh: &SshTarget, remote_store_path: &str) -> Result<(), ConfigError> {
    ensure_remote_dir(ssh, &format!("{remote_store_path}/staging"))
}

pub fn execute_proxy_download(
    ssh: &SshTarget,
    download: &StagedDownload,
) -> Result<(), ConfigError> {
    if let Some(parent) = download.local_out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    fetch_remote_file(ssh, &download.remote_path, &download.local_out)?;
    Ok(())
}

pub fn cleanup_remote_staging(ssh: &SshTarget, paths: &[String]) {
    for path in paths {
        let resolved = match resolve_remote_path(ssh, path) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let escaped = shell_escape_path(&resolved);
        let _ = ssh.run_remote(&format!("rm -f {escaped}"));
    }
}

fn ensure_remote_dir(ssh: &SshTarget, dir: &str) -> Result<(), ConfigError> {
    let resolved = if dir.starts_with('~') {
        ssh.resolve_path_on_host(dir)?
    } else {
        dir.to_string()
    };
    let escaped = shell_escape_path(&resolved);
    let output = ssh.run_remote(&format!("mkdir -p {escaped}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ConfigError::Remote(format!(
            "failed to create remote staging directory ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

fn resolve_remote_path(ssh: &SshTarget, path: &str) -> Result<String, ConfigError> {
    if path.starts_with('~') {
        ssh.resolve_path_on_host(path)
    } else {
        Ok(path.to_string())
    }
}

fn replace_flag_value(args: &mut [String], flag: &str, old: &str, new: &str) {
    let mut i = 0;
    while i + 1 < args.len() {
        if args[i] == flag && args[i + 1] == old {
            args[i + 1] = new.to_string();
            return;
        }
        i += 1;
    }
}

fn staging_path(staging_dir: &str, local_path: &Path) -> String {
    let basename = local_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    format!("{staging_dir}/{}-{basename}", Uuid::new_v4())
}

fn take_flag_value(args: &[String], flag_index: usize) -> Option<&str> {
    let value_index = flag_index + 1;
    if value_index < args.len() && !args[value_index].starts_with('-') {
        Some(args[value_index].as_str())
    } else {
        None
    }
}

fn is_download_proxy_command(args: &[String]) -> bool {
    matches!(
        proxy_command_verb(args),
        Some(("gmail", "attachment"))
            | Some(("whatsapp", "download"))
            | Some(("telegram", "download"))
            | Some(("linkedin", "download"))
    )
}

fn is_drive_download_proxy_command(args: &[String]) -> bool {
    matches!(proxy_command_verb(args), Some(("drive", "download")))
}

fn proxy_command_verb(args: &[String]) -> Option<(&str, &str)> {
    let first = args.first().map(|s| s.as_str())?;
    let second = args.get(1).map(|s| s.as_str())?;
    Some((first, second))
}

fn shell_escape_path(path: &str) -> String {
    if path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '@'))
    {
        path.to_string()
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_download_proxy_commands() {
        assert!(is_download_proxy_command(&[
            "gmail".into(),
            "attachment".into(),
            "m1".into(),
            "a1".into(),
            "--out".into(),
            "/tmp/x".into(),
        ]));
        assert!(is_download_proxy_command(&[
            "whatsapp".into(),
            "download".into(),
            "m1".into(),
            "--out".into(),
            "/tmp/x".into(),
        ]));
        assert!(!is_download_proxy_command(&[
            "send".into(),
            "--via".into(),
            "gmail".into(),
            "--file".into(),
            "/tmp/x".into(),
        ]));
    }

    #[test]
    fn plan_rewrites_file_for_upload() {
        let tmp = std::env::temp_dir().join(format!("void-proxy-upload-{}", Uuid::new_v4()));
        std::fs::write(&tmp, b"hi").unwrap();

        let plan = plan_proxy_file_transfer(
            "~/.local/share/void",
            &[
                "send".into(),
                "--via".into(),
                "gmail".into(),
                "--to".into(),
                "a@b.com".into(),
                "--message".into(),
                "hi".into(),
                "--file".into(),
                tmp.to_string_lossy().into_owned(),
            ],
        )
        .unwrap();

        assert_eq!(plan.uploads.len(), 1);
        assert!(plan.uploads[0]
            .remote_path
            .starts_with("~/.local/share/void/staging/"));
        assert!(plan.download.is_none());
        let file_arg = plan
            .args
            .windows(2)
            .find(|w| w[0] == "--file")
            .map(|w| w[1].clone())
            .unwrap();
        assert_eq!(file_arg, plan.uploads[0].remote_path);

        let _ = std::fs::remove_file(tmp);
    }

    #[test]
    fn plan_rewrites_out_for_download() {
        let plan = plan_proxy_file_transfer(
            "~/.local/share/void",
            &[
                "gmail".into(),
                "attachment".into(),
                "m1".into(),
                "a1".into(),
                "--out".into(),
                "~/Downloads/file.pdf".into(),
            ],
        )
        .unwrap();

        assert!(plan.uploads.is_empty());
        let download = plan.download.as_ref().unwrap();
        assert!(download
            .remote_path
            .starts_with("~/.local/share/void/staging/"));
        assert_eq!(download.local_out, expand_tilde("~/Downloads/file.pdf"));
        let out_arg = plan
            .args
            .windows(2)
            .find(|w| w[0] == "--out")
            .map(|w| w[1].clone())
            .unwrap();
        assert_eq!(out_arg, download.remote_path);
    }

    #[test]
    fn plan_errors_when_local_file_missing() {
        let err = plan_proxy_file_transfer(
            "~/.local/share/void",
            &[
                "reply".into(),
                "m1".into(),
                "--message".into(),
                "hi".into(),
                "--file".into(),
                "/no/such/file.pdf".into(),
            ],
        )
        .unwrap_err();
        assert!(err.to_string().contains("local file not found"));
    }

    #[test]
    fn plan_rewrites_drive_download_output() {
        for flag in ["--output", "-o"] {
            let plan = plan_proxy_file_transfer(
                "/var/void",
                &[
                    "drive".into(),
                    "download".into(),
                    "https://drive.google.com/file/d/abc".into(),
                    flag.into(),
                    "~/out/doc.pdf".into(),
                ],
            )
            .unwrap();
            assert!(plan.uploads.is_empty());
            let download = plan.download.as_ref().unwrap();
            assert!(download.remote_path.starts_with("/var/void/staging/"));
            assert_eq!(download.local_out, expand_tilde("~/out/doc.pdf"));
            let out_arg = plan
                .args
                .windows(2)
                .find(|w| w[0] == flag)
                .map(|w| w[1].clone())
                .unwrap();
            assert_eq!(out_arg, download.remote_path);
        }
    }

    #[test]
    fn plan_skips_drive_download_when_stdout() {
        let plan = plan_proxy_file_transfer(
            "/var/void",
            &[
                "drive".into(),
                "download".into(),
                "file-id".into(),
                "--stdout".into(),
                "--output".into(),
                "/tmp/ignored.pdf".into(),
            ],
        )
        .unwrap();
        assert!(plan.download.is_none());
        assert_eq!(
            plan.args
                .windows(2)
                .find(|w| w[0] == "--output")
                .map(|w| w[1].as_str()),
            Some("/tmp/ignored.pdf")
        );
    }

    #[test]
    fn plan_rewrites_download_for_all_connectors() {
        for (verb, sub) in [
            ("gmail", "attachment"),
            ("whatsapp", "download"),
            ("telegram", "download"),
            ("linkedin", "download"),
        ] {
            let plan = plan_proxy_file_transfer(
                "/store",
                &[
                    verb.into(),
                    sub.into(),
                    "m1".into(),
                    "--out".into(),
                    "/tmp/out.bin".into(),
                ],
            )
            .unwrap();
            assert!(
                plan.download.is_some(),
                "{verb} {sub} should stage download"
            );
        }
    }

    #[test]
    fn plan_rewrites_gmail_draft_create_file() {
        let tmp = std::env::temp_dir().join(format!("void-proxy-draft-{}", Uuid::new_v4()));
        std::fs::write(&tmp, b"draft").unwrap();

        let plan = plan_proxy_file_transfer(
            "/store",
            &[
                "gmail".into(),
                "draft".into(),
                "create".into(),
                "--to".into(),
                "a@b.com".into(),
                "--subject".into(),
                "s".into(),
                "--body".into(),
                "b".into(),
                "--file".into(),
                tmp.to_string_lossy().into_owned(),
            ],
        )
        .unwrap();

        assert_eq!(plan.uploads.len(), 1);
        assert_eq!(plan.uploads[0].local_path, tmp);
        let _ = std::fs::remove_file(tmp);
    }

    #[test]
    fn plan_preserves_unrelated_args() {
        let plan = plan_proxy_file_transfer(
            "/store",
            &[
                "gmail".into(),
                "attachment".into(),
                "m1".into(),
                "a1".into(),
                "--connection".into(),
                "work@gmail".into(),
                "--out".into(),
                "/tmp/x".into(),
            ],
        )
        .unwrap();
        assert!(plan
            .args
            .windows(2)
            .any(|w| w[0] == "--connection" && w[1] == "work@gmail"));
    }

    #[test]
    fn shell_escape_path_quotes_spaces() {
        assert_eq!(
            shell_escape_path("/path/with spaces/file"),
            "'/path/with spaces/file'"
        );
    }
}
