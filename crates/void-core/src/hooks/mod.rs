mod execute;
mod hook_fs;
mod model;
mod placeholders;
mod runner;

#[cfg(test)]
mod tests;

pub use execute::{execute_hook_public, HookExecOptions, HookExecResult};
pub use hook_fs::{
    delete_hook, find_hook, hook_files, hooks_dir, hooks_dir_for, load_hooks, save_hook, slugify,
    update_hook_enabled,
};
pub use model::{
    ActiveWindow, Hook, HookLog, HookLogInsert, HookRunSummary, PromptConfig, Trigger, Weekday,
};
pub use placeholders::expand_placeholders_public;
pub use runner::HookRunner;
