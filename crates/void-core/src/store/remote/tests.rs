use super::{cache_is_fresh, format_remote_scp_path, now_secs, SshTarget};

#[test]
fn scp_path_keeps_tilde_unquoted() {
    assert_eq!(
        format_remote_scp_path("~/.config/void/config.toml"),
        "~/.config/void/config.toml"
    );
}

#[test]
fn scp_path_quotes_spaces() {
    assert_eq!(
        format_remote_scp_path("/path/with spaces/file"),
        "'/path/with spaces/file'"
    );
}

#[test]
fn scp_path_keeps_tilde_for_upload_destination() {
    assert_eq!(
        format_remote_scp_path("~/.local/share/void/staging/uuid-file.pdf"),
        "~/.local/share/void/staging/uuid-file.pdf"
    );
}

// Fake ssh/scp integration tests. `run_remote`, `scp_to` and `scp_from` shell
// out to PATH-resolved `ssh`/`scp`, so we can drop fake scripts into a tempdir,
// prepend it to PATH, and observe argv + error surfacing. PATH is process-global,
// so these tests serialize on a shared mutex.
#[cfg(unix)]
#[cfg(test)]
mod fake_ssh_tests {
    use super::SshTarget;

    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    // Serializes PATH mutation across tests in this module.
    static PATH_GUARD: Mutex<()> = Mutex::new(());

    fn write_script(dir: &std::path::Path, name: &str, body: &str) {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        f.flush().unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
    }

    /// Run `f` with `dir` prepended to PATH, restoring PATH afterwards.
    fn with_path_prefix<T>(dir: &std::path::Path, f: impl FnOnce() -> T) -> T {
        let _guard = PATH_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let original = std::env::var_os("PATH");
        let mut new_path = std::ffi::OsString::from(dir);
        if let Some(orig) = &original {
            new_path.push(":");
            new_path.push(orig);
        }
        std::env::set_var("PATH", &new_path);
        let result = f();
        match original {
            Some(p) => std::env::set_var("PATH", p),
            None => std::env::remove_var("PATH"),
        }
        result
    }

    fn target(port: u16) -> SshTarget {
        SshTarget {
            host: "fakehost".into(),
            user: Some("bob".into()),
            port,
            identity_file: None,
        }
    }

    #[test]
    fn run_remote_argv_includes_destination_and_command() {
        let dir = tempfile::tempdir().unwrap();
        let argv_log = dir.path().join("ssh-argv.txt");
        // Fake ssh records its argv (one per line) and prints fixed stdout.
        write_script(
            dir.path(),
            "ssh",
            &format!(
                "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{}'\necho REMOTE_OK\nexit 0\n",
                argv_log.display()
            ),
        );

        let out = with_path_prefix(dir.path(), || target(2222).run_remote("echo hi").unwrap());
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "REMOTE_OK");

        let logged = std::fs::read_to_string(&argv_log).unwrap();
        let args: Vec<&str> = logged.lines().collect();
        // base_ssh_args: -o BatchMode=yes -o StrictHostKeyChecking=accept-new -p <port>
        assert!(args.contains(&"BatchMode=yes"), "argv: {args:?}");
        assert!(
            args.contains(&"StrictHostKeyChecking=accept-new"),
            "argv: {args:?}"
        );
        let port_idx = args.iter().position(|a| *a == "-p").unwrap();
        assert_eq!(args[port_idx + 1], "2222", "port forwarded as -p value");
        assert!(args.contains(&"bob@fakehost"), "destination present");
        assert!(args.contains(&"echo hi"), "remote command is last arg");
    }

    #[test]
    fn run_remote_surfaces_nonzero_exit_via_output_status() {
        let dir = tempfile::tempdir().unwrap();
        write_script(
            dir.path(),
            "ssh",
            "#!/bin/sh\necho 'connection refused' 1>&2\nexit 255\n",
        );

        let out = with_path_prefix(dir.path(), || target(22).run_remote("whoami").unwrap());
        assert!(!out.status.success(), "non-zero exit propagated");
        assert!(String::from_utf8_lossy(&out.stderr).contains("connection refused"));
    }

    #[test]
    fn scp_to_argv_orders_local_then_remote_and_uses_capital_p_port() {
        let dir = tempfile::tempdir().unwrap();
        let argv_log = dir.path().join("scp-argv.txt");
        write_script(
            dir.path(),
            "scp",
            &format!(
                "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{}'\nexit 0\n",
                argv_log.display()
            ),
        );

        let local = dir.path().join("payload.bin");
        std::fs::write(&local, b"data").unwrap();

        with_path_prefix(dir.path(), || {
            target(2200)
                .scp_to(&local, "/remote/dir/payload.bin")
                .unwrap()
        });

        let logged = std::fs::read_to_string(&argv_log).unwrap();
        let args: Vec<&str> = logged.lines().collect();
        // scp uses -P (capital) for port, unlike ssh's -p.
        let p_idx = args.iter().position(|a| *a == "-P").unwrap();
        assert_eq!(args[p_idx + 1], "2200");
        // Last two positionals: local source, then remote dest.
        let last = args.last().unwrap();
        let second_last = &args[args.len() - 2];
        assert_eq!(*second_last, local.to_string_lossy());
        assert_eq!(*last, "bob@fakehost:/remote/dir/payload.bin");
    }

    #[test]
    fn scp_to_surfaces_error_when_fake_exits_nonzero() {
        let dir = tempfile::tempdir().unwrap();
        write_script(
            dir.path(),
            "scp",
            "#!/bin/sh\necho 'permission denied' 1>&2\nexit 1\n",
        );
        let local = dir.path().join("f.bin");
        std::fs::write(&local, b"x").unwrap();

        let err = with_path_prefix(dir.path(), || {
            target(22).scp_to(&local, "/remote/f.bin").unwrap_err()
        });
        let msg = err.to_string();
        assert!(msg.contains("scp failed"), "msg: {msg}");
        assert!(msg.contains("permission denied"), "stderr surfaced: {msg}");
    }

    #[test]
    fn execute_proxy_uploads_runs_mkdir_then_scp_in_order() {
        use crate::store::proxy_files::{execute_proxy_uploads, StagedUpload};

        let dir = tempfile::tempdir().unwrap();
        let order_log = dir.path().join("order.txt");
        // ssh handles the mkdir -p staging step; record that it ran.
        write_script(
            dir.path(),
            "ssh",
            &format!(
                "#!/bin/sh\nprintf 'ssh\\n' >> '{}'\nexit 0\n",
                order_log.display()
            ),
        );
        write_script(
            dir.path(),
            "scp",
            &format!(
                "#!/bin/sh\nprintf 'scp\\n' >> '{}'\nexit 0\n",
                order_log.display()
            ),
        );

        let local = dir.path().join("attach.pdf");
        std::fs::write(&local, b"pdf").unwrap();
        let uploads = vec![StagedUpload {
            local_path: local,
            remote_path: "/store/staging/x-attach.pdf".into(),
        }];

        with_path_prefix(dir.path(), || {
            execute_proxy_uploads(&target(22), "/store", &uploads).unwrap();
        });

        let order = std::fs::read_to_string(&order_log).unwrap();
        let steps: Vec<&str> = order.lines().collect();
        // ensure_remote_dir (ssh mkdir) must precede the scp upload.
        assert_eq!(steps.first(), Some(&"ssh"), "mkdir before scp: {steps:?}");
        assert!(steps.contains(&"scp"), "scp upload ran: {steps:?}");
        let ssh_pos = steps.iter().position(|s| *s == "ssh").unwrap();
        let scp_pos = steps.iter().position(|s| *s == "scp").unwrap();
        assert!(ssh_pos < scp_pos, "staging mkdir precedes scp: {steps:?}");
    }
}

#[cfg(test)]
mod ssh_tests {
    use super::{cache_is_fresh, now_secs, SshTarget};

    #[test]
    fn ssh_destination_with_user() {
        let target = SshTarget {
            host: "homeserver".into(),
            user: Some("alice".into()),
            port: 22,
            identity_file: None,
        };
        assert_eq!(target.destination(), "alice@homeserver");
    }

    #[test]
    fn cache_freshness_respects_ttl() {
        let now = now_secs();
        assert!(cache_is_fresh(now, 30));
        assert!(!cache_is_fresh(now.saturating_sub(60), 30));
        assert!(!cache_is_fresh(now, 0));
    }
}
