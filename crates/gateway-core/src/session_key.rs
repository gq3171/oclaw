use oclaws_channel_core::types::ChatType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// DM session scope — controls how session keys are generated for direct messages.
/// Mirrors Node OpenClaw's `dmScope` configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DmScope {
    /// Single shared session across all channels and peers (default — aligns with Node dmScope="main").
    #[default]
    Main,
    /// Same peer shares a session across channels (cross-platform identity).
    PerPeer,
    /// Per-channel per-peer (each channel has its own session).
    PerChannelPeer,
    /// Most granular: per-account per-channel per-peer.
    PerAccountChannelPeer,
}

/// Cross-channel identity links: maps a canonical peer ID to channel-specific IDs.
/// Example: `{ "alice": ["telegram:123456", "slack:U0ABC"] }`
/// When a message arrives from `telegram:123456`, we resolve it to canonical "alice".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IdentityLinks {
    /// canonical_id → vec of "channel:peer_id" strings
    pub links: HashMap<String, Vec<String>>,
}

impl IdentityLinks {
    /// Resolve a channel-specific peer to a canonical identity.
    /// Returns the canonical ID if found, otherwise None.
    pub fn resolve(&self, channel: &str, peer_id: &str) -> Option<&str> {
        let needle = format!("{}:{}", channel, peer_id);
        for (canonical, entries) in &self.links {
            if entries.iter().any(|e| e == &needle) {
                return Some(canonical.as_str());
            }
        }
        None
    }
}

/// Composite session key: `channel:chatType:chatId[:userId]`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionKey {
    pub channel: String,
    pub chat_type: ChatType,
    pub chat_id: String,
    pub user_id: Option<String>,
}

impl SessionKey {
    pub fn new(
        channel: &str,
        chat_type: ChatType,
        chat_id: &str,
        user_id: Option<&str>,
    ) -> Self {
        Self {
            channel: channel.to_string(),
            chat_type,
            chat_id: chat_id.to_string(),
            user_id: user_id.map(|s| s.to_string()),
        }
    }

    /// Parse `"channel:chatType:chatId[:userId]"`.
    pub fn parse(raw: &str) -> Option<Self> {
        let parts: Vec<&str> = raw.splitn(4, ':').collect();
        if parts.len() < 3 {
            return None;
        }
        let chat_type = match parts[1] {
            "direct" => ChatType::Direct,
            "group" => ChatType::Group,
            "channel" => ChatType::Channel,
            "thread" => ChatType::Thread,
            _ => return None,
        };
        Some(Self {
            channel: parts[0].to_string(),
            chat_type,
            chat_id: parts[2].to_string(),
            user_id: parts.get(3).map(|s| s.to_string()),
        })
    }

    pub fn is_dm(&self) -> bool {
        self.chat_type == ChatType::Direct
    }

    pub fn is_group(&self) -> bool {
        self.chat_type == ChatType::Group
    }
}

impl fmt::Display for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.channel, self.chat_type, self.chat_id)?;
        if let Some(ref uid) = self.user_id {
            write!(f, ":{}", uid)?;
        }
        Ok(())
    }
}

/// Build a session ID string based on DmScope configuration.
///
/// For group chats, always uses `{channel}_{chat_id}` (groups are inherently per-channel).
/// For DMs, the scope determines how the key is constructed:
/// - `Main` → `"main"` (single shared session)
/// - `PerPeer` → `"{peer}"` (resolved via identity links if available)
/// - `PerChannelPeer` → `"{channel}_{peer}"` (default)
/// - `PerAccountChannelPeer` → `"{channel}_{account}_{peer}"`
pub fn build_session_id(
    channel: &str,
    chat_id: &str,
    is_group: bool,
    scope: DmScope,
    identity_links: Option<&IdentityLinks>,
    account_id: Option<&str>,
) -> String {
    // Groups always use per-channel scope
    if is_group {
        return format!("{}_{}", channel, chat_id);
    }

    // Resolve cross-channel identity if links are available
    let resolved_peer = identity_links
        .and_then(|links| links.resolve(channel, chat_id))
        .unwrap_or(chat_id);

    match scope {
        DmScope::Main => "main".to_string(),
        DmScope::PerPeer => resolved_peer.to_string(),
        DmScope::PerChannelPeer => format!("{}_{}", channel, resolved_peer),
        DmScope::PerAccountChannelPeer => {
            let acct = account_id.unwrap_or("default");
            format!("{}_{}_{}", channel, acct, resolved_peer)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let key = SessionKey::new("telegram", ChatType::Group, "12345", Some("u1"));
        let s = key.to_string();
        assert_eq!(s, "telegram:group:12345:u1");
        let parsed = SessionKey::parse(&s).unwrap();
        assert_eq!(parsed, key);
    }

    #[test]
    fn parse_no_user() {
        let key = SessionKey::parse("slack:direct:C123").unwrap();
        assert!(key.is_dm());
        assert!(key.user_id.is_none());
    }

    #[test]
    fn parse_invalid() {
        assert!(SessionKey::parse("bad").is_none());
        assert!(SessionKey::parse("a:unknown:b").is_none());
    }

    #[test]
    fn dm_scope_main() {
        let id = build_session_id("telegram", "123", false, DmScope::Main, None, None);
        assert_eq!(id, "main");
    }

    #[test]
    fn dm_scope_per_peer() {
        let id = build_session_id("telegram", "123", false, DmScope::PerPeer, None, None);
        assert_eq!(id, "123");
    }

    #[test]
    fn dm_scope_per_channel_peer() {
        let id = build_session_id("telegram", "123", false, DmScope::PerChannelPeer, None, None);
        assert_eq!(id, "telegram_123");
    }

    #[test]
    fn dm_scope_group_always_per_channel() {
        let id = build_session_id("slack", "G999", true, DmScope::Main, None, None);
        assert_eq!(id, "slack_G999");
    }

    #[test]
    fn identity_links_resolve() {
        let mut links = IdentityLinks::default();
        links.links.insert("alice".to_string(), vec![
            "telegram:123".to_string(),
            "slack:U0ABC".to_string(),
        ]);
        assert_eq!(links.resolve("telegram", "123"), Some("alice"));
        assert_eq!(links.resolve("slack", "U0ABC"), Some("alice"));
        assert_eq!(links.resolve("discord", "999"), None);
    }

    #[test]
    fn cross_channel_session_via_links() {
        let mut links = IdentityLinks::default();
        links.links.insert("alice".to_string(), vec![
            "telegram:123".to_string(),
            "whatsapp:456".to_string(),
        ]);
        let id1 = build_session_id("telegram", "123", false, DmScope::PerPeer, Some(&links), None);
        let id2 = build_session_id("whatsapp", "456", false, DmScope::PerPeer, Some(&links), None);
        assert_eq!(id1, "alice");
        assert_eq!(id2, "alice");
        assert_eq!(id1, id2); // same session!
    }
}
