mod model;
mod hook_fs;
mod placeholders;
mod execute;
mod runner;

#[cfg(test)]
mod tests;

pub use execute::{execute_hook_public, HookExecResult};
pub use hook_fs::{
    delete_hook, find_hook, hooks_dir, load_hooks, save_hook, slugify, update_hook_enabled,
};
pub use model::{Hook, HookLog, HookLogInsert, PromptConfig, Trigger};
pub use placeholders::expand_placeholders_public;
pub use runner::HookRunner;
