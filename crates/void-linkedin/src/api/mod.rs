mod client;
mod types;

pub mod posts;
pub use posts::{AccountOwnerProfile, UnipileComment, UnipileCommentAuthor, UnipilePost};

pub use client::{normalize_api_base, UnipileClient};
pub use types::{
    AccountResponse, ListResponse, UnipileAttachment, UnipileChat, UnipileChatAttendee,
    UnipileMessage, UnipileUserProfile,
};

#[cfg(test)]
mod integration;

#[cfg(test)]
mod tests;
