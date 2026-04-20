use std::path::{Path, PathBuf};

use tracing::warn;

use crate::error::HookError;

use super::model::Hook;

pub fn hooks_dir() -> PathBuf {
    crate::config::default_config_path()
        .parent()
        .map(|path| path.join("hooks"))
        .unwrap_or_else(|| PathBuf::from(".config/void/hooks"))
}

pub fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn load_hooks(dir: &Path) -> Vec<Hook> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut hooks = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => match toml::from_str::<Hook>(&content) {
                Ok(hook) => hooks.push(hook),
                Err(e) => warn!(path = %path.display(), "invalid hook file: {e}"),
            },
            Err(e) => warn!(path = %path.display(), "cannot read hook file: {e}"),
        }
    }
    hooks
}

pub fn save_hook(dir: &Path, hook: &Hook) -> Result<(), HookError> {
    std::fs::create_dir_all(dir)?;
    let filename = format!("{}.toml", slugify(&hook.name));
    let path = dir.join(filename);
    let content = toml::to_string_pretty(hook).map_err(|e| HookError::Other(e.to_string()))?;
    std::fs::write(&path, content)?;
    Ok(())
}

pub fn delete_hook(dir: &Path, name: &str) -> Result<bool, HookError> {
    let filename = format!("{}.toml", slugify(name));
    let path = dir.join(&filename);
    if path.exists() {
        std::fs::remove_file(&path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn find_hook(dir: &Path, name: &str) -> Result<Hook, HookError> {
    load_hooks(dir)
        .into_iter()
        .find(|h| slugify(&h.name) == slugify(name))
        .ok_or_else(|| HookError::NotFound(name.to_string()))
}

pub fn update_hook_enabled(dir: &Path, name: &str, enabled: bool) -> Result<bool, HookError> {
    match find_hook(dir, name) {
        Ok(mut hook) => {
            hook.enabled = enabled;
            save_hook(dir, &hook)?;
            Ok(true)
        }
        Err(HookError::NotFound(_)) => Ok(false),
        Err(e) => Err(e),
    }
}
