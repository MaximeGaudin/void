//! Google Calendar connector: token refresh, incremental sync, event CRUD, and `Connector` trait.

mod connector_trait;
mod events;
mod mapping;
mod sync_ops;
mod types;

#[cfg(test)]
mod tests;

pub use types::{CalendarConnector, CreateEventParams, UpdateEventParams};
