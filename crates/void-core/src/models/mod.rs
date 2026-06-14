mod calendar;
mod connector;
mod contact;
mod conversation;
mod health;
mod message;
mod serde_ts;

pub use calendar::CalendarEvent;
pub use connector::ConnectorType;
pub use contact::{Contact, SyncState};
pub use conversation::{Conversation, ConversationKind};
pub use health::{HealthStatus, MessageContent};
pub use message::{parse_reply_id, Message};

#[cfg(test)]
mod tests;
