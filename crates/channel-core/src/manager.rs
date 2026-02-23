use crate::traits::*;
use crate::webchat::WebchatChannel;
use crate::whatsapp::WhatsAppChannel;
use crate::telegram::TelegramChannel;
use crate::discord::DiscordChannel;
use crate::slack::SlackChannel;
use crate::signal::SignalChannel;
use crate::line::LineChannel;
use crate::matrix::MatrixChannel;
use crate::nostr::NostrChannel;
use crate::irc::IrcChannel;
use crate::google_chat::GoogleChatChannel;
use crate::mattermost::MattermostChannel;
use oclaws_config::settings::Channels;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::fmt;

pub struct ChannelManager {
    channels: HashMap<String, Arc<RwLock<dyn Channel>>>,
}

impl ChannelManager {
    pub fn new() -> Self {
        Self {
            channels: HashMap::new(),
        }
    }

    pub async fn from_config(config: &Channels) -> Self {
        let mut manager = Self::new();

        if let Some(webchat) = &config.webchat
            && webchat.enabled.unwrap_or(false) {
                let mut channel = WebchatChannel::new();
                if let Some(auth) = &webchat.auth
                    && let (Some(username), Some(password)) = (&auth.username, &auth.password) {
                        channel = channel.with_credentials(username, password);
                    }
                manager.register("webchat".to_string(), channel).await;
            }

        if let Some(whatsapp) = &config.whatsapp
            && whatsapp.enabled.unwrap_or(false)
                && let (Some(phone_number_id), Some(api_token)) = (&whatsapp.phone_number_id, &whatsapp.api_token) {
                    let channel = WhatsAppChannel::new()
                        .with_config(phone_number_id, api_token, whatsapp.business_account_id.as_deref());
                    manager.register("whatsapp".to_string(), channel).await;
                }

        if let Some(telegram) = &config.telegram
            && telegram.enabled.unwrap_or(false)
                && let Some(bot_token) = &telegram.bot_token {
                    let channel = TelegramChannel::new()
                        .with_bot_token(bot_token);
                    manager.register("telegram".to_string(), channel).await;
                }

        if let Some(discord) = &config.discord
            && discord.enabled.unwrap_or(false)
                && let (Some(bot_token), Some(guild_id)) = (&discord.bot_token, &discord.guild_id) {
                    let mut channel = DiscordChannel::new()
                        .with_config(bot_token, guild_id);
                    
                    if let Some(channel_ids) = &discord.channel_ids {
                        for ch in channel_ids {
                            channel = channel.add_channel(ch);
                        }
                    }
                    
                    manager.register("discord".to_string(), channel).await;
                }

        if let Some(slack) = &config.slack
            && slack.enabled.unwrap_or(false)
                && let Some(bot_token) = &slack.bot_token {
                    let mut channel = SlackChannel::new()
                        .with_config(bot_token, slack.signing_secret.as_deref());
                    
                    if let Some(channel_ids) = &slack.channel_ids {
                        for ch in channel_ids {
                            channel = channel.add_channel(ch);
                        }
                    }
                    
                    manager.register("slack".to_string(), channel).await;
                }

        if let Some(signal) = &config.signal
            && signal.enabled.unwrap_or(false)
                && let (Some(phone_number), Some(api_url)) = (&signal.phone_number, &signal.api_url) {
                    let channel = SignalChannel::with_config(
                        phone_number,
                        signal.signal_cli_path.as_deref(),
                        Some(api_url),
                    );
                    manager.register("signal".to_string(), channel).await;
                }

        if let Some(line) = &config.line
            && line.enabled.unwrap_or(false)
                && let Some(access_token) = &line.channel_access_token {
                    let mut channel = LineChannel::new()
                        .with_config(access_token, line.channel_secret.as_deref());
                    
                    if let Some(user_id) = &line.user_id {
                        channel = channel.with_user_id(user_id);
                    }
                    
                    manager.register("line".to_string(), channel).await;
                }

        if let Some(matrix) = &config.matrix
            && matrix.enabled.unwrap_or(false)
                && let (Some(homeserver), Some(user_id), Some(access_token)) = (&matrix.homeserver, &matrix.user_id, &matrix.access_token) {
                    let mut channel = MatrixChannel::new()
                        .with_config(homeserver, user_id, access_token);
                    
                    if let Some(device_id) = &matrix.device_id {
                        channel = channel.with_device_id(device_id);
                    }
                    
                    if let Some(room_id) = &matrix.room_id {
                        channel = channel.with_room(room_id);
                    }
                    
                    manager.register("matrix".to_string(), channel).await;
                }

        if let Some(nostr) = &config.nostr
            && nostr.enabled.unwrap_or(false)
                && let (Some(private_key), Some(public_key)) = (&nostr.private_key, &nostr.public_key) {
                    let relay_urls = nostr.relay_urls.clone().unwrap_or_else(Vec::new);
                    let channel = NostrChannel::new()
                        .with_relays(relay_urls.iter().map(|s| s.as_str()).collect())
                        .with_keys(private_key, public_key);
                    manager.register("nostr".to_string(), channel).await;
                }

        if let Some(irc) = &config.irc
            && irc.enabled.unwrap_or(false)
                && let (Some(server), Some(nick)) = (&irc.server, &irc.nick) {
                    let mut channel = IrcChannel::new()
                        .with_config(server, nick)
                        .with_port(irc.port.unwrap_or(6667));
                    
                    if let Some(password) = &irc.password {
                        channel = channel.with_password(password);
                    }
                    
                    if let Some(channels) = &irc.channels {
                        for ch in channels {
                            channel = channel.join_channel(ch);
                        }
                    }
                    
                    manager.register("irc".to_string(), channel).await;
                }

        if let Some(google_chat) = &config.google_chat
            && google_chat.enabled.unwrap_or(false)
                && let Some(space_name) = &google_chat.space_name {
                    let channel = GoogleChatChannel::new()
                        .with_space(space_name)
                        .with_service_account(google_chat.service_account_json.as_deref().unwrap_or(""));
                    manager.register("google_chat".to_string(), channel).await;
                }

        if let Some(mattermost) = &config.mattermost
            && mattermost.enabled.unwrap_or(false)
                && let (Some(server_url), Some(access_token)) = (&mattermost.server_url, &mattermost.access_token) {
                    let mut channel = MattermostChannel::new()
                        .with_config(server_url, access_token);
                    
                    if let Some(team_id) = &mattermost.team_id {
                        channel = channel.with_team(team_id);
                    }
                    
                    if let Some(channel_id) = &mattermost.channel_id {
                        channel = channel.with_channel(channel_id);
                    }
                    
                    manager.register("mattermost".to_string(), channel).await;
                }

        manager
    }

    pub async fn register(&mut self, name: String, channel: impl Channel + 'static) {
        self.channels.insert(name, Arc::new(RwLock::new(channel)));
    }

    pub async fn get(&self, name: &str) -> Option<Arc<RwLock<dyn Channel>>> {
        self.channels.get(name).cloned()
    }

    pub async fn list(&self) -> Vec<String> {
        self.channels.keys().cloned().collect()
    }

    pub async fn connect_all(&self) -> ChannelResult<()> {
        for (name, channel) in &self.channels {
            tracing::info!("Connecting channel: {}", name);
            let mut ch = channel.write().await;
            if let Err(e) = ch.connect().await {
                tracing::error!("Failed to connect channel {}: {}", name, e);
            }
        }
        Ok(())
    }

    pub async fn disconnect_all(&self) -> ChannelResult<()> {
        for (name, channel) in &self.channels {
            tracing::info!("Disconnecting channel: {}", name);
            let mut ch = channel.write().await;
            if let Err(e) = ch.disconnect().await {
                tracing::error!("Failed to disconnect channel {}: {}", name, e);
            }
        }
        Ok(())
    }

    pub async fn send_to_channel(&self, channel_name: &str, message: &ChannelMessage) -> ChannelResult<String> {
        let channel = self.channels.get(channel_name)
            .ok_or_else(|| ChannelError::NotFound(format!("Channel not found: {}", channel_name)))?;
        
        let ch = channel.read().await;
        ch.send_message(message).await
    }

    pub async fn get_status(&self) -> HashMap<String, ChannelStatus> {
        let mut status = HashMap::new();
        for (name, channel) in &self.channels {
            let ch = channel.read().await;
            status.insert(name.clone(), ch.status());
        }
        status
    }
}

impl Default for ChannelManager {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ChannelFactory;

impl ChannelFactory {
    pub fn create_webchat(username: Option<&str>, password: Option<&str>) -> impl Channel {
        let mut channel = WebchatChannel::new();
        if let (Some(u), Some(p)) = (username, password) {
            channel = channel.with_credentials(u, p);
        }
        channel
    }

    pub fn create_whatsapp(phone_number_id: &str, api_token: &str, business_account_id: Option<&str>) -> impl Channel {
        WhatsAppChannel::new().with_config(phone_number_id, api_token, business_account_id)
    }

    pub fn create_telegram(bot_token: &str) -> impl Channel {
        TelegramChannel::new().with_bot_token(bot_token)
    }

    pub fn create_discord(bot_token: &str, guild_id: &str, channel_ids: Option<Vec<&str>>) -> impl Channel {
        let mut channel = DiscordChannel::new().with_config(bot_token, guild_id);
        if let Some(ids) = channel_ids {
            for id in ids {
                channel = channel.add_channel(id);
            }
        }
        channel
    }

    pub fn create_slack(bot_token: &str, signing_secret: Option<&str>, channel_ids: Option<Vec<&str>>) -> impl Channel {
        let mut channel = SlackChannel::new().with_config(bot_token, signing_secret);
        if let Some(ids) = channel_ids {
            for id in ids {
                channel = channel.add_channel(id);
            }
        }
        channel
    }

    pub fn create_signal(phone_number: &str, signal_cli_path: Option<&str>, api_url: Option<&str>) -> impl Channel {
        SignalChannel::with_config(phone_number, signal_cli_path, api_url)
    }

    pub fn create_line(access_token: &str, channel_secret: Option<&str>, user_id: Option<&str>) -> impl Channel {
        let mut channel = LineChannel::new().with_config(access_token, channel_secret);
        if let Some(uid) = user_id {
            channel = channel.with_user_id(uid);
        }
        channel
    }

    pub fn create_matrix(homeserver: &str, user_id: &str, access_token: &str, device_id: Option<&str>, room_id: Option<&str>) -> impl Channel {
        let mut channel = MatrixChannel::new().with_config(homeserver, user_id, access_token);
        if let Some(did) = device_id {
            channel = channel.with_device_id(did);
        }
        if let Some(rid) = room_id {
            channel = channel.with_room(rid);
        }
        channel
    }

    pub fn create_nostr(relays: Vec<&str>, private_key: &str, public_key: &str) -> impl Channel {
        NostrChannel::new()
            .with_relays(relays)
            .with_keys(private_key, public_key)
    }

    pub fn create_irc(server: &str, nick: &str, port: Option<u16>, password: Option<&str>, channels: Option<Vec<&str>>) -> impl Channel {
        let mut channel = IrcChannel::new().with_config(server, nick);
        if let Some(p) = port {
            channel = channel.with_port(p);
        }
        if let Some(pass) = password {
            channel = channel.with_password(pass);
        }
        if let Some(chs) = channels {
            for ch in chs {
                channel = channel.join_channel(ch);
            }
        }
        channel
    }

    pub fn create_google_chat(space_name: &str, service_account_json: Option<&str>) -> impl Channel {
        GoogleChatChannel::new()
            .with_space(space_name)
            .with_service_account(service_account_json.unwrap_or(""))
    }

    pub fn create_mattermost(server_url: &str, access_token: &str, team_id: Option<&str>, channel_id: Option<&str>) -> impl Channel {
        let mut channel = MattermostChannel::new().with_config(server_url, access_token);
        if let Some(tid) = team_id {
            channel = channel.with_team(tid);
        }
        if let Some(cid) = channel_id {
            channel = channel.with_channel(cid);
        }
        channel
    }
}

#[derive(Debug, Clone)]
pub struct ChannelInfo {
    pub name: String,
    pub channel_type: String,
    pub status: ChannelStatus,
    pub registered_at: i64,
}

type ChannelInstanceMap = HashMap<String, Arc<RwLock<dyn Channel>>>;

pub struct ChannelRegistry {
    channels: Arc<RwLock<HashMap<String, ChannelInfo>>>,
    channel_instances: Arc<RwLock<ChannelInstanceMap>>,
}

impl ChannelRegistry {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            channel_instances: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(
        &self,
        name: String,
        channel_type: String,
        channel: impl Channel + 'static,
    ) -> Result<(), ChannelError> {
        let mut channels = self.channels.write().await;
        let mut instances = self.channel_instances.write().await;

        if channels.contains_key(&name) {
            return Err(ChannelError::ConfigError(format!(
                "Channel '{}' already registered",
                name
            )));
        }

        let info = ChannelInfo {
            name: name.clone(),
            channel_type: channel_type.clone(),
            status: ChannelStatus::Disconnected,
            registered_at: chrono::Utc::now().timestamp(),
        };

        let channel_type_clone = channel_type.clone();
        channels.insert(name.clone(), info);
        instances.insert(name.clone(), Arc::new(RwLock::new(channel)));

        tracing::info!("Channel registered: {} (type: {})", name, channel_type_clone);
        Ok(())
    }

    pub async fn unregister(&self, name: &str) -> Result<(), ChannelError> {
        let mut channels = self.channels.write().await;
        let mut instances = self.channel_instances.write().await;

        if !channels.contains_key(name) {
            return Err(ChannelError::NotFound(format!(
                "Channel '{}' not found",
                name
            )));
        }

        channels.remove(name);
        instances.remove(name);

        tracing::info!("Channel unregistered: {}", name);
        Ok(())
    }

    pub async fn get(&self, name: &str) -> Option<Arc<RwLock<dyn Channel>>> {
        self.channel_instances.read().await.get(name).cloned()
    }

    pub async fn list(&self) -> Vec<ChannelInfo> {
        self.channels.read().await.values().cloned().collect()
    }

    pub async fn get_by_type(&self, channel_type: &str) -> Vec<(String, Arc<RwLock<dyn Channel>>)> {
        let channels = self.channels.read().await;
        let instances = self.channel_instances.read().await;

        channels
            .iter()
            .filter(|(_, info)| info.channel_type == channel_type)
            .filter_map(|(name, _)| {
                instances.get(name).map(|ch| (name.clone(), ch.clone()))
            })
            .collect()
    }

    pub async fn connect(&self, name: &str) -> Result<(), ChannelError> {
        let channel = self.channel_instances.read().await.get(name)
            .ok_or_else(|| ChannelError::NotFound(format!("Channel '{}' not found", name)))?
            .clone();

        let mut ch = channel.write().await;
        ch.connect().await?;

        {
            let mut channels = self.channels.write().await;
            if let Some(info) = channels.get_mut(name) {
                info.status = ChannelStatus::Connected;
            }
        }

        tracing::info!("Channel connected: {}", name);
        Ok(())
    }

    pub async fn disconnect(&self, name: &str) -> Result<(), ChannelError> {
        let channel = self.channel_instances.read().await.get(name)
            .ok_or_else(|| ChannelError::NotFound(format!("Channel '{}' not found", name)))?
            .clone();

        let mut ch = channel.write().await;
        ch.disconnect().await?;

        {
            let mut channels = self.channels.write().await;
            if let Some(info) = channels.get_mut(name) {
                info.status = ChannelStatus::Disconnected;
            }
        }

        tracing::info!("Channel disconnected: {}", name);
        Ok(())
    }

    pub async fn send_message(&self, name: &str, message: &ChannelMessage) -> Result<String, ChannelError> {
        let channel = self.channel_instances.read().await.get(name)
            .ok_or_else(|| ChannelError::NotFound(format!("Channel '{}' not found", name)))?
            .clone();

        let ch = channel.read().await;
        let result = ch.send_message(message).await?;
        Ok(result)
    }

    pub async fn broadcast(&self, message: &ChannelMessage) -> Vec<(String, Result<String, ChannelError>)> {
        let channels = self.channel_instances.read().await;
        let mut results = Vec::new();

        for (name, channel) in channels.iter() {
            let ch = channel.read().await;
            let result = ch.send_message(message).await;
            results.push((name.clone(), result));
        }

        results
    }

    pub async fn get_status(&self) -> HashMap<String, ChannelStatus> {
        self.channels
            .read()
            .await
            .iter()
            .map(|(name, info)| (name.clone(), info.status))
            .collect()
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ChannelRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelRegistry")
            .field("channel_count", &self.channels.try_read().map(|c| c.len()).unwrap_or(0))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webchat::WebchatChannel;

    #[tokio::test]
    async fn test_register_get_list_lifecycle() {
        let mut mgr = ChannelManager::new();
        assert!(mgr.list().await.is_empty());

        mgr.register("webchat".to_string(), WebchatChannel::new()).await;
        let names = mgr.list().await;
        assert_eq!(names.len(), 1);
        assert!(names.contains(&"webchat".to_string()));

        let ch = mgr.get("webchat").await;
        assert!(ch.is_some());

        assert!(mgr.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_send_to_channel_unknown_returns_not_found() {
        let mgr = ChannelManager::new();
        let msg = ChannelMessage {
            id: "1".to_string(),
            channel: "unknown".to_string(),
            sender: String::new(),
            content: "hello".to_string(),
            timestamp: 0,
            metadata: HashMap::new(),
        };
        let result = mgr.send_to_channel("unknown", &msg).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ChannelError::NotFound(_) => {}
            other => panic!("Expected NotFound, got {:?}", other),
        }
    }
}
