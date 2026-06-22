mod connection;
mod ignore;
mod paths;
mod secure_fs;
mod void_config;

#[cfg(test)]
mod tests;

pub use connection::{
    empty_settings, settings_i64, settings_set_opt_string, settings_set_string,
    settings_set_string_list, settings_set_u32, settings_str, settings_string,
    settings_string_list, settings_u32, ConnectionConfig,
};
pub use ignore::conversation_matches_ignore;
pub use paths::{
    default_config, default_config_path, expand_tilde, redact_token, resolve_config_path,
};
pub use secure_fs::{restrict_file, write_secure};
pub use void_config::{
    RemoteCacheConfig, RemoteSshConfig, RemoteStoreConfig, StoreConfig, StoreMode, SyncConfig,
    VoidConfig,
};
