use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Target directory (default: /usr/local/bin)
    #[arg(long)]
    pub dir: Option<PathBuf>,
}

fn default_install_dir() -> PathBuf {
    PathBuf::from("/usr/local/bin")
}

pub fn run(args: &InstallArgs) -> anyhow::Result<()> {
    let src = std::env::current_exe()?;
    let dest_dir = args.dir.clone().unwrap_or_else(default_install_dir);
    let dest = dest_dir.join("void");

    if !dest_dir.exists() {
        anyhow::bail!(
            "Directory {} does not exist. Create it or choose another with --dir.",
            dest_dir.display()
        );
    }

    if src == dest {
        eprintln!("Already installed at {}", dest.display());
        return Ok(());
    }

    eprintln!("Installing void → {}", dest.display());

    fs::copy(&src, &dest).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            anyhow::anyhow!(
                "Permission denied writing to {}. Try with sudo or use --dir to pick a user-writable path.",
                dest_dir.display()
            )
        } else {
            e.into()
        }
    })?;

    fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;

    eprintln!("Installed successfully.");

    if !is_on_path(&dest_dir) {
        eprintln!(
            "\nWarning: {} is not on your PATH. Add it with:\n  export PATH=\"{}:$PATH\"",
            dest_dir.display(),
            dest_dir.display()
        );
    }

    Ok(())
}

fn is_on_path(dir: &PathBuf) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d == *dir))
        .unwrap_or(false)
}
