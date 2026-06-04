mod connection;
mod ignore;
mod paths;
mod void_config;

#[cfg(test)]
mod tests;

pub use connection::{ConnectionConfig, ConnectionSettings};
pub use ignore::conversation_matches_ignore;
pub use paths::{
    default_config, default_config_path, expand_tilde, redact_token, resolve_config_path,
};
pub use void_config::{
    RemoteCacheConfig, RemoteSshConfig, RemoteStoreConfig, StoreConfig, StoreMode, SyncConfig,
    VoidConfig,
};
