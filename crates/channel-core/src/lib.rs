pub mod traits;
pub mod webchat;
pub mod whatsapp;
pub mod telegram;
pub mod discord;
pub mod slack;
pub mod signal;
pub mod line;
pub mod matrix;
pub mod nostr;
pub mod irc;
pub mod google_chat;
pub mod mattermost;
pub mod manager;

pub use traits::*;
pub use manager::{ChannelManager, ChannelFactory, ChannelRegistry, ChannelInfo};
