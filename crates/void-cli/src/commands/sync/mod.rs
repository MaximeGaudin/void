//! Continuous sync daemon and one-shot sync runs.

mod args;
mod daemon;
mod engine;
mod lock;
mod status;

pub use args::SyncArgs;
pub use daemon::{daemonize, run_daemon_inner, stop_daemon};
pub use engine::run;
pub use status::show_status;
