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
pub mod feishu;
pub mod msteams;
pub mod twitch;
pub mod zalo;
pub mod nextcloud;
pub mod synology;
pub mod bluebubbles;
pub mod manager;
pub mod group_gate;
pub mod draft_stream;

pub use traits::*;
pub use draft_stream::{DraftStreamLoop, DraftStreamHandle};
pub use router::{
    MessageType, Attachment, MessageContent, Message,
    MessageSender as RouterMessageSender, MessageRecipient, MessageReaction,
    MessageQueue, QueueError, MessageRouter, normalize_message,
};
pub use manager::{ChannelManager, ChannelFactory, ChannelRegistry, ChannelInfo};
