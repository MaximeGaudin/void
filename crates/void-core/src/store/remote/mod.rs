mod cache;
mod fetch;
mod ssh;

#[cfg(test)]
mod tests;

pub use cache::{cache_is_fresh, default_cache_dir, now_secs, CacheMeta};
pub use fetch::{fetch_remote_file, fetch_remote_files_if_present};
pub use ssh::{RemoteProxyTargets, SshTarget};

/// Prepended to remote SSH commands so `void` and user tools are discoverable.
pub const REMOTE_PATH_PREFIX: &str = "PATH=\"$HOME/bin:$HOME/.local/bin:$HOME/.cargo/bin:$PATH\"";

#[cfg(test)]
pub(crate) use ssh::format_remote_scp_path;
