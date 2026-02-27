pub mod bluebubbles;
pub mod discord;
pub mod draft_stream;
pub mod feishu;
pub mod google_chat;
pub mod group_gate;
pub mod irc;
pub mod line;
pub mod manager;
pub mod matrix;
pub mod mattermost;
pub mod msteams;
pub mod nextcloud;
pub mod nostr;
pub mod policy;
pub mod router;
pub mod signal;
pub mod slack;
pub mod streaming;
pub mod synology;
pub mod telegram;
pub mod traits;
pub mod twitch;
pub mod types;
pub mod webchat;
pub mod whatsapp;
pub mod zalo;

pub use draft_stream::{DraftStreamHandle, DraftStreamLoop};
pub use manager::{ChannelFactory, ChannelInfo, ChannelManager, ChannelRegistry};
pub use router::{
    Attachment, Message, MessageContent, MessageQueue, MessageReaction, MessageRecipient,
    MessageRouter, MessageSender as RouterMessageSender, MessageType, QueueError,
    normalize_message,
};
pub use traits::*;
