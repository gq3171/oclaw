pub mod traits;
pub mod router;
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
pub use router::{
    MessageType, Attachment, MessageContent, Message,
    MessageSender as RouterMessageSender, MessageRecipient, MessageReaction,
    MessageQueue, QueueError, MessageRouter, normalize_message,
};
pub use manager::{ChannelManager, ChannelFactory, ChannelRegistry, ChannelInfo};
