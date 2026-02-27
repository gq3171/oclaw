use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::ToolResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResponse {
    pub id: String,
    pub result: serde_json::Value,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Tool {
    Bash(BashTool),
    ReadFile(ReadFileTool),
    WriteFile(WriteFileTool),
    ListDir(ListDirTool),
    WebFetch(WebFetchTool),
    Memory(MemoryTool),
    MemorySearch(MemorySearchTool),
    MemoryGet(MemoryGetTool),
    Browse(BrowseTool),
    WebSearch(WebSearchTool),
    LinkReader(LinkReaderTool),
    MediaDescribe(MediaDescribeTool),
    Cron(CronTool),
    Message(MessageTool),
    SessionsList(SessionsListTool),
    SessionsHistory(SessionsHistoryTool),
    SessionsSend(SessionsSendTool),
    SessionsSpawn(SessionsSpawnTool),
    Subagents(SubagentsTool),
    SessionStatus(SessionStatusTool),
    Tts(TtsTool),
    Workspace(WorkspaceTool),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BashTool {
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

fn default_timeout() -> u64 {
    30
}

impl Tool {
    pub fn name(&self) -> &str {
        match self {
            Tool::Bash(_) => "bash",
            Tool::ReadFile(_) => "read_file",
            Tool::WriteFile(_) => "write_file",
            Tool::ListDir(_) => "list_dir",
            Tool::WebFetch(_) => "web_fetch",
            Tool::Memory(_) => "memory",
            Tool::MemorySearch(_) => "memory_search",
            Tool::MemoryGet(_) => "memory_get",
            Tool::Browse(_) => "browse",
            Tool::WebSearch(_) => "web_search",
            Tool::LinkReader(_) => "link_reader",
            Tool::MediaDescribe(_) => "media_describe",
            Tool::Cron(_) => "cron",
            Tool::Message(_) => "message",
            Tool::SessionsList(_) => "sessions_list",
            Tool::SessionsHistory(_) => "sessions_history",
            Tool::SessionsSend(_) => "sessions_send",
            Tool::SessionsSpawn(_) => "sessions_spawn",
            Tool::Subagents(_) => "subagents",
            Tool::SessionStatus(_) => "session_status",
            Tool::Tts(_) => "tts",
            Tool::Workspace(_) => "workspace",
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Tool::Bash(_) => "Execute a shell command and return the output",
            Tool::ReadFile(_) => "Read contents of a file",
            Tool::WriteFile(_) => "Write content to a file",
            Tool::ListDir(_) => "List contents of a directory",
            Tool::WebFetch(_) => "Fetch content from a URL via HTTP GET",
            Tool::Memory(_) => "Store and retrieve key-value memory entries",
            Tool::MemorySearch(_) => {
                "Mandatory recall step: semantically search memory before answering questions about prior work, decisions, dates, people, preferences, or todos"
            }
            Tool::MemoryGet(_) => "Read a specific memory entry by ID after memory_search",
            Tool::Browse(_) => {
                "Browser automation (alias: browser): open websites, navigate, click, type, screenshot, evaluate JS, get DOM snapshot, console logs, and network requests"
            }
            Tool::WebSearch(_) => {
                "Search the web and return a list of results with titles, URLs, and snippets"
            }
            Tool::LinkReader(_) => "Fetch a URL and extract its main text content",
            Tool::MediaDescribe(_) => "Describe an image from a URL using vision capabilities",
            Tool::Cron(_) => "Manage scheduled cron jobs: list, add, update, remove, run, status",
            Tool::Message(_) => {
                "Send/manage channel messages: send/reply, react/unreact/reactions, read/search, edit/delete/unsend, pin/unpin/list pins, permissions, attachments, thread create/reply, polls, member/group listing, broadcast"
            }
            Tool::SessionsList(_) => "List active sessions with optional filters",
            Tool::SessionsHistory(_) => "Retrieve message history for a session",
            Tool::SessionsSend(_) => "Send a message into an existing session",
            Tool::SessionsSpawn(_) => "Spawn a new sub-session with an agent",
            Tool::Subagents(_) => {
                "Manage subagents: help, agents, list, info, log, send, kill, steer, spawn, focus, unfocus"
            }
            Tool::SessionStatus(_) => "Get current session status and metadata",
            Tool::Tts(_) => "Convert text to speech audio",
            Tool::Workspace(_) => {
                "Read, write, append, or list files in the agent workspace (SOUL.md, IDENTITY.md, HEARTBEAT.md, memory/). Use this to evolve your personality and store durable memories."
            }
        }
    }

    pub fn parameters(&self) -> serde_json::Value {
        match self {
            Tool::Bash(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" }
                },
                "required": ["command"]
            }),
            Tool::ReadFile(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
            Tool::WriteFile(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
            Tool::ListDir(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
            Tool::WebFetch(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" }
                },
                "required": ["url"]
            }),
            Tool::Memory(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["get", "set", "delete", "list"] },
                    "key": { "type": "string" },
                    "value": { "type": "string" }
                },
                "required": ["action"]
            }),
            Tool::MemorySearch(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Semantic search query for memory recall" },
                    "max_results": { "type": "integer", "description": "Maximum results to return (default 5)" },
                    "min_score": { "type": "number", "description": "Minimum relevance score threshold (0.0-1.0, default 0.0)" }
                },
                "required": ["query"]
            }),
            Tool::MemoryGet(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Memory entry ID returned by memory_search" }
                },
                "required": ["id"]
            }),
            Tool::Browse(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["navigate", "click", "type", "screenshot", "evaluate", "snapshot", "console", "network", "back", "forward", "reload"], "description": "Browser action to perform (default: navigate)" },
                    "url": { "type": "string", "description": "URL to navigate to (for navigate action)" },
                    "selector": { "type": "string", "description": "CSS selector (for click/type actions)" },
                    "text": { "type": "string", "description": "Text to type (for type action)" },
                    "expression": { "type": "string", "description": "JavaScript expression (for evaluate action)" },
                    "wait_ms": { "type": "integer", "description": "Wait time in ms after action (default 1000)" }
                },
                "required": ["action"]
            }),
            Tool::WebSearch(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "max_results": { "type": "integer", "description": "Max results (default 5)" },
                    "provider": { "type": "string", "enum": ["auto", "brave", "perplexity", "duckduckgo"], "description": "Search provider (default: auto)" }
                },
                "required": ["query"]
            }),
            Tool::LinkReader(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to read" },
                    "max_chars": { "type": "integer", "description": "Max content chars (default 6000)" }
                },
                "required": ["url"]
            }),
            Tool::MediaDescribe(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Image URL to describe" },
                    "prompt": { "type": "string", "description": "What to describe (default: general description)" }
                },
                "required": ["url"]
            }),
            Tool::Cron(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "add", "update", "remove", "run", "status"], "description": "Cron action to perform" },
                    "job_id": { "type": "string", "description": "Job ID (for update/remove/run/status)" },
                    "schedule": { "type": "string", "description": "Cron expression (for add/update)" },
                    "command": { "type": "string", "description": "Command to execute (for add/update)" },
                    "label": { "type": "string", "description": "Human-readable label (for add/update)" },
                    "enabled": { "type": "boolean", "description": "Whether job is enabled (for add/update)" }
                },
                "required": ["action"]
            }),
            Tool::Message(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "Message action: send/reply/send_with_effect/react/unreact/reactions/read/search/edit/delete/unsend/pin/unpin/list_pins/permissions/send_attachment/sticker/sticker_search/sticker_upload/thread_create/thread_list/thread_reply/poll/channel_list/channel_info/channel_create/channel_edit/channel_delete/channel_move/channel_permission_set/channel_permission_remove/category_create/category_edit/category_delete/topic_create/member_info/add_participant/remove_participant/leave_group/role_info/role_add/role_remove/kick_member/ban_member/timeout_member/event_list/event_create/emoji_list/emoji_upload/voice_status/rename_group/set_group_icon/set_presence/broadcast (default: send)" },
                    "channel": { "type": "string", "description": "Delivery channel (telegram/feishu/slack/...) ; optional when current session has delivery context" },
                    "provider": { "type": "string", "description": "Alias for channel (Node-style)" },
                    "channels": { "type": "array", "items": { "type": "string" }, "description": "Target channels for broadcast (Node-style)" },
                    "target": { "type": "string", "description": "Target chat/user ID (preferred)" },
                    "to": { "type": "string", "description": "Alias for target (Node-style)" },
                    "targets": { "type": "array", "items": { "type": "string" }, "description": "Target list for broadcast" },
                    "text": { "type": "string", "description": "Message text to send" },
                    "message": { "type": "string", "description": "Alias for text (Node-style)" },
                    "content": { "type": "string", "description": "Alias for text" },
                    "reply_to": { "type": "string", "description": "Message ID to reply to" },
                    "replyTo": { "type": "string", "description": "Alias for reply_to (Node-style)" },
                    "thread_id": { "type": "string", "description": "Thread id for thread-style channels (or send reply anchor)" },
                    "threadId": { "type": "string", "description": "Alias for thread_id (Node-style)" },
                    "message_id": { "type": "string", "description": "Message id for reaction/edit/delete actions" },
                    "messageId": { "type": "string", "description": "Alias for message_id (Node-style)" },
                    "emoji": { "type": "string", "description": "Emoji for reaction actions" },
                    "remove": { "type": "boolean", "description": "When action=react and remove=true, convert to remove_reaction (Node-style)" },
                    "query": { "type": "string", "description": "Search query for action=search" },
                    "before": { "type": "string", "description": "Cursor/message id before for action=read" },
                    "after": { "type": "string", "description": "Cursor/message id after for action=read" },
                    "around": { "type": "string", "description": "Cursor/message id around for action=read" },
                    "name": { "type": "string", "description": "Generic name/title field for create/edit operations" },
                    "description": { "type": "string", "description": "Generic description field for create/upload operations" },
                    "user_id": { "type": "string", "description": "User/member id for moderation/role/participant operations" },
                    "userId": { "type": "string", "description": "Alias for user_id" },
                    "role_id": { "type": "string", "description": "Role id for role operations" },
                    "roleId": { "type": "string", "description": "Alias for role_id" },
                    "target_id": { "type": "string", "description": "Overwrite target id for channel_permission_set/remove" },
                    "targetId": { "type": "string", "description": "Alias for target_id" },
                    "target_type": { "type": "string", "description": "Overwrite target type: role/member for channel_permission_set" },
                    "targetType": { "type": "string", "description": "Alias for target_type" },
                    "allow": { "type": "string", "description": "Permission bitset allow mask for channel_permission_set" },
                    "deny": { "type": "string", "description": "Permission bitset deny mask for channel_permission_set" },
                    "parent_id": { "type": "string", "description": "Parent channel/category id" },
                    "parentId": { "type": "string", "description": "Alias for parent_id" },
                    "position": { "type": "integer", "description": "Position/order hint for move/edit operations" },
                    "topic": { "type": "string", "description": "Topic text for topic/channel edits" },
                    "channel_type": { "type": "string", "description": "Channel type hint for channel_create/category_create" },
                    "channelType": { "type": "string", "description": "Alias for channel_type" },
                    "start_time": { "type": "string", "description": "Event start time (RFC3339)" },
                    "startTime": { "type": "string", "description": "Alias for start_time" },
                    "end_time": { "type": "string", "description": "Event end time (RFC3339)" },
                    "endTime": { "type": "string", "description": "Alias for end_time" },
                    "duration_minutes": { "type": "integer", "description": "Timeout duration in minutes" },
                    "durationMinutes": { "type": "integer", "description": "Alias for duration_minutes" },
                    "delete_message_seconds": { "type": "integer", "description": "Ban cleanup window in seconds" },
                    "deleteMessageSeconds": { "type": "integer", "description": "Alias for delete_message_seconds" },
                    "tags": { "type": "string", "description": "Tags for sticker upload" },
                    "image": { "type": "string", "description": "Emoji/group icon payload (data URL / URL / file path)" },
                    "icon": { "type": "string", "description": "Group icon payload (data URL / URL / file path)" },
                    "effect": { "type": "string", "description": "Message effect name/id for sendWithEffect" },
                    "thread_name": { "type": "string", "description": "Thread title/name for thread_create" },
                    "threadName": { "type": "string", "description": "Alias for thread_name" },
                    "auto_archive_minutes": { "type": "integer", "description": "Thread auto-archive minutes for thread_create" },
                    "autoArchiveMinutes": { "type": "integer", "description": "Alias for auto_archive_minutes" },
                    "autoArchiveMin": { "type": "integer", "description": "Alias for auto_archive_minutes (Node-style)" },
                    "thread_type": { "type": "string", "description": "Optional thread type hint (e.g. thread_public/thread_private)" },
                    "threadType": { "type": "string", "description": "Alias for thread_type" },
                    "group_id": { "type": "string", "description": "Optional group/channel scope for member_info fallback/list_members" },
                    "groupId": { "type": "string", "description": "Alias for group_id" },
                    "guild_id": { "type": "string", "description": "Alias for group_id in guild-based channels" },
                    "guildId": { "type": "string", "description": "Alias for group_id in guild-based channels (Node-style)" },
                    "limit": { "type": "integer", "description": "Optional result limit for list actions" },
                    "include_archived": { "type": "boolean", "description": "Thread list: include archived threads for target channel" },
                    "includeArchived": { "type": "boolean", "description": "Alias for include_archived" },
                    "media": { "type": "string", "description": "Attachment source: URL, local path, data URL, or file id" },
                    "path": { "type": "string", "description": "Alias for media local path" },
                    "filePath": { "type": "string", "description": "Alias for media local path (Node-style)" },
                    "file_id": { "type": "string", "description": "Platform file id for attachment reuse" },
                    "fileId": { "type": "string", "description": "Alias for file_id (Node-style)" },
                    "buffer": { "type": "string", "description": "Attachment base64 or data URL payload" },
                    "filename": { "type": "string", "description": "Attachment filename hint" },
                    "mime_type": { "type": "string", "description": "Attachment MIME type hint" },
                    "mimeType": { "type": "string", "description": "Alias for mime_type (Node-style)" },
                    "contentType": { "type": "string", "description": "Alias for mime_type" },
                    "caption": { "type": "string", "description": "Attachment caption text" },
                    "as_voice": { "type": "boolean", "description": "Force voice media type for attachment" },
                    "asVoice": { "type": "boolean", "description": "Alias for as_voice" },
                    "poll_question": { "type": "string", "description": "Poll question text" },
                    "pollQuestion": { "type": "string", "description": "Alias for poll_question (Node-style)" },
                    "poll_options": { "type": "array", "items": { "type": "string" }, "description": "Poll options list (snake_case)" },
                    "pollOption": { "type": "array", "items": { "type": "string" }, "description": "Poll options list (Node-style alias)" },
                    "poll_anonymous": { "type": "boolean", "description": "Whether poll is anonymous (default true)" },
                    "pollAnonymous": { "type": "boolean", "description": "Alias for poll_anonymous" },
                    "poll_multiple": { "type": "boolean", "description": "Whether poll allows multiple answers (default false)" },
                    "pollMulti": { "type": "boolean", "description": "Alias for poll_multiple" },
                    "account_id": { "type": "string", "description": "Optional account ID for multi-account channels" },
                    "accountId": { "type": "string", "description": "Alias for account_id (Node-style)" }
                },
                "required": []
            }),
            Tool::SessionsList(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "channel": { "type": "string", "description": "Filter by channel" },
                    "limit": { "type": "integer", "description": "Max results (default 20)" },
                    "active_only": { "type": "boolean", "description": "Only show active sessions" }
                }
            }),
            Tool::SessionsHistory(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "session_key": { "type": "string", "description": "Session key to get history for" },
                    "limit": { "type": "integer", "description": "Max messages (default 50)" },
                    "before": { "type": "string", "description": "Cursor for pagination" }
                },
                "required": ["session_key"]
            }),
            Tool::SessionsSend(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "session_key": { "type": "string", "description": "Target session key (snake_case alias)" },
                    "sessionKey": { "type": "string", "description": "Target session key (Node-style alias)" },
                    "text": { "type": "string", "description": "Message text (snake_case alias)" },
                    "message": { "type": "string", "description": "Message text (Node-style alias)" },
                    "label": { "type": "string", "description": "Optional target label (resolved by orchestrator)" },
                    "agentId": { "type": "string", "description": "Optional agent scope when resolving label" },
                    "role": { "type": "string", "enum": ["user", "system"], "description": "Message role (default: user)" },
                    "timeoutSeconds": { "type": "number", "description": "Wait timeout in seconds (0 = async fire-and-forget)" }
                },
                "required": []
            }),
            Tool::SessionsSpawn(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "Task description for the spawned subagent" },
                    "label": { "type": "string", "description": "Optional display label for the spawned subagent" },
                    "agentId": { "type": "string", "description": "Requested agent id (Node-style alias)" },
                    "agent_id": { "type": "string", "description": "Requested agent id (snake_case alias)" },
                    "model": { "type": "string", "description": "Optional model override for this spawned run" },
                    "thinking": { "type": "string", "description": "Optional thinking mode hint" },
                    "runTimeoutSeconds": { "type": "number", "description": "Run timeout in seconds" },
                    "timeoutSeconds": { "type": "number", "description": "Back-compat timeout alias" },
                    "thread": { "type": "boolean", "description": "Request thread binding for subagent session" },
                    "mode": { "type": "string", "enum": ["run", "session"], "description": "Run mode: one-shot run or persistent session" },
                    "cleanup": { "type": "string", "enum": ["keep", "delete"], "description": "Cleanup policy after completion" },
                    "prompt": { "type": "string", "description": "Back-compat initial prompt alias" },
                    "parent_session_key": { "type": "string", "description": "Parent session key (back-compat)" }
                },
                "required": []
            }),
            Tool::Subagents(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["help", "agents", "list", "info", "log", "send", "kill", "steer", "spawn", "focus", "unfocus"],
                        "description": "Subagent action"
                    },
                    "target": { "type": "string", "description": "Target run/session/label/index ('all' supported for kill)" },
                    "session_key": { "type": "string", "description": "Back-compat target session key alias" },
                    "sessionKey": { "type": "string", "description": "Node-style target session key alias" },
                    "message": { "type": "string", "description": "Message for send/steer actions" },
                    "recentMinutes": { "type": "number", "description": "Recent window in minutes for list action" },
                    "limit": { "type": "number", "description": "History limit for log action (default 20, max 200)" },
                    "includeTools": { "type": "boolean", "description": "Include tool/toolresult rows in log output" },
                    "agentId": { "type": "string", "description": "Agent id for spawn action (Node-style)" },
                    "agent_id": { "type": "string", "description": "Agent id for spawn action (snake_case)" },
                    "task": { "type": "string", "description": "Task for spawn action" },
                    "prompt": { "type": "string", "description": "Back-compat task alias for spawn action" },
                    "model": { "type": "string", "description": "Model override for spawn action" },
                    "thinking": { "type": "string", "description": "Thinking mode hint for spawn action" },
                    "thread": { "type": "boolean", "description": "Request thread binding for spawn session mode" },
                    "mode": { "type": "string", "enum": ["run", "session"], "description": "Spawn mode" },
                    "cleanup": { "type": "string", "enum": ["keep", "delete"], "description": "Spawn cleanup policy" },
                    "runTimeoutSeconds": { "type": "number", "description": "Spawn run timeout seconds" },
                    "timeoutSeconds": { "type": "number", "description": "Back-compat timeout alias" },
                    "targetKind": { "type": "string", "enum": ["subagent", "acp"], "description": "Focus target kind override" },
                    "channel": { "type": "string", "description": "Focus binding channel" },
                    "to": { "type": "string", "description": "Focus binding destination (for example channel:123 / group:456)" },
                    "accountId": { "type": "string", "description": "Focus binding account id (Node-style)" },
                    "account_id": { "type": "string", "description": "Focus binding account id (snake_case)" },
                    "threadId": { "type": "string", "description": "Focus binding thread id (Node-style)" },
                    "thread_id": { "type": "string", "description": "Focus binding thread id (snake_case)" },
                    "parentConversationId": { "type": "string", "description": "Focus binding parent conversation id (Node-style)" },
                    "parent_conversation_id": { "type": "string", "description": "Focus binding parent conversation id (snake_case)" },
                    "bindingId": { "type": "string", "description": "Specific binding id for unfocus (Node-style)" },
                    "binding_id": { "type": "string", "description": "Specific binding id for unfocus (snake_case)" }
                },
                "required": ["action"]
            }),
            Tool::SessionStatus(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "session_key": { "type": "string", "description": "Session key (snake_case alias)" },
                    "sessionKey": { "type": "string", "description": "Session key (Node-style alias)" },
                    "model": { "type": "string", "description": "Optional per-session model override; 'default' resets override" }
                }
            }),
            Tool::Tts(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Text to convert to speech" },
                    "provider": { "type": "string", "enum": ["openai", "elevenlabs", "edge"], "description": "TTS provider (default: auto)" },
                    "voice": { "type": "string", "description": "Voice name/ID" },
                    "model": { "type": "string", "description": "Model name (provider-specific)" }
                },
                "required": ["text"]
            }),
            Tool::Workspace(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["read", "write", "append", "list"], "description": "Action to perform on workspace files" },
                    "path": { "type": "string", "description": "Relative path within workspace (e.g. SOUL.md, IDENTITY.md, memory/2026-02-25.md)" },
                    "content": { "type": "string", "description": "Content to write or append (for write/append actions)" }
                },
                "required": ["action"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        match self {
            Tool::Bash(tool) => tool.execute(arguments).await,
            Tool::ReadFile(tool) => tool.execute(arguments).await,
            Tool::WriteFile(tool) => tool.execute(arguments).await,
            Tool::ListDir(tool) => tool.execute(arguments).await,
            Tool::WebFetch(tool) => tool.execute(arguments).await,
            Tool::Memory(tool) => tool.execute(arguments).await,
            Tool::MemorySearch(tool) => tool.execute(arguments).await,
            Tool::MemoryGet(tool) => tool.execute(arguments).await,
            Tool::Browse(tool) => tool.execute(arguments).await,
            Tool::WebSearch(tool) => tool.execute(arguments).await,
            Tool::LinkReader(tool) => tool.execute(arguments).await,
            Tool::MediaDescribe(tool) => tool.execute(arguments).await,
            Tool::Cron(tool) => tool.execute(arguments).await,
            Tool::Message(tool) => tool.execute(arguments).await,
            Tool::SessionsList(tool) => tool.execute(arguments).await,
            Tool::SessionsHistory(tool) => tool.execute(arguments).await,
            Tool::SessionsSend(tool) => tool.execute(arguments).await,
            Tool::SessionsSpawn(tool) => tool.execute(arguments).await,
            Tool::Subagents(tool) => tool.execute(arguments).await,
            Tool::SessionStatus(tool) => tool.execute(arguments).await,
            Tool::Tts(tool) => tool.execute(arguments).await,
            Tool::Workspace(tool) => tool.execute(arguments).await,
        }
    }
}

impl BashTool {
    pub fn new() -> Self {
        Self {
            timeout_seconds: 30,
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        use std::collections::HashMap;
        use std::process::Stdio;
        use tokio::process::Command;
        use tokio::time::{Duration, timeout};

        #[derive(Deserialize)]
        struct BashArguments {
            command: String,
            #[serde(default)]
            timeout_seconds: Option<u64>,
            #[serde(default)]
            working_dir: Option<String>,
            #[serde(default)]
            env: Option<HashMap<String, String>>,
        }

        let args: BashArguments = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let timeout_secs = args.timeout_seconds.unwrap_or(self.timeout_seconds);
        let timeout_dur = Duration::from_secs(timeout_secs);

        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", &args.command]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", &args.command]);
            c
        };

        if let Some(dir) = args.working_dir {
            cmd.current_dir(dir);
        }

        if let Some(env_vars) = args.env {
            // Block dangerous environment variables that could hijack execution
            const BLOCKED_ENV: &[&str] = &[
                "LD_PRELOAD",
                "LD_LIBRARY_PATH",
                "DYLD_INSERT_LIBRARIES",
                "DYLD_LIBRARY_PATH",
                "DYLD_FRAMEWORK_PATH",
                "PATH",
                "HOME",
                "SHELL",
                "USER",
                "LOGNAME",
                "BASH_ENV",
                "ENV",
                "CDPATH",
                "GLOBIGNORE",
                "BASH_FUNC_",
                "PS4",
                "PROMPT_COMMAND",
                "PYTHONSTARTUP",
                "PERL5OPT",
                "RUBYOPT",
                "NODE_OPTIONS",
                "JAVA_TOOL_OPTIONS",
                "_JAVA_OPTIONS",
                "CLASSPATH",
                "GIT_SSH_COMMAND",
                "http_proxy",
                "https_proxy",
                "CURL_CA_BUNDLE",
            ];
            for (key, value) in env_vars {
                let key_upper = key.to_uppercase();
                let is_blocked = BLOCKED_ENV.iter().any(|b| {
                    key_upper == *b
                        || key_upper.starts_with("LD_")
                        || key_upper.starts_with("DYLD_")
                });
                if is_blocked {
                    return Err(crate::ToolError::InvalidInput(format!(
                        "Environment variable '{}' is blocked for security",
                        key
                    )));
                }
                cmd.env(&key, &value);
            }
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let result = timeout(timeout_dur, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                #[derive(Serialize)]
                struct BashOutput {
                    stdout: String,
                    stderr: String,
                    exit_code: Option<i32>,
                    timed_out: bool,
                }
                let output = BashOutput {
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    exit_code: output.status.code(),
                    timed_out: false,
                };
                Ok(serde_json::to_value(output)?)
            }
            Ok(Err(e)) => Err(crate::ToolError::ExecutionFailed(e.to_string())),
            Err(_) => {
                #[derive(Serialize)]
                struct BashOutput {
                    stdout: String,
                    stderr: String,
                    exit_code: Option<i32>,
                    timed_out: bool,
                }
                let output = BashOutput {
                    stdout: String::new(),
                    stderr: "Command timed out".to_string(),
                    exit_code: None,
                    timed_out: true,
                };
                Ok(serde_json::to_value(output)?)
            }
        }
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ToolRegistry {
    tools: HashMap<String, Tool>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut tools = HashMap::new();
        tools.insert("bash".to_string(), Tool::Bash(BashTool::new()));
        tools.insert("read_file".to_string(), Tool::ReadFile(ReadFileTool::new()));
        tools.insert(
            "write_file".to_string(),
            Tool::WriteFile(WriteFileTool::new()),
        );
        tools.insert("list_dir".to_string(), Tool::ListDir(ListDirTool::new()));
        tools.insert("web_fetch".to_string(), Tool::WebFetch(WebFetchTool::new()));
        tools.insert("memory".to_string(), Tool::Memory(MemoryTool::new()));
        tools.insert(
            "memory_search".to_string(),
            Tool::MemorySearch(MemorySearchTool::new()),
        );
        tools.insert(
            "memory_get".to_string(),
            Tool::MemoryGet(MemoryGetTool::new()),
        );
        tools.insert("browse".to_string(), Tool::Browse(BrowseTool::new()));
        tools.insert(
            "web_search".to_string(),
            Tool::WebSearch(WebSearchTool::new()),
        );
        tools.insert(
            "link_reader".to_string(),
            Tool::LinkReader(LinkReaderTool::new()),
        );
        tools.insert(
            "media_describe".to_string(),
            Tool::MediaDescribe(MediaDescribeTool::new()),
        );
        tools.insert("cron".to_string(), Tool::Cron(CronTool::new()));
        tools.insert("message".to_string(), Tool::Message(MessageTool::new()));
        tools.insert(
            "sessions_list".to_string(),
            Tool::SessionsList(SessionsListTool::new()),
        );
        tools.insert(
            "sessions_history".to_string(),
            Tool::SessionsHistory(SessionsHistoryTool::new()),
        );
        tools.insert(
            "sessions_send".to_string(),
            Tool::SessionsSend(SessionsSendTool::new()),
        );
        tools.insert(
            "sessions_spawn".to_string(),
            Tool::SessionsSpawn(SessionsSpawnTool::new()),
        );
        tools.insert(
            "subagents".to_string(),
            Tool::Subagents(SubagentsTool::new()),
        );
        tools.insert(
            "session_status".to_string(),
            Tool::SessionStatus(SessionStatusTool::new()),
        );
        tools.insert("tts".to_string(), Tool::Tts(TtsTool::new()));
        Self { tools }
    }

    pub fn configure_browser(
        &mut self,
        cdp_url: Option<&str>,
        executable_path: Option<&str>,
        headless: Option<bool>,
        foreground: Option<bool>,
    ) {
        let mut browse = BrowseTool::new();
        if let Some(url) = cdp_url {
            browse.cdp_url = Some(url.to_string());
        }
        if let Some(exe) = executable_path {
            browse.executable_path = Some(exe.to_string());
        }
        if let Some(h) = headless {
            browse.headless = Some(h);
        }
        if let Some(fg) = foreground {
            browse.foreground = Some(fg);
        }
        self.tools
            .insert("browse".to_string(), Tool::Browse(browse));
    }

    pub fn configure_workspace(&mut self, workspace_root: &str) {
        self.tools.insert(
            "workspace".to_string(),
            Tool::Workspace(WorkspaceTool::new(workspace_root)),
        );
    }

    /// Inject a MemoryManager into the memory_search and memory_get tools,
    /// replacing the default no-op instances.
    pub fn with_memory_manager(
        &mut self,
        manager: std::sync::Arc<oclaw_memory_core::MemoryManager>,
    ) {
        self.tools.insert(
            "memory_search".to_string(),
            Tool::MemorySearch(MemorySearchTool::with_manager(manager.clone())),
        );
        self.tools.insert(
            "memory_get".to_string(),
            Tool::MemoryGet(MemoryGetTool::with_manager(manager)),
        );
    }

    pub fn register(&mut self, tool: Tool) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&Tool> {
        self.tools.get(name)
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn list(&self) -> Vec<serde_json::Value> {
        self.tools
            .values()
            .map(|t| {
                serde_json::json!({
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.parameters(),
                })
            })
            .collect()
    }

    /// Return only essential tools for LLM function calling (reduces token usage).
    pub fn list_for_llm(&self) -> Vec<serde_json::Value> {
        let essential = [
            "bash",
            "web_fetch",
            "web_search",
            "browse",
            "memory_search",
            "memory_get",
            "link_reader",
            "media_describe",
            "cron",
            "message",
            "sessions_list",
            "sessions_history",
            "sessions_send",
            "sessions_spawn",
            "subagents",
            "session_status",
            "tts",
            "workspace",
        ];
        self.tools
            .values()
            .filter(|t| essential.contains(&t.name()))
            .map(|t| {
                serde_json::json!({
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.parameters(),
                })
            })
            .collect()
    }

    pub async fn execute_call(&self, call: ToolCall) -> ToolResponse {
        let tool = self.get(&call.name);

        match tool {
            Some(t) => {
                let result: ToolResult<serde_json::Value> = t.execute(call.arguments).await;
                match result {
                    Ok(result) => ToolResponse {
                        id: call.id,
                        result,
                        error: None,
                    },
                    Err(e) => ToolResponse {
                        id: call.id,
                        result: serde_json::Value::Null,
                        error: Some(e.to_string()),
                    },
                }
            }
            None => ToolResponse {
                id: call.id,
                result: serde_json::Value::Null,
                error: Some(format!("Tool not found: {}", call.name)),
            },
        }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileTool {
    pub max_size_bytes: Option<u64>,
}

impl ReadFileTool {
    pub fn new() -> Self {
        Self {
            max_size_bytes: Some(1024 * 1024),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct ReadFileArgs {
            path: String,
        }

        let args: ReadFileArgs = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let content = tokio::fs::read_to_string(&args.path).await.map_err(|e| {
            crate::ToolError::ExecutionFailed(format!("Failed to read file: {}", e))
        })?;

        if let Some(max_size) = self.max_size_bytes
            && content.len() > max_size as usize
        {
            return Err(crate::ToolError::ExecutionFailed(
                "File too large".to_string(),
            ));
        }

        Ok(serde_json::json!({
            "path": args.path,
            "content": content,
            "size": content.len()
        }))
    }
}

impl Default for ReadFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFileTool {
    pub create_parents: bool,
}

impl WriteFileTool {
    pub fn new() -> Self {
        Self {
            create_parents: true,
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct WriteFileArgs {
            path: String,
            content: String,
        }

        let args: WriteFileArgs = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        if self.create_parents
            && let Some(parent) = std::path::Path::new(&args.path).parent()
        {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                crate::ToolError::ExecutionFailed(format!("Failed to create directory: {}", e))
            })?;
        }

        tokio::fs::write(&args.path, &args.content)
            .await
            .map_err(|e| {
                crate::ToolError::ExecutionFailed(format!("Failed to write file: {}", e))
            })?;

        Ok(serde_json::json!({
            "path": args.path,
            "bytes_written": args.content.len()
        }))
    }
}

impl Default for WriteFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDirTool {
    pub include_hidden: bool,
}

impl ListDirTool {
    pub fn new() -> Self {
        Self {
            include_hidden: true,
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct ListDirArgs {
            path: String,
        }

        let args: ListDirArgs = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&args.path).await.map_err(|e| {
            crate::ToolError::ExecutionFailed(format!("Failed to read directory: {}", e))
        })?;

        while let Some(entry) = dir.next_entry().await.map_err(|e| {
            crate::ToolError::ExecutionFailed(format!("Failed to read entry: {}", e))
        })? {
            let file_name = entry.file_name().to_string_lossy().to_string();

            if !self.include_hidden && file_name.starts_with('.') {
                continue;
            }

            let metadata = entry.metadata().await.map_err(|e| {
                crate::ToolError::ExecutionFailed(format!("Failed to get metadata: {}", e))
            })?;

            entries.push(serde_json::json!({
                "name": file_name,
                "is_file": metadata.is_file(),
                "is_dir": metadata.is_dir(),
                "size": metadata.len(),
            }));
        }

        Ok(serde_json::json!({
            "path": args.path,
            "entries": entries,
            "count": entries.len()
        }))
    }
}

impl Default for ListDirTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchTool {
    pub timeout_seconds: u64,
    pub max_body_bytes: usize,
}

impl WebFetchTool {
    pub fn new() -> Self {
        Self {
            timeout_seconds: 30,
            max_body_bytes: 2 * 1024 * 1024,
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            url: String,
            #[serde(default)]
            headers: Option<HashMap<String, String>>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        // SSRF guard: block private IPs unless explicitly allowed
        if !Self::is_url_allowed(&args.url) {
            return Err(crate::ToolError::ExecutionFailed(
                "URL targets a private/localhost address (SSRF blocked)".into(),
            ));
        }

        // Use Firecrawl if API key is available
        if let Ok(fc_key) = std::env::var("FIRECRAWL_API_KEY") {
            return self.fetch_firecrawl(&args.url, &fc_key).await;
        }

        // Fallback: direct HTTP fetch
        self.fetch_direct(&args.url, args.headers).await
    }

    fn is_url_allowed(url: &str) -> bool {
        let parsed = match url::Url::parse(url) {
            Ok(u) => u,
            Err(_) => return false,
        };

        // Only allow http/https schemes
        match parsed.scheme() {
            "http" | "https" => {}
            _ => return false,
        }

        let host = match parsed.host_str() {
            Some(h) => h,
            None => return false,
        };

        let blocked = ["localhost", "127.0.0.1", "0.0.0.0", "[::1]", "::1"];
        if blocked.contains(&host) {
            return std::env::var("OCLAWS_ALLOW_PRIVATE_FETCH").is_ok();
        }

        // Block private IP ranges (RFC 1918 + link-local + IPv6 specials)
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            let is_private = match ip {
                std::net::IpAddr::V4(v4) => {
                    v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
                }
                std::net::IpAddr::V6(v6) => {
                    let seg = v6.segments();
                    v6.is_loopback()
                        || v6.is_unspecified()
                        // Link-local fe80::/10
                        || (seg[0] & 0xffc0) == 0xfe80
                        // Unique-local fc00::/7
                        || (seg[0] & 0xfe00) == 0xfc00
                        // Multicast ff00::/8
                        || (seg[0] & 0xff00) == 0xff00
                        // IPv4-mapped ::ffff:0:0/96 — check the mapped v4 address
                        || matches!(v6.to_ipv4_mapped(), Some(v4) if
                            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified())
                }
            };
            if is_private {
                return std::env::var("OCLAWS_ALLOW_PRIVATE_FETCH").is_ok();
            }
        }

        true
    }

    async fn fetch_direct(
        &self,
        url: &str,
        headers: Option<HashMap<String, String>>,
    ) -> ToolResult<serde_json::Value> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_seconds))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let mut req = client.get(url);
        if let Some(hdrs) = headers {
            for (k, v) in hdrs {
                req = req.header(&k, &v);
            }
        }

        let resp = req
            .send()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let max_chars = 50_000;
        let truncated = body.len() > max_chars;
        let body = if truncated { &body[..max_chars] } else { &body };

        Ok(serde_json::json!({
            "url": url,
            "status": status,
            "body": body,
            "truncated": truncated,
            "backend": "direct"
        }))
    }

    async fn fetch_firecrawl(&self, url: &str, api_key: &str) -> ToolResult<serde_json::Value> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let body = serde_json::json!({
            "url": url,
            "formats": ["markdown"],
            "onlyMainContent": true
        });

        let resp = client
            .post("https://api.firecrawl.dev/v1/scrape")
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                crate::ToolError::ExecutionFailed(format!("Firecrawl request failed: {}", e))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(crate::ToolError::ExecutionFailed(format!(
                "Firecrawl error ({}): {}",
                status, text
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let markdown = json["data"]["markdown"].as_str().unwrap_or("").to_string();

        let max_chars = 50_000;
        let truncated = markdown.len() > max_chars;
        let content = if truncated {
            &markdown[..max_chars]
        } else {
            &markdown
        };

        Ok(serde_json::json!({
            "url": url,
            "body": content,
            "truncated": truncated,
            "backend": "firecrawl"
        }))
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryTool {
    #[serde(skip)]
    store: std::sync::Arc<std::sync::Mutex<HashMap<String, String>>>,
}

impl MemoryTool {
    pub fn new() -> Self {
        Self {
            store: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            action: String,
            key: Option<String>,
            value: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let mut store = self.store.lock().unwrap();
        match args.action.as_str() {
            "get" => {
                let key = args
                    .key
                    .ok_or_else(|| crate::ToolError::InvalidInput("key required".into()))?;
                let val = store.get(&key).cloned();
                Ok(serde_json::json!({ "key": key, "value": val }))
            }
            "set" => {
                let key = args
                    .key
                    .ok_or_else(|| crate::ToolError::InvalidInput("key required".into()))?;
                let val = args.value.unwrap_or_default();
                store.insert(key.clone(), val.clone());
                Ok(serde_json::json!({ "key": key, "value": val }))
            }
            "delete" => {
                let key = args
                    .key
                    .ok_or_else(|| crate::ToolError::InvalidInput("key required".into()))?;
                let removed = store.remove(&key);
                Ok(serde_json::json!({ "key": key, "removed": removed.is_some() }))
            }
            "list" => {
                let keys: Vec<&String> = store.keys().collect();
                Ok(serde_json::json!({ "keys": keys, "count": keys.len() }))
            }
            other => Err(crate::ToolError::InvalidInput(format!(
                "Unknown action: {}",
                other
            ))),
        }
    }
}

impl Default for MemoryTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Semantic memory search tool — aligned with Node's `memory_search`.
/// Holds an optional MemoryManager; without one, returns empty results.
#[derive(Clone, Serialize, Deserialize)]
pub struct MemorySearchTool {
    #[serde(skip)]
    manager: Option<std::sync::Arc<oclaw_memory_core::MemoryManager>>,
}

impl std::fmt::Debug for MemorySearchTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemorySearchTool")
            .field("has_manager", &self.manager.is_some())
            .finish()
    }
}

impl MemorySearchTool {
    pub fn new() -> Self {
        Self { manager: None }
    }

    pub fn with_manager(manager: std::sync::Arc<oclaw_memory_core::MemoryManager>) -> Self {
        Self {
            manager: Some(manager),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            query: String,
            max_results: Option<usize>,
            min_score: Option<f64>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let Some(ref mm) = self.manager else {
            return Ok(
                serde_json::json!({ "results": [], "note": "memory manager not configured" }),
            );
        };

        let results = mm
            .search(&args.query)
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let min_score = args.min_score.unwrap_or(0.0);
        let max_results = args.max_results.unwrap_or(5);

        let filtered: Vec<serde_json::Value> = results
            .into_iter()
            .filter(|r| r.score >= min_score)
            .take(max_results)
            .map(|r| {
                serde_json::json!({
                    "path": r.path,
                    "snippet": r.snippet,
                    "score": r.score,
                    "source": r.source,
                })
            })
            .collect();

        Ok(serde_json::json!({ "results": filtered }))
    }
}

impl Default for MemorySearchTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Retrieve a specific memory entry by ID — aligned with Node's `memory_get`.
#[derive(Clone, Serialize, Deserialize)]
pub struct MemoryGetTool {
    #[serde(skip)]
    manager: Option<std::sync::Arc<oclaw_memory_core::MemoryManager>>,
}

impl std::fmt::Debug for MemoryGetTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryGetTool")
            .field("has_manager", &self.manager.is_some())
            .finish()
    }
}

impl MemoryGetTool {
    pub fn new() -> Self {
        Self { manager: None }
    }

    pub fn with_manager(manager: std::sync::Arc<oclaw_memory_core::MemoryManager>) -> Self {
        Self {
            manager: Some(manager),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            id: String,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let Some(ref mm) = self.manager else {
            return Err(crate::ToolError::ExecutionFailed(
                "memory manager not configured".into(),
            ));
        };

        match mm.get_memory(&args.id) {
            Ok(Some(chunk)) => Ok(serde_json::json!({
                "id": chunk.id,
                "path": chunk.path,
                "content": chunk.content,
                "source": chunk.source,
                "updated_at_ms": chunk.updated_at_ms,
            })),
            Ok(None) => Ok(serde_json::json!({ "error": "not_found", "id": args.id })),
            Err(e) => Err(crate::ToolError::ExecutionFailed(e.to_string())),
        }
    }
}

impl Default for MemoryGetTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseTool {
    pub cdp_url: Option<String>,
    pub executable_path: Option<String>,
    pub headless: Option<bool>,
    /// Whether to launch a visible foreground browser window when auto-launching.
    pub foreground: Option<bool>,
    pub timeout_seconds: u64,
    /// Track a child browser process we launched (PID).
    #[serde(skip)]
    launched_pid: std::sync::Arc<std::sync::Mutex<Option<u32>>>,
    /// Page state tracking (console, errors, network).
    #[serde(skip)]
    state: std::sync::Arc<std::sync::Mutex<oclaw_browser_core::PageState>>,
}

impl BrowseTool {
    pub fn new() -> Self {
        Self {
            cdp_url: None,
            executable_path: None,
            headless: None,
            foreground: None,
            timeout_seconds: 30,
            launched_pid: Default::default(),
            state: Default::default(),
        }
    }

    pub fn with_cdp_url(mut self, url: &str) -> Self {
        self.cdp_url = Some(url.to_string());
        self
    }

    pub fn with_executable(mut self, path: &str) -> Self {
        self.executable_path = Some(path.to_string());
        self
    }

    pub fn with_headless(mut self, headless: bool) -> Self {
        self.headless = Some(headless);
        self
    }

    pub fn with_foreground(mut self, foreground: bool) -> Self {
        self.foreground = Some(foreground);
        self
    }

    /// Detect browser executable: config > Edge > Chrome
    fn detect_browser(&self) -> Option<String> {
        if let Some(ref p) = self.executable_path
            && std::path::Path::new(p).exists()
        {
            return Some(p.clone());
        }

        let candidates = if cfg!(windows) {
            vec![
                r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
                r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
                r"C:\Program Files\Google\Chrome\Application\chrome.exe",
                r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            ]
        } else if cfg!(target_os = "macos") {
            vec![
                "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            ]
        } else {
            vec![
                "/usr/bin/microsoft-edge",
                "/usr/bin/microsoft-edge-stable",
                "/usr/bin/google-chrome",
                "/usr/bin/google-chrome-stable",
                "/usr/bin/chromium",
                "/usr/bin/chromium-browser",
            ]
        };

        candidates
            .into_iter()
            .find(|p| std::path::Path::new(p).exists())
            .map(|s| s.to_string())
    }

    /// Try connecting to CDP; if fails, auto-launch browser then retry.
    async fn ensure_browser(&self) -> Result<oclaw_browser_core::BrowserManager, crate::ToolError> {
        let cdp_url = self.cdp_url.as_deref().unwrap_or("http://127.0.0.1:9222");

        // Try connecting first
        if let Ok(mgr) = oclaw_browser_core::BrowserManager::new(cdp_url).await {
            return Ok(mgr);
        }

        // Auto-launch browser
        let exe = self.detect_browser().ok_or_else(|| {
            crate::ToolError::ExecutionFailed(
                "No browser found. Install Edge or Chrome, or set browser.executablePath in config.".into()
            )
        })?;

        tracing::info!("Auto-launching browser: {}", exe);

        let port = cdp_url
            .split(':')
            .next_back()
            .and_then(|s| s.trim_matches('/').parse::<u16>().ok())
            .unwrap_or(9222);

        let mut cmd = tokio::process::Command::new(&exe);
        cmd.arg(format!("--remote-debugging-port={}", port));
        let headless = self.headless.unwrap_or(false);
        let foreground = self.foreground.unwrap_or(!headless);
        if headless {
            cmd.arg("--headless=new");
        } else if foreground {
            // Force a visible window for interactive debugging.
            cmd.arg("--new-window");
        }
        cmd.arg("--no-first-run");
        cmd.arg("--no-default-browser-check");
        cmd.arg("--disable-gpu");
        cmd.arg("--disable-sync");
        cmd.arg("--disable-background-networking");
        cmd.arg("--disable-component-update");
        cmd.arg("--disable-session-crashed-bubble");
        cmd.arg("--hide-crash-restore-bubble");
        let user_data_dir = std::env::temp_dir().join("oclaw-browser-profile");
        cmd.arg(format!("--user-data-dir={}", user_data_dir.display()));
        cmd.arg("about:blank");

        #[cfg(windows)]
        if !foreground {
            cmd.creation_flags(0x00000008); // DETACHED_PROCESS
        }
        let child = cmd
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| {
                crate::ToolError::ExecutionFailed(format!("Failed to launch browser: {}", e))
            })?;

        if let Some(pid) = child.id() {
            *self.launched_pid.lock().unwrap() = Some(pid);
        }

        // Wait for CDP to become available
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            if let Ok(mgr) = oclaw_browser_core::BrowserManager::new(cdp_url).await {
                return Ok(mgr);
            }
        }

        Err(crate::ToolError::ExecutionFailed(format!(
            "Browser launched but CDP not available at {} after 6s",
            cdp_url
        )))
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(default = "default_action")]
            action: String,
            #[serde(default)]
            url: Option<String>,
            #[serde(default)]
            selector: Option<String>,
            #[serde(default)]
            text: Option<String>,
            #[serde(default)]
            expression: Option<String>,
            #[serde(default)]
            wait_ms: Option<u64>,
        }
        fn default_action() -> String {
            "navigate".into()
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let mut manager = self.ensure_browser().await?;
        let mut page = manager.create_page().await.map_err(|e| {
            crate::ToolError::ExecutionFailed(format!("Failed to create page: {}", e))
        })?;

        let wait = args.wait_ms.unwrap_or(1000);
        let mut state = self.state.lock().unwrap().clone();

        let result = match args.action.as_str() {
            "navigate" => {
                let url = args.url.as_deref().ok_or_else(|| {
                    crate::ToolError::InvalidInput("url required for navigate".into())
                })?;
                page.navigate(url).await.map_err(|e| {
                    crate::ToolError::ExecutionFailed(format!("Navigation failed: {}", e))
                })?;
                tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                state.url = Some(url.to_string());
                let title = eval_string(&page, "document.title").await;
                state.title = Some(title.clone());
                let text = eval_string(&page, "document.body.innerText").await;
                let max_len = 8000;
                let truncated = text.len() > max_len;
                let content = if truncated { &text[..max_len] } else { &text };
                serde_json::json!({ "action": "navigate", "url": url, "title": title, "content": content, "truncated": truncated })
            }
            "click" => {
                let sel = args.selector.as_deref().ok_or_else(|| {
                    crate::ToolError::InvalidInput("selector required for click".into())
                })?;
                page.click_element(sel).await.map_err(|e| {
                    crate::ToolError::ExecutionFailed(format!("Click failed: {}", e))
                })?;
                tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                serde_json::json!({ "action": "click", "selector": sel, "ok": true })
            }
            "type" => {
                let sel = args.selector.as_deref().ok_or_else(|| {
                    crate::ToolError::InvalidInput("selector required for type".into())
                })?;
                let text = args.text.as_deref().ok_or_else(|| {
                    crate::ToolError::InvalidInput("text required for type".into())
                })?;
                page.type_text(sel, text).await.map_err(|e| {
                    crate::ToolError::ExecutionFailed(format!("Type failed: {}", e))
                })?;
                serde_json::json!({ "action": "type", "selector": sel, "ok": true })
            }
            "screenshot" => {
                let bytes = page.take_screenshot().await.map_err(|e| {
                    crate::ToolError::ExecutionFailed(format!("Screenshot failed: {}", e))
                })?;
                use base64::Engine;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                serde_json::json!({ "action": "screenshot", "base64": b64, "size_bytes": bytes.len() })
            }
            "evaluate" => {
                let expr = args.expression.as_deref().ok_or_else(|| {
                    crate::ToolError::InvalidInput("expression required for evaluate".into())
                })?;
                let result = page.evaluate(expr).await.map_err(|e| {
                    crate::ToolError::ExecutionFailed(format!("Evaluate failed: {}", e))
                })?;
                serde_json::json!({ "action": "evaluate", "result": result.value })
            }
            "snapshot" => {
                let html = page.get_html().await.unwrap_or_default();
                let title = eval_string(&page, "document.title").await;
                let url = eval_string(&page, "window.location.href").await;
                let max_len = 12000;
                let truncated = html.len() > max_len;
                let content = if truncated { &html[..max_len] } else { &html };
                serde_json::json!({ "action": "snapshot", "url": url, "title": title, "html": content, "html_length": html.len(), "truncated": truncated })
            }
            "console" => {
                let entries: Vec<_> = state.recent_console(50).into_iter().cloned().collect();
                serde_json::json!({ "action": "console", "entries": entries, "count": entries.len() })
            }
            "network" => {
                let entries: Vec<_> = state.recent_requests(50).into_iter().cloned().collect();
                serde_json::json!({ "action": "network", "entries": entries, "count": entries.len() })
            }
            "back" => {
                page.go_back().await.map_err(|e| {
                    crate::ToolError::ExecutionFailed(format!("Back failed: {}", e))
                })?;
                tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                serde_json::json!({ "action": "back", "ok": true })
            }
            "forward" => {
                page.go_forward().await.map_err(|e| {
                    crate::ToolError::ExecutionFailed(format!("Forward failed: {}", e))
                })?;
                tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                serde_json::json!({ "action": "forward", "ok": true })
            }
            "reload" => {
                page.reload().await.map_err(|e| {
                    crate::ToolError::ExecutionFailed(format!("Reload failed: {}", e))
                })?;
                tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                serde_json::json!({ "action": "reload", "ok": true })
            }
            other => {
                return Err(crate::ToolError::InvalidInput(format!(
                    "Unknown action: {}",
                    other
                )));
            }
        };

        *self.state.lock().unwrap() = state;
        page.close().await.ok();
        manager.disconnect().await.ok();

        Ok(result)
    }
}

async fn eval_string(page: &oclaw_browser_core::Page, expr: &str) -> String {
    page.evaluate(expr)
        .await
        .ok()
        .and_then(|r| r.value)
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

impl Default for BrowseTool {
    fn default() -> Self {
        Self::new()
    }
}

// --- WebSearchTool: DuckDuckGo HTML scraping (no API key needed) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchTool {
    pub timeout_seconds: u64,
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self {
            timeout_seconds: 15,
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            query: String,
            #[serde(default)]
            max_results: Option<usize>,
            #[serde(default)]
            provider: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;
        let max = args.max_results.unwrap_or(5).min(10);

        let provider = args.provider.as_deref().unwrap_or("auto");
        match provider {
            "brave" => self.search_brave(&args.query, max).await,
            "perplexity" => self.search_perplexity(&args.query).await,
            _ => {
                // Auto: try Brave if key exists, else DuckDuckGo
                if std::env::var("BRAVE_API_KEY").is_ok() {
                    self.search_brave(&args.query, max).await
                } else {
                    self.search_ddg(&args.query, max).await
                }
            }
        }
    }

    async fn search_ddg(&self, query: &str, max: usize) -> ToolResult<serde_json::Value> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_seconds))
            .build()
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let resp = client
            .get("https://html.duckduckgo.com/html/")
            .query(&[("q", query)])
            .header("User-Agent", "Mozilla/5.0 (compatible; oclaw/1.0)")
            .send()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Search failed: {}", e)))?;

        let html = resp
            .text()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let results = parse_ddg_results(&html, max);
        Ok(serde_json::json!({ "query": query, "provider": "duckduckgo", "results": results }))
    }

    async fn search_brave(&self, query: &str, max: usize) -> ToolResult<serde_json::Value> {
        let api_key = std::env::var("BRAVE_API_KEY")
            .map_err(|_| crate::ToolError::ExecutionFailed("BRAVE_API_KEY not set".into()))?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_seconds))
            .build()
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let resp = client
            .get("https://api.search.brave.com/res/v1/web/search")
            .query(&[("q", query), ("count", &max.to_string())])
            .header("X-Subscription-Token", &api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| {
                crate::ToolError::ExecutionFailed(format!("Brave search failed: {}", e))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(crate::ToolError::ExecutionFailed(format!(
                "Brave API error ({}): {}",
                status, body
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let results: Vec<serde_json::Value> = json["web"]["results"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .take(max)
                    .map(|r| {
                        serde_json::json!({
                            "title": r["title"].as_str().unwrap_or(""),
                            "url": r["url"].as_str().unwrap_or(""),
                            "snippet": r["description"].as_str().unwrap_or(""),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(serde_json::json!({
            "query": query,
            "provider": "brave",
            "results": results
        }))
    }

    async fn search_perplexity(&self, query: &str) -> ToolResult<serde_json::Value> {
        let api_key = std::env::var("PERPLEXITY_API_KEY")
            .map_err(|_| crate::ToolError::ExecutionFailed("PERPLEXITY_API_KEY not set".into()))?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let body = serde_json::json!({
            "model": "sonar",
            "messages": [{"role": "user", "content": query}]
        });

        let resp = client
            .post("https://api.perplexity.ai/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Perplexity failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(crate::ToolError::ExecutionFailed(format!(
                "Perplexity API error ({}): {}",
                status, body
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let answer = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(serde_json::json!({
            "query": query,
            "provider": "perplexity",
            "answer": format!("[web_content]{answer}[/web_content]"),
        }))
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_ddg_results(html: &str, max: usize) -> Vec<serde_json::Value> {
    let mut results = Vec::new();
    // Parse DuckDuckGo HTML result blocks: <a class="result__a" href="...">title</a>
    // and <a class="result__snippet">snippet</a>
    let mut pos = 0;
    while results.len() < max {
        // Find result link
        let link_marker = "class=\"result__a\"";
        let Some(link_start) = html[pos..].find(link_marker) else {
            break;
        };
        let link_start = pos + link_start;

        // Extract href
        let before = &html[link_start.saturating_sub(200)..link_start];
        let href = extract_attr(before, "href").unwrap_or_default();

        // Extract title text
        let after_tag = link_start + link_marker.len();
        let title = extract_tag_text(&html[after_tag..]).unwrap_or_default();

        // Find snippet
        let snippet_marker = "class=\"result__snippet\"";
        let snippet = if let Some(spos) = html[after_tag..].find(snippet_marker) {
            let s = after_tag + spos + snippet_marker.len();
            extract_tag_text(&html[s..]).unwrap_or_default()
        } else {
            String::new()
        };

        // Decode DuckDuckGo redirect URL
        let url = if href.contains("uddg=") {
            href.split("uddg=")
                .nth(1)
                .and_then(|u| urlencoding::decode(u.split('&').next().unwrap_or(u)).ok())
                .map(|s| s.into_owned())
                .unwrap_or(href)
        } else {
            href
        };

        if !url.is_empty() && !title.is_empty() {
            results.push(serde_json::json!({
                "title": strip_html_tags(&title),
                "url": url,
                "snippet": strip_html_tags(&snippet),
            }));
        }
        pos = after_tag + 1;
    }
    results
}

fn extract_attr(before: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    let start = before.rfind(&pattern)? + pattern.len();
    let end = before[start..].find('"')? + start;
    Some(before[start..end].to_string())
}

fn extract_tag_text(html: &str) -> Option<String> {
    let start = html.find('>')? + 1;
    let end = html[start..].find('<')? + start;
    Some(html[start..end].trim().to_string())
}

fn strip_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

// --- LinkReaderTool: fetch URL, strip HTML, return text ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkReaderTool {
    pub timeout_seconds: u64,
}

impl LinkReaderTool {
    pub fn new() -> Self {
        Self {
            timeout_seconds: 20,
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            url: String,
            #[serde(default)]
            max_chars: Option<usize>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;
        let max_chars = args.max_chars.unwrap_or(6000);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_seconds))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let resp = client
            .get(&args.url)
            .header("User-Agent", "Mozilla/5.0 (compatible; oclaw/1.0)")
            .send()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Fetch failed: {}", e)))?;

        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = resp
            .text()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let text = if content_type.contains("html") {
            html_to_text(&body)
        } else {
            body
        };

        let truncated = text.len() > max_chars;
        let content = if truncated { &text[..max_chars] } else { &text };

        Ok(serde_json::json!({
            "url": args.url, "status": status,
            "content": content, "truncated": truncated,
            "content_type": content_type,
        }))
    }
}

impl Default for LinkReaderTool {
    fn default() -> Self {
        Self::new()
    }
}

fn html_to_text(html: &str) -> String {
    // Phase 1: strip <script> and <style> blocks from the raw HTML
    let stripped = strip_script_style(html);

    // Phase 2: convert remaining HTML to plain text
    let mut out = String::with_capacity(stripped.len() / 3);
    let mut in_tag = false;
    let mut last_was_space = false;

    for c in stripped.chars() {
        if c == '<' {
            in_tag = true;
            continue;
        }
        if c == '>' {
            in_tag = false;
            continue;
        }
        if in_tag {
            continue;
        }
        if c.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(c);
            last_was_space = false;
        }
    }
    out.trim().to_string()
}

/// Remove all <script>...</script> and <style>...</style> blocks from HTML.
fn strip_script_style(html: &str) -> String {
    let mut result = html.to_string();
    for tag in &["script", "style"] {
        loop {
            let lower = result.to_lowercase();
            let open = format!("<{}", tag);
            let close = format!("</{}>", tag);
            let Some(start) = lower.find(&open) else {
                break;
            };
            let Some(end_rel) = lower[start..].find(&close) else {
                // No closing tag — remove from open tag to end
                result.truncate(start);
                break;
            };
            let end = start + end_rel + close.len();
            result.replace_range(start..end, "");
        }
    }
    result
}

// --- MediaDescribeTool: describe image via HTTP download + base64 for vision API ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaDescribeTool {
    pub timeout_seconds: u64,
}

impl MediaDescribeTool {
    pub fn new() -> Self {
        Self {
            timeout_seconds: 30,
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            url: String,
            #[serde(default)]
            prompt: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_seconds))
            .build()
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let resp =
            client.get(&args.url).send().await.map_err(|e| {
                crate::ToolError::ExecutionFailed(format!("Image fetch failed: {}", e))
            })?;

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("image/jpeg")
            .to_string();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let prompt = args
            .prompt
            .unwrap_or_else(|| "Describe this image in detail.".into());

        Ok(serde_json::json!({
            "url": args.url,
            "content_type": content_type,
            "size_bytes": bytes.len(),
            "base64": b64,
            "prompt": prompt,
            "note": "Image downloaded. Use the base64 data with a vision-capable model to get a description."
        }))
    }
}

impl Default for MediaDescribeTool {
    fn default() -> Self {
        Self::new()
    }
}

// --- CronTool: manage scheduled cron jobs ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronTool;

impl CronTool {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            action: String,
            #[serde(default)]
            job_id: Option<String>,
            #[serde(default)]
            schedule: Option<String>,
            #[serde(default)]
            command: Option<String>,
            #[serde(default)]
            label: Option<String>,
            #[serde(default)]
            enabled: Option<bool>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        match args.action.as_str() {
            "list" => Ok(
                serde_json::json!({ "action": "list", "jobs": [], "note": "Cron store not connected — use gateway RPC for persistent jobs" }),
            ),
            "add" => {
                let schedule = args
                    .schedule
                    .ok_or_else(|| crate::ToolError::InvalidInput("schedule required".into()))?;
                let command = args
                    .command
                    .ok_or_else(|| crate::ToolError::InvalidInput("command required".into()))?;
                let id = uuid::Uuid::new_v4().to_string();
                Ok(serde_json::json!({
                    "action": "add", "job_id": id,
                    "schedule": schedule, "command": command,
                    "label": args.label, "enabled": args.enabled.unwrap_or(true)
                }))
            }
            "update" => {
                let job_id = args
                    .job_id
                    .ok_or_else(|| crate::ToolError::InvalidInput("job_id required".into()))?;
                Ok(serde_json::json!({
                    "action": "update", "job_id": job_id,
                    "schedule": args.schedule, "command": args.command,
                    "label": args.label, "enabled": args.enabled
                }))
            }
            "remove" => {
                let job_id = args
                    .job_id
                    .ok_or_else(|| crate::ToolError::InvalidInput("job_id required".into()))?;
                Ok(serde_json::json!({ "action": "remove", "job_id": job_id, "removed": true }))
            }
            "run" => {
                let job_id = args
                    .job_id
                    .ok_or_else(|| crate::ToolError::InvalidInput("job_id required".into()))?;
                Ok(serde_json::json!({ "action": "run", "job_id": job_id, "triggered": true }))
            }
            "status" => {
                let job_id = args
                    .job_id
                    .ok_or_else(|| crate::ToolError::InvalidInput("job_id required".into()))?;
                Ok(serde_json::json!({ "action": "status", "job_id": job_id, "status": "unknown" }))
            }
            other => Err(crate::ToolError::InvalidInput(format!(
                "Unknown cron action: {}",
                other
            ))),
        }
    }
}

impl Default for CronTool {
    fn default() -> Self {
        Self::new()
    }
}

fn pick_first_non_empty(values: &[Option<String>]) -> Option<String> {
    values.iter().find_map(|value| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
    })
}

// --- MessageTool: send cross-channel messages ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTool;

impl MessageTool {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(default)]
            action: Option<String>,
            #[serde(default)]
            channel: Option<String>,
            #[serde(default)]
            provider: Option<String>,
            #[serde(default)]
            target: Option<String>,
            #[serde(default)]
            to: Option<String>,
            #[serde(default)]
            targets: Option<Vec<String>>,
            #[serde(default)]
            channels: Option<Vec<String>>,
            #[serde(default)]
            text: Option<String>,
            #[serde(default)]
            message: Option<String>,
            #[serde(default)]
            content: Option<String>,
            #[serde(default)]
            reply_to: Option<String>,
            #[serde(default, rename = "replyTo")]
            reply_to_alias: Option<String>,
            #[serde(default)]
            thread_id: Option<String>,
            #[serde(default, rename = "threadId")]
            thread_id_alias: Option<String>,
            #[serde(default)]
            message_id: Option<String>,
            #[serde(default, rename = "messageId")]
            message_id_alias: Option<String>,
            #[serde(default)]
            emoji: Option<String>,
            #[serde(default)]
            remove: Option<bool>,
            #[serde(default)]
            query: Option<String>,
            #[serde(default)]
            before: Option<String>,
            #[serde(default)]
            after: Option<String>,
            #[serde(default)]
            around: Option<String>,
            #[serde(default)]
            name: Option<String>,
            #[serde(default)]
            description: Option<String>,
            #[serde(default)]
            user_id: Option<String>,
            #[serde(default, rename = "userId")]
            user_id_alias: Option<String>,
            #[serde(default)]
            role_id: Option<String>,
            #[serde(default, rename = "roleId")]
            role_id_alias: Option<String>,
            #[serde(default)]
            target_id: Option<String>,
            #[serde(default, rename = "targetId")]
            target_id_alias: Option<String>,
            #[serde(default)]
            target_type: Option<String>,
            #[serde(default, rename = "targetType")]
            target_type_alias: Option<String>,
            #[serde(default)]
            allow: Option<String>,
            #[serde(default)]
            deny: Option<String>,
            #[serde(default)]
            parent_id: Option<String>,
            #[serde(default, rename = "parentId")]
            parent_id_alias: Option<String>,
            #[serde(default)]
            position: Option<u64>,
            #[serde(default)]
            topic: Option<String>,
            #[serde(default)]
            channel_type: Option<String>,
            #[serde(default, rename = "channelType")]
            channel_type_alias: Option<String>,
            #[serde(default)]
            start_time: Option<String>,
            #[serde(default, rename = "startTime")]
            start_time_alias: Option<String>,
            #[serde(default)]
            end_time: Option<String>,
            #[serde(default, rename = "endTime")]
            end_time_alias: Option<String>,
            #[serde(default)]
            duration_minutes: Option<u64>,
            #[serde(default, rename = "durationMinutes")]
            duration_minutes_alias: Option<u64>,
            #[serde(default)]
            delete_message_seconds: Option<u64>,
            #[serde(default, rename = "deleteMessageSeconds")]
            delete_message_seconds_alias: Option<u64>,
            #[serde(default)]
            tags: Option<String>,
            #[serde(default)]
            image: Option<String>,
            #[serde(default)]
            icon: Option<String>,
            #[serde(default)]
            effect: Option<String>,
            #[serde(default)]
            thread_name: Option<String>,
            #[serde(default, rename = "threadName")]
            thread_name_alias: Option<String>,
            #[serde(default)]
            auto_archive_minutes: Option<u64>,
            #[serde(default, rename = "autoArchiveMinutes")]
            auto_archive_minutes_alias: Option<u64>,
            #[serde(default, rename = "autoArchiveMin")]
            auto_archive_min_alias: Option<u64>,
            #[serde(default)]
            thread_type: Option<String>,
            #[serde(default, rename = "threadType")]
            thread_type_alias: Option<String>,
            #[serde(default)]
            group_id: Option<String>,
            #[serde(default, rename = "groupId")]
            group_id_alias: Option<String>,
            #[serde(default)]
            guild_id: Option<String>,
            #[serde(default, rename = "guildId")]
            guild_id_alias: Option<String>,
            #[serde(default)]
            limit: Option<u64>,
            #[serde(default)]
            include_archived: Option<bool>,
            #[serde(default, rename = "includeArchived")]
            include_archived_alias: Option<bool>,
            #[serde(default)]
            media: Option<String>,
            #[serde(default)]
            path: Option<String>,
            #[serde(default, rename = "filePath")]
            file_path: Option<String>,
            #[serde(default)]
            file_id: Option<String>,
            #[serde(default, rename = "fileId")]
            file_id_alias: Option<String>,
            #[serde(default)]
            buffer: Option<String>,
            #[serde(default)]
            filename: Option<String>,
            #[serde(default)]
            mime_type: Option<String>,
            #[serde(default, rename = "mimeType")]
            mime_type_alias: Option<String>,
            #[serde(default, rename = "contentType")]
            content_type_alias: Option<String>,
            #[serde(default)]
            caption: Option<String>,
            #[serde(default)]
            as_voice: Option<bool>,
            #[serde(default, rename = "asVoice")]
            as_voice_alias: Option<bool>,
            #[serde(default)]
            poll_question: Option<String>,
            #[serde(default, rename = "pollQuestion")]
            poll_question_alias: Option<String>,
            #[serde(default)]
            poll_options: Option<Vec<String>>,
            #[serde(default, rename = "pollOption")]
            poll_options_alias: Option<Vec<String>>,
            #[serde(default)]
            poll_anonymous: Option<bool>,
            #[serde(default, rename = "pollAnonymous")]
            poll_anonymous_alias: Option<bool>,
            #[serde(default)]
            poll_multiple: Option<bool>,
            #[serde(default, rename = "pollMulti")]
            poll_multiple_alias: Option<bool>,
            #[serde(default)]
            account_id: Option<String>,
            #[serde(default, rename = "accountId")]
            account_id_alias: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let action = args
            .action
            .as_deref()
            .unwrap_or("send")
            .trim()
            .to_ascii_lowercase();
        let channel = pick_first_non_empty(&[args.channel, args.provider]);
        let target = pick_first_non_empty(&[args.target, args.to]);
        let text = pick_first_non_empty(&[args.text, args.message, args.content]);
        let reply_to = pick_first_non_empty(&[args.reply_to, args.reply_to_alias]);
        let thread_id = pick_first_non_empty(&[args.thread_id, args.thread_id_alias]);
        let reply_anchor = pick_first_non_empty(&[reply_to.clone(), thread_id.clone()]);
        let message_id = pick_first_non_empty(&[args.message_id, args.message_id_alias]);
        let poll_question = pick_first_non_empty(&[args.poll_question, args.poll_question_alias]);
        let poll_options = args
            .poll_options
            .or(args.poll_options_alias)
            .unwrap_or_default();
        let poll_anonymous = args
            .poll_anonymous
            .or(args.poll_anonymous_alias)
            .unwrap_or(true);
        let poll_multiple = args
            .poll_multiple
            .or(args.poll_multiple_alias)
            .unwrap_or(false);
        let emoji = args
            .emoji
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let account_id = pick_first_non_empty(&[args.account_id, args.account_id_alias]);
        let thread_name = pick_first_non_empty(&[args.thread_name, args.thread_name_alias]);
        let auto_archive_minutes = args
            .auto_archive_minutes
            .or(args.auto_archive_minutes_alias)
            .or(args.auto_archive_min_alias);
        let thread_type = pick_first_non_empty(&[args.thread_type, args.thread_type_alias]);
        let group_id = pick_first_non_empty(&[
            args.group_id,
            args.group_id_alias,
            args.guild_id,
            args.guild_id_alias,
        ]);
        let include_archived = args
            .include_archived
            .or(args.include_archived_alias)
            .unwrap_or(false);
        let media = pick_first_non_empty(&[args.media, args.path, args.file_path]);
        let file_id = pick_first_non_empty(&[args.file_id, args.file_id_alias]);
        let mime_type = pick_first_non_empty(&[
            args.mime_type,
            args.mime_type_alias,
            args.content_type_alias,
        ]);
        let caption = args
            .caption
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let as_voice = args.as_voice.or(args.as_voice_alias).unwrap_or(false);
        let query = args
            .query
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let before = args
            .before
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let after = args
            .after
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let around = args
            .around
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let name = args
            .name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let description = args
            .description
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let user_id = pick_first_non_empty(&[args.user_id, args.user_id_alias]);
        let role_id = pick_first_non_empty(&[args.role_id, args.role_id_alias]);
        let target_id = pick_first_non_empty(&[args.target_id, args.target_id_alias]);
        let target_type = pick_first_non_empty(&[args.target_type, args.target_type_alias]);
        let permission_allow = args
            .allow
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let permission_deny = args
            .deny
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let parent_id = pick_first_non_empty(&[args.parent_id, args.parent_id_alias]);
        let topic = args
            .topic
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let channel_type = pick_first_non_empty(&[args.channel_type, args.channel_type_alias]);
        let start_time = pick_first_non_empty(&[args.start_time, args.start_time_alias]);
        let end_time = pick_first_non_empty(&[args.end_time, args.end_time_alias]);
        let duration_minutes = args.duration_minutes.or(args.duration_minutes_alias);
        let delete_message_seconds = args
            .delete_message_seconds
            .or(args.delete_message_seconds_alias);
        let tags = args
            .tags
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let image = args
            .image
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let icon = args
            .icon
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let effect = args
            .effect
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let mut targets = args.targets.unwrap_or_default();
        if targets.is_empty()
            && let Some(single_target) = target.as_deref()
        {
            targets.push(single_target.to_string());
        }
        targets = targets
            .into_iter()
            .map(|raw| raw.trim().to_string())
            .filter(|raw| !raw.is_empty())
            .collect();
        let channels = args
            .channels
            .unwrap_or_default()
            .into_iter()
            .map(|raw| raw.trim().to_string())
            .filter(|raw| !raw.is_empty())
            .collect::<Vec<String>>();

        // Message delivery is delegated to the channel adapter at runtime.
        // This tool returns a structured intent that the orchestrator fulfills.
        let intent = match action.as_str() {
            "send" | "send_message" | "message.send" | "reply" | "message.reply" => {
                let text = text.ok_or_else(|| {
                    crate::ToolError::InvalidInput("text/message/content required".into())
                })?;
                serde_json::json!({
                    "action": "send_message",
                    "channel": channel,
                    "target": target,
                    "text": text,
                    "reply_to": reply_anchor,
                    "account_id": account_id,
                    "status": "queued"
                })
            }
            "react" | "reaction" | "send_reaction" | "message.react" => {
                let message_id = message_id.ok_or_else(|| {
                    crate::ToolError::InvalidInput("message_id/messageId required".into())
                })?;
                let emoji =
                    emoji.ok_or_else(|| crate::ToolError::InvalidInput("emoji required".into()))?;
                let reaction_action = if args.remove.unwrap_or(false) {
                    "remove_reaction"
                } else {
                    "send_reaction"
                };
                serde_json::json!({
                    "action": reaction_action,
                    "channel": channel,
                    "target": target,
                    "message_id": message_id,
                    "emoji": emoji,
                    "account_id": account_id,
                    "status": "queued"
                })
            }
            "unreact" | "remove_reaction" | "message.unreact" => {
                let message_id = message_id.ok_or_else(|| {
                    crate::ToolError::InvalidInput("message_id/messageId required".into())
                })?;
                let emoji =
                    emoji.ok_or_else(|| crate::ToolError::InvalidInput("emoji required".into()))?;
                serde_json::json!({
                    "action": "remove_reaction",
                    "channel": channel,
                    "target": target,
                    "message_id": message_id,
                    "emoji": emoji,
                    "account_id": account_id,
                    "status": "queued"
                })
            }
            "reactions" | "list_reactions" | "message.reactions" => {
                let message_id = message_id.ok_or_else(|| {
                    crate::ToolError::InvalidInput("message_id/messageId required".into())
                })?;
                serde_json::json!({
                    "action": "list_reactions",
                    "channel": channel,
                    "target": target,
                    "message_id": message_id,
                    "limit": args.limit,
                    "status": "queued"
                })
            }
            "read" | "read_messages" | "message.read" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for read".into())
                })?;
                serde_json::json!({
                    "action": "read_messages",
                    "channel": channel,
                    "target": target,
                    "limit": args.limit,
                    "before": before,
                    "after": after,
                    "around": around,
                    "status": "queued"
                })
            }
            "search" | "search_messages" | "message.search" => {
                let query = query.or_else(|| text.clone()).ok_or_else(|| {
                    crate::ToolError::InvalidInput("query required for search".into())
                })?;
                serde_json::json!({
                    "action": "search_messages",
                    "channel": channel,
                    "target": target,
                    "query": query,
                    "limit": args.limit,
                    "status": "queued"
                })
            }
            "edit" | "edit_message" | "message.edit" => {
                let message_id = message_id.ok_or_else(|| {
                    crate::ToolError::InvalidInput("message_id/messageId required".into())
                })?;
                let text = text.ok_or_else(|| {
                    crate::ToolError::InvalidInput("text/message/content required".into())
                })?;
                serde_json::json!({
                    "action": "edit_message",
                    "channel": channel,
                    "target": target,
                    "message_id": message_id,
                    "text": text,
                    "account_id": account_id,
                    "status": "queued"
                })
            }
            "delete" | "delete_message" | "message.delete" | "unsend" | "message.unsend" => {
                let message_id = message_id.ok_or_else(|| {
                    crate::ToolError::InvalidInput("message_id/messageId required".into())
                })?;
                serde_json::json!({
                    "action": "delete_message",
                    "channel": channel,
                    "target": target,
                    "message_id": message_id,
                    "account_id": account_id,
                    "status": "queued"
                })
            }
            "pin" | "pin_message" | "message.pin" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for pin".into())
                })?;
                let message_id = message_id.ok_or_else(|| {
                    crate::ToolError::InvalidInput("message_id/messageId required".into())
                })?;
                serde_json::json!({
                    "action": "pin_message",
                    "channel": channel,
                    "target": target,
                    "message_id": message_id,
                    "status": "queued"
                })
            }
            "unpin" | "unpin_message" | "message.unpin" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for unpin".into())
                })?;
                let message_id = message_id.ok_or_else(|| {
                    crate::ToolError::InvalidInput("message_id/messageId required".into())
                })?;
                serde_json::json!({
                    "action": "unpin_message",
                    "channel": channel,
                    "target": target,
                    "message_id": message_id,
                    "status": "queued"
                })
            }
            "list_pins" | "list-pins" | "message.list_pins" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for list_pins".into())
                })?;
                serde_json::json!({
                    "action": "list_pins",
                    "channel": channel,
                    "target": target,
                    "limit": args.limit,
                    "status": "queued"
                })
            }
            "permissions" | "get_permissions" | "message.permissions" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for permissions".into())
                })?;
                serde_json::json!({
                    "action": "get_permissions",
                    "channel": channel,
                    "target": target,
                    "status": "queued"
                })
            }
            "channel_info" | "channel-info" | "message.channel_info" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for channel_info".into())
                })?;
                serde_json::json!({
                    "action": "channel_info",
                    "channel": channel,
                    "target": target,
                    "status": "queued"
                })
            }
            "channel_create" | "channel-create" | "message.channel_create" => {
                let name = name.ok_or_else(|| {
                    crate::ToolError::InvalidInput("name required for channel_create".into())
                })?;
                serde_json::json!({
                    "action": "channel_create",
                    "channel": channel,
                    "name": name,
                    "channel_type": channel_type,
                    "parent_id": parent_id,
                    "topic": topic,
                    "position": args.position,
                    "status": "queued"
                })
            }
            "channel_edit" | "channel-edit" | "message.channel_edit" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for channel_edit".into())
                })?;
                serde_json::json!({
                    "action": "channel_edit",
                    "channel": channel,
                    "target": target,
                    "name": name,
                    "topic": topic,
                    "parent_id": parent_id,
                    "position": args.position,
                    "status": "queued"
                })
            }
            "channel_delete" | "channel-delete" | "message.channel_delete" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for channel_delete".into())
                })?;
                serde_json::json!({
                    "action": "channel_delete",
                    "channel": channel,
                    "target": target,
                    "status": "queued"
                })
            }
            "channel_move" | "channel-move" | "message.channel_move" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for channel_move".into())
                })?;
                let position = args.position.ok_or_else(|| {
                    crate::ToolError::InvalidInput("position required for channel_move".into())
                })?;
                serde_json::json!({
                    "action": "channel_move",
                    "channel": channel,
                    "target": target,
                    "position": position,
                    "parent_id": parent_id,
                    "status": "queued"
                })
            }
            "channel_permission_set"
            | "channel-permission-set"
            | "channelpermissionset"
            | "message.channel_permission_set" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput(
                        "target/to required for channel_permission_set".into(),
                    )
                })?;
                let overwrite_target_id = target_id
                    .or_else(|| role_id.clone())
                    .or_else(|| user_id.clone())
                    .ok_or_else(|| {
                        crate::ToolError::InvalidInput(
                            "target_id/targetId (or role_id/user_id) required for channel_permission_set"
                                .into(),
                        )
                    })?;
                serde_json::json!({
                    "action": "channel_permission_set",
                    "channel": channel,
                    "target": target,
                    "target_id": overwrite_target_id,
                    "target_type": target_type.unwrap_or_else(|| "role".to_string()),
                    "allow": permission_allow,
                    "deny": permission_deny,
                    "status": "queued"
                })
            }
            "channel_permission_remove"
            | "channel-permission-remove"
            | "channelpermissionremove"
            | "message.channel_permission_remove" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput(
                        "target/to required for channel_permission_remove".into(),
                    )
                })?;
                let overwrite_target_id = target_id
                    .or_else(|| role_id.clone())
                    .or_else(|| user_id.clone())
                    .ok_or_else(|| {
                        crate::ToolError::InvalidInput(
                            "target_id/targetId (or role_id/user_id) required for channel_permission_remove"
                                .into(),
                        )
                    })?;
                serde_json::json!({
                    "action": "channel_permission_remove",
                    "channel": channel,
                    "target": target,
                    "target_id": overwrite_target_id,
                    "status": "queued"
                })
            }
            "category_create" | "category-create" | "message.category_create" => {
                let name = name.ok_or_else(|| {
                    crate::ToolError::InvalidInput("name required for category_create".into())
                })?;
                serde_json::json!({
                    "action": "category_create",
                    "channel": channel,
                    "name": name,
                    "position": args.position,
                    "status": "queued"
                })
            }
            "category_edit" | "category-edit" | "message.category_edit" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for category_edit".into())
                })?;
                let name = name.ok_or_else(|| {
                    crate::ToolError::InvalidInput("name required for category_edit".into())
                })?;
                serde_json::json!({
                    "action": "category_edit",
                    "channel": channel,
                    "target": target,
                    "name": name,
                    "position": args.position,
                    "status": "queued"
                })
            }
            "category_delete" | "category-delete" | "message.category_delete" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for category_delete".into())
                })?;
                serde_json::json!({
                    "action": "category_delete",
                    "channel": channel,
                    "target": target,
                    "status": "queued"
                })
            }
            "topic_create" | "topic-create" | "message.topic_create" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for topic_create".into())
                })?;
                let topic = topic.or_else(|| text.clone()).ok_or_else(|| {
                    crate::ToolError::InvalidInput("topic/text required for topic_create".into())
                })?;
                serde_json::json!({
                    "action": "topic_create",
                    "channel": channel,
                    "target": target,
                    "topic": topic,
                    "status": "queued"
                })
            }
            "thread_list" | "thread-list" | "message.thread_list" => serde_json::json!({
                "action": "thread_list",
                "channel": channel,
                "target": target,
                "guild_id": group_id.clone(),
                "group_id": group_id.clone(),
                "include_archived": include_archived,
                "before": before,
                "limit": args.limit,
                "status": "queued"
            }),
            "add_participant" | "addparticipant" | "message.add_participant" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for add_participant".into())
                })?;
                let user_id = user_id.ok_or_else(|| {
                    crate::ToolError::InvalidInput(
                        "user_id/userId required for add_participant".into(),
                    )
                })?;
                serde_json::json!({
                    "action": "add_participant",
                    "channel": channel,
                    "target": target,
                    "user_id": user_id,
                    "status": "queued"
                })
            }
            "remove_participant" | "removeparticipant" | "message.remove_participant" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput(
                        "target/to required for remove_participant".into(),
                    )
                })?;
                let user_id = user_id.ok_or_else(|| {
                    crate::ToolError::InvalidInput(
                        "user_id/userId required for remove_participant".into(),
                    )
                })?;
                serde_json::json!({
                    "action": "remove_participant",
                    "channel": channel,
                    "target": target,
                    "user_id": user_id,
                    "status": "queued"
                })
            }
            "leave_group" | "leavegroup" | "message.leave_group" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for leave_group".into())
                })?;
                serde_json::json!({
                    "action": "leave_group",
                    "channel": channel,
                    "target": target,
                    "status": "queued"
                })
            }
            "role_info" | "role-info" | "message.role_info" => serde_json::json!({
                "action": "role_info",
                "channel": channel,
                "guild_id": group_id.clone(),
                "role_id": role_id,
                "status": "queued"
            }),
            "role_add" | "role-add" | "message.role_add" => {
                let user_id = user_id.ok_or_else(|| {
                    crate::ToolError::InvalidInput("user_id/userId required for role_add".into())
                })?;
                let role_id = role_id.ok_or_else(|| {
                    crate::ToolError::InvalidInput("role_id/roleId required for role_add".into())
                })?;
                serde_json::json!({
                    "action": "role_add",
                    "channel": channel,
                    "guild_id": group_id.clone(),
                    "user_id": user_id,
                    "role_id": role_id,
                    "status": "queued"
                })
            }
            "role_remove" | "role-remove" | "message.role_remove" => {
                let user_id = user_id.ok_or_else(|| {
                    crate::ToolError::InvalidInput("user_id/userId required for role_remove".into())
                })?;
                let role_id = role_id.ok_or_else(|| {
                    crate::ToolError::InvalidInput("role_id/roleId required for role_remove".into())
                })?;
                serde_json::json!({
                    "action": "role_remove",
                    "channel": channel,
                    "guild_id": group_id.clone(),
                    "user_id": user_id,
                    "role_id": role_id,
                    "status": "queued"
                })
            }
            "kick_member" | "kick" | "message.kick" => {
                let user_id = user_id.ok_or_else(|| {
                    crate::ToolError::InvalidInput("user_id/userId required for kick".into())
                })?;
                serde_json::json!({
                    "action": "kick_member",
                    "channel": channel,
                    "guild_id": group_id.clone(),
                    "user_id": user_id,
                    "status": "queued"
                })
            }
            "ban_member" | "ban" | "message.ban" => {
                let user_id = user_id.ok_or_else(|| {
                    crate::ToolError::InvalidInput("user_id/userId required for ban".into())
                })?;
                serde_json::json!({
                    "action": "ban_member",
                    "channel": channel,
                    "guild_id": group_id.clone(),
                    "user_id": user_id,
                    "delete_message_seconds": delete_message_seconds,
                    "status": "queued"
                })
            }
            "timeout_member" | "timeout" | "message.timeout" => {
                let user_id = user_id.ok_or_else(|| {
                    crate::ToolError::InvalidInput("user_id/userId required for timeout".into())
                })?;
                serde_json::json!({
                    "action": "timeout_member",
                    "channel": channel,
                    "guild_id": group_id.clone(),
                    "user_id": user_id,
                    "duration_minutes": duration_minutes.unwrap_or(10),
                    "status": "queued"
                })
            }
            "event_list" | "event-list" | "message.event_list" => serde_json::json!({
                "action": "event_list",
                "channel": channel,
                "guild_id": group_id.clone(),
                "limit": args.limit,
                "status": "queued"
            }),
            "event_create" | "event-create" | "message.event_create" => {
                let name = name.ok_or_else(|| {
                    crate::ToolError::InvalidInput("name required for event_create".into())
                })?;
                serde_json::json!({
                    "action": "event_create",
                    "channel": channel,
                    "guild_id": group_id.clone(),
                    "target": target,
                    "name": name,
                    "description": description,
                    "start_time": start_time,
                    "end_time": end_time,
                    "status": "queued"
                })
            }
            "emoji_list" | "emoji-list" | "message.emoji_list" => serde_json::json!({
                "action": "emoji_list",
                "channel": channel,
                "guild_id": group_id.clone(),
                "status": "queued"
            }),
            "emoji_upload" | "emoji-upload" | "message.emoji_upload" => {
                let name = name.ok_or_else(|| {
                    crate::ToolError::InvalidInput("name required for emoji_upload".into())
                })?;
                let image = image
                    .or_else(|| media.clone())
                    .or_else(|| args.buffer.clone())
                    .ok_or_else(|| {
                        crate::ToolError::InvalidInput(
                            "image/media/buffer required for emoji_upload".into(),
                        )
                    })?;
                serde_json::json!({
                    "action": "emoji_upload",
                    "channel": channel,
                    "guild_id": group_id.clone(),
                    "name": name,
                    "image": image,
                    "filename": args.filename,
                    "mime_type": mime_type,
                    "status": "queued"
                })
            }
            "sticker_search" | "sticker-search" | "message.sticker_search" => {
                let query = query
                    .or_else(|| name.clone())
                    .or_else(|| text.clone())
                    .ok_or_else(|| {
                        crate::ToolError::InvalidInput("query required for sticker_search".into())
                    })?;
                serde_json::json!({
                    "action": "sticker_search",
                    "channel": channel,
                    "guild_id": group_id.clone(),
                    "query": query,
                    "limit": args.limit,
                    "status": "queued"
                })
            }
            "sticker_upload" | "sticker-upload" | "message.sticker_upload" => {
                let name = name.ok_or_else(|| {
                    crate::ToolError::InvalidInput("name required for sticker_upload".into())
                })?;
                let media = media.or_else(|| args.buffer.clone()).ok_or_else(|| {
                    crate::ToolError::InvalidInput(
                        "media/path/filePath/buffer required for sticker_upload".into(),
                    )
                })?;
                serde_json::json!({
                    "action": "sticker_upload",
                    "channel": channel,
                    "guild_id": group_id.clone(),
                    "name": name,
                    "description": description,
                    "tags": tags,
                    "media": media,
                    "filename": args.filename,
                    "mime_type": mime_type,
                    "status": "queued"
                })
            }
            "sticker" | "send_sticker" | "message.sticker" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for sticker".into())
                })?;
                let sticker_id =
                    pick_first_non_empty(&[file_id.clone(), message_id]).ok_or_else(|| {
                        crate::ToolError::InvalidInput(
                            "sticker_id/file_id/message_id required for sticker".into(),
                        )
                    })?;
                serde_json::json!({
                    "action": "send_sticker",
                    "channel": channel,
                    "target": target,
                    "sticker_id": sticker_id,
                    "text": text,
                    "status": "queued"
                })
            }
            "voice_status" | "voice-status" | "message.voice_status" => serde_json::json!({
                "action": "voice_status",
                "channel": channel,
                "guild_id": group_id.clone(),
                "target": target,
                "user_id": user_id,
                "status": "queued"
            }),
            "rename_group" | "renamegroup" | "message.rename_group" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for rename_group".into())
                })?;
                let name = name.ok_or_else(|| {
                    crate::ToolError::InvalidInput("name required for rename_group".into())
                })?;
                serde_json::json!({
                    "action": "rename_group",
                    "channel": channel,
                    "target": target,
                    "name": name,
                    "status": "queued"
                })
            }
            "set_group_icon" | "setgroupicon" | "message.set_group_icon" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for set_group_icon".into())
                })?;
                let icon = icon
                    .or_else(|| image.clone())
                    .or_else(|| media.clone())
                    .ok_or_else(|| {
                        crate::ToolError::InvalidInput(
                            "icon/image/media required for set_group_icon".into(),
                        )
                    })?;
                serde_json::json!({
                    "action": "set_group_icon",
                    "channel": channel,
                    "target": target,
                    "icon": icon,
                    "filename": args.filename,
                    "mime_type": mime_type,
                    "status": "queued"
                })
            }
            "set_presence" | "set-presence" | "message.set_presence" => serde_json::json!({
                "action": "set_presence",
                "channel": channel,
                "status_text": text.or(name),
                "status": "queued"
            }),
            "send_with_effect"
            | "sendwitheffect"
            | "sendWithEffect"
            | "message.send_with_effect" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for send_with_effect".into())
                })?;
                let text = text.ok_or_else(|| {
                    crate::ToolError::InvalidInput(
                        "text/message/content required for send_with_effect".into(),
                    )
                })?;
                serde_json::json!({
                    "action": "send_with_effect",
                    "channel": channel,
                    "target": target,
                    "text": text,
                    "effect": effect,
                    "status": "queued"
                })
            }
            "broadcast" | "broadcast_message" | "message.broadcast" => {
                let text = text.ok_or_else(|| {
                    crate::ToolError::InvalidInput("text/message/content required".into())
                })?;
                if targets.is_empty() {
                    return Err(crate::ToolError::InvalidInput(
                        "targets (or target/to) required for broadcast".into(),
                    ));
                }
                serde_json::json!({
                    "action": "broadcast_message",
                    "channel": channel,
                    "channels": channels,
                    "targets": targets,
                    "text": text,
                    "reply_to": reply_anchor,
                    "account_id": account_id,
                    "status": "queued"
                })
            }
            "send_attachment" | "sendattachment" | "message.send_attachment" => {
                let media = media.or(file_id.clone()).ok_or_else(|| {
                    crate::ToolError::InvalidInput(
                        "media/path/filePath/file_id/fileId required for send_attachment".into(),
                    )
                })?;
                serde_json::json!({
                    "action": "send_attachment",
                    "channel": channel,
                    "target": target,
                    "media": media,
                    "file_id": file_id,
                    "buffer": args.buffer,
                    "filename": args.filename,
                    "mime_type": mime_type,
                    "caption": caption.or(text),
                    "as_voice": as_voice,
                    "account_id": account_id,
                    "status": "queued"
                })
            }
            "thread_create" | "thread-create" | "threadcreate" | "message.thread_create" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for thread_create".into())
                })?;
                let thread_name = thread_name.ok_or_else(|| {
                    crate::ToolError::InvalidInput("thread_name/threadName required".into())
                })?;
                serde_json::json!({
                    "action": "thread_create",
                    "channel": channel,
                    "target": target,
                    "thread_name": thread_name,
                    "message_id": message_id,
                    "text": text,
                    "auto_archive_minutes": auto_archive_minutes,
                    "thread_type": thread_type,
                    "account_id": account_id,
                    "status": "queued"
                })
            }
            "thread_reply" | "thread-reply" | "message.thread_reply" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for thread_reply".into())
                })?;
                let text = text.ok_or_else(|| {
                    crate::ToolError::InvalidInput("text/message/content required".into())
                })?;
                let thread_id = thread_id.unwrap_or_else(|| target.clone());
                serde_json::json!({
                    "action": "send_thread_reply",
                    "channel": channel,
                    "target": target,
                    "thread_id": thread_id,
                    "reply_to": reply_to,
                    "text": text,
                    "account_id": account_id,
                    "status": "queued"
                })
            }
            "channel_list" | "channel-list" | "channellist" | "message.channel_list" => {
                serde_json::json!({
                    "action": "list_groups",
                    "channel": channel,
                    "limit": args.limit,
                    "status": "queued"
                })
            }
            "member_info" | "member-info" | "memberinfo" | "message.member_info" => {
                if let Some(uid) = user_id {
                    serde_json::json!({
                        "action": "member_info",
                        "channel": channel,
                        "target": target,
                        "user_id": uid,
                        "group_id": group_id.clone(),
                        "guild_id": group_id.clone(),
                        "status": "queued"
                    })
                } else {
                    let resolved_group_id = group_id.or_else(|| target.clone());
                    serde_json::json!({
                        "action": "list_members",
                        "channel": channel,
                        "target": target,
                        "group_id": resolved_group_id,
                        "limit": args.limit,
                        "status": "queued"
                    })
                }
            }
            "poll" | "send_poll" | "message.poll" => {
                let target = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/to required for poll".into())
                })?;
                let question = poll_question.ok_or_else(|| {
                    crate::ToolError::InvalidInput(
                        "poll_question/pollQuestion required for poll".into(),
                    )
                })?;
                if poll_options.is_empty() {
                    return Err(crate::ToolError::InvalidInput(
                        "poll_options/pollOption required for poll".into(),
                    ));
                }
                serde_json::json!({
                    "action": "send_poll",
                    "channel": channel,
                    "target": target,
                    "question": question,
                    "options": poll_options,
                    "is_anonymous": poll_anonymous,
                    "allows_multiple": poll_multiple,
                    "account_id": account_id,
                    "status": "queued"
                })
            }
            other => {
                return Err(crate::ToolError::InvalidInput(format!(
                    "unsupported message action: {}",
                    other
                )));
            }
        };

        Ok(intent)
    }
}

impl Default for MessageTool {
    fn default() -> Self {
        Self::new()
    }
}

// --- SessionsListTool: list active sessions ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsListTool;

impl SessionsListTool {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(default)]
            channel: Option<String>,
            #[serde(default)]
            limit: Option<usize>,
            #[serde(default)]
            active_only: Option<bool>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        Ok(serde_json::json!({
            "action": "sessions_list",
            "channel": args.channel,
            "limit": args.limit.unwrap_or(20),
            "active_only": args.active_only.unwrap_or(false),
            "sessions": [],
            "note": "Session store not connected — results populated by orchestrator"
        }))
    }
}

impl Default for SessionsListTool {
    fn default() -> Self {
        Self::new()
    }
}

// --- SessionsHistoryTool: retrieve session message history ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsHistoryTool;

impl SessionsHistoryTool {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            session_key: String,
            #[serde(default)]
            limit: Option<usize>,
            #[serde(default)]
            before: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        Ok(serde_json::json!({
            "action": "sessions_history",
            "session_key": args.session_key,
            "limit": args.limit.unwrap_or(50),
            "before": args.before,
            "messages": [],
            "note": "Session store not connected — results populated by orchestrator"
        }))
    }
}

impl Default for SessionsHistoryTool {
    fn default() -> Self {
        Self::new()
    }
}

// --- SessionsSendTool: send message into a session ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsSendTool;

impl SessionsSendTool {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(default)]
            session_key: Option<String>,
            #[serde(default)]
            #[serde(rename = "sessionKey")]
            session_key_alias: Option<String>,
            #[serde(default)]
            text: Option<String>,
            #[serde(default)]
            message: Option<String>,
            #[serde(default)]
            label: Option<String>,
            #[serde(default)]
            #[serde(rename = "agentId")]
            agent_id: Option<String>,
            #[serde(default)]
            role: Option<String>,
            #[serde(default)]
            #[serde(rename = "timeoutSeconds")]
            timeout_seconds: Option<f64>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let session_key = args
            .session_key
            .or(args.session_key_alias)
            .unwrap_or_default();
        if !session_key.trim().is_empty() && !args.label.as_deref().unwrap_or("").trim().is_empty()
        {
            return Err(crate::ToolError::InvalidInput(
                "Provide either session_key/sessionKey or label (not both)".into(),
            ));
        }
        let text = args.text.or(args.message).unwrap_or_default();
        if session_key.trim().is_empty() && args.label.as_deref().unwrap_or("").trim().is_empty() {
            return Err(crate::ToolError::InvalidInput(
                "Either session_key/sessionKey or label is required".into(),
            ));
        }
        if text.trim().is_empty() {
            return Err(crate::ToolError::InvalidInput(
                "text/message is required".into(),
            ));
        }
        let role = args.role.unwrap_or_else(|| "user".to_string());
        let timeout_seconds = args.timeout_seconds.map(|v| v.max(0.0).floor() as u64);
        Ok(serde_json::json!({
            "action": "sessions_send",
            "session_key": session_key,
            "text": text,
            "label": args.label,
            "agent_id": args.agent_id,
            "role": role,
            "timeout_seconds": timeout_seconds,
            "status": "queued"
        }))
    }
}

impl Default for SessionsSendTool {
    fn default() -> Self {
        Self::new()
    }
}

// --- SessionsSpawnTool: spawn a sub-session ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsSpawnTool;

impl SessionsSpawnTool {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(default)]
            task: Option<String>,
            #[serde(default)]
            label: Option<String>,
            #[serde(default)]
            #[serde(rename = "agentId")]
            agent_id_alias: Option<String>,
            #[serde(default)]
            agent_id: Option<String>,
            #[serde(default)]
            model: Option<String>,
            #[serde(default)]
            thinking: Option<String>,
            #[serde(default)]
            #[serde(rename = "runTimeoutSeconds")]
            run_timeout_seconds: Option<f64>,
            #[serde(default)]
            #[serde(rename = "timeoutSeconds")]
            timeout_seconds: Option<f64>,
            #[serde(default)]
            thread: Option<bool>,
            #[serde(default)]
            mode: Option<String>,
            #[serde(default)]
            cleanup: Option<String>,
            #[serde(default)]
            prompt: Option<String>,
            #[serde(default)]
            parent_session_key: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let agent_id = args
            .agent_id_alias
            .or(args.agent_id)
            .unwrap_or_else(|| "default".to_string());
        let task = args.task.or(args.prompt).unwrap_or_default();
        if task.trim().is_empty() {
            return Err(crate::ToolError::InvalidInput(
                "task/prompt is required".into(),
            ));
        }
        let run_timeout_seconds = args
            .run_timeout_seconds
            .or(args.timeout_seconds)
            .map(|v| v.max(0.0).floor() as u64);
        let session_id = uuid::Uuid::new_v4().to_string();
        Ok(serde_json::json!({
            "action": "sessions_spawn",
            "session_id": session_id,
            "agent_id": agent_id,
            "task": task,
            "label": args.label,
            "model": args.model,
            "thinking": args.thinking,
            "run_timeout_seconds": run_timeout_seconds,
            "thread": args.thread.unwrap_or(false),
            "mode": args.mode,
            "cleanup": args.cleanup,
            "parent_session_key": args.parent_session_key,
            "status": "spawned"
        }))
    }
}

impl Default for SessionsSpawnTool {
    fn default() -> Self {
        Self::new()
    }
}

// --- SubagentsTool: manage running subagents ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentsTool;

impl SubagentsTool {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            action: String,
            #[serde(default)]
            target: Option<String>,
            #[serde(default)]
            session_key: Option<String>,
            #[serde(default)]
            #[serde(rename = "sessionKey")]
            session_key_alias: Option<String>,
            #[serde(default)]
            message: Option<String>,
            #[serde(default)]
            #[serde(rename = "recentMinutes")]
            recent_minutes: Option<f64>,
            #[serde(default)]
            limit: Option<f64>,
            #[serde(default)]
            #[serde(rename = "includeTools")]
            include_tools: Option<bool>,
            #[serde(default)]
            #[serde(rename = "agentId")]
            agent_id_alias: Option<String>,
            #[serde(default)]
            agent_id: Option<String>,
            #[serde(default)]
            task: Option<String>,
            #[serde(default)]
            prompt: Option<String>,
            #[serde(default)]
            model: Option<String>,
            #[serde(default)]
            thinking: Option<String>,
            #[serde(default)]
            thread: Option<bool>,
            #[serde(default)]
            mode: Option<String>,
            #[serde(default)]
            cleanup: Option<String>,
            #[serde(default)]
            #[serde(rename = "runTimeoutSeconds")]
            run_timeout_seconds: Option<f64>,
            #[serde(default)]
            #[serde(rename = "timeoutSeconds")]
            timeout_seconds: Option<f64>,
            #[serde(default)]
            #[serde(rename = "targetKind")]
            target_kind: Option<String>,
            #[serde(default)]
            channel: Option<String>,
            #[serde(default)]
            to: Option<String>,
            #[serde(default)]
            #[serde(rename = "accountId")]
            account_id_alias: Option<String>,
            #[serde(default)]
            account_id: Option<String>,
            #[serde(default)]
            #[serde(rename = "threadId")]
            thread_id_alias: Option<String>,
            #[serde(default)]
            thread_id: Option<String>,
            #[serde(default)]
            #[serde(rename = "parentConversationId")]
            parent_conversation_id_alias: Option<String>,
            #[serde(default)]
            parent_conversation_id: Option<String>,
            #[serde(default)]
            #[serde(rename = "bindingId")]
            binding_id_alias: Option<String>,
            #[serde(default)]
            binding_id: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;
        let target = args
            .target
            .or(args.session_key)
            .or(args.session_key_alias)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let recent_minutes = args.recent_minutes.map(|v| v.max(1.0).floor() as u64);
        let log_limit = args.limit.map(|v| v.max(1.0).min(200.0).floor() as u64);
        let account_id = args
            .account_id_alias
            .or(args.account_id)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let thread_id = args
            .thread_id_alias
            .or(args.thread_id)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let parent_conversation_id = args
            .parent_conversation_id_alias
            .or(args.parent_conversation_id)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let binding_id = args
            .binding_id_alias
            .or(args.binding_id)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());

        match args.action.as_str() {
            "help" => Ok(serde_json::json!({
                "action": "subagents_help"
            })),
            "agents" => Ok(serde_json::json!({
                "action": "subagents_agents"
            })),
            "list" => Ok(serde_json::json!({
                "action": "subagents_list",
                "subagents": [],
                "recent_minutes": recent_minutes,
                "note": "Populated by orchestrator at runtime"
            })),
            "info" => {
                let key = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/session_key required for info".into())
                })?;
                Ok(serde_json::json!({
                    "action": "subagents_info",
                    "target": key
                }))
            }
            "log" => {
                let key = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/session_key required for log".into())
                })?;
                Ok(serde_json::json!({
                    "action": "subagents_log",
                    "target": key,
                    "limit": log_limit.unwrap_or(20),
                    "include_tools": args.include_tools.unwrap_or(false)
                }))
            }
            "send" => {
                let key = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/session_key required for send".into())
                })?;
                let msg = args.message.ok_or_else(|| {
                    crate::ToolError::InvalidInput("message required for send".into())
                })?;
                Ok(serde_json::json!({
                    "action": "subagents_send",
                    "target": key,
                    "message": msg
                }))
            }
            "kill" => {
                let key = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/session_key required for kill".into())
                })?;
                Ok(serde_json::json!({
                    "action": "subagents_kill",
                    "target": key,
                    "recent_minutes": recent_minutes,
                    "status": "killed"
                }))
            }
            "steer" => {
                let key = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/session_key required for steer".into())
                })?;
                let msg = args.message.ok_or_else(|| {
                    crate::ToolError::InvalidInput("message required for steer".into())
                })?;
                Ok(serde_json::json!({
                    "action": "subagents_steer",
                    "target": key,
                    "message": msg,
                    "recent_minutes": recent_minutes,
                    "status": "steered"
                }))
            }
            "spawn" => {
                let agent_id = args
                    .agent_id_alias
                    .or(args.agent_id)
                    .or_else(|| target.clone())
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| {
                        crate::ToolError::InvalidInput(
                            "agentId/agent_id (or target) required for spawn".into(),
                        )
                    })?;
                let task = args
                    .task
                    .or(args.prompt)
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| {
                        crate::ToolError::InvalidInput("task/prompt required for spawn".into())
                    })?;
                let run_timeout_seconds = args
                    .run_timeout_seconds
                    .or(args.timeout_seconds)
                    .map(|v| v.max(0.0).floor() as u64);
                Ok(serde_json::json!({
                    "action": "subagents_spawn",
                    "agent_id": agent_id,
                    "task": task,
                    "model": args.model,
                    "thinking": args.thinking,
                    "thread": args.thread.unwrap_or(false),
                    "mode": args.mode,
                    "cleanup": args.cleanup,
                    "run_timeout_seconds": run_timeout_seconds
                }))
            }
            "focus" => {
                let key = target.ok_or_else(|| {
                    crate::ToolError::InvalidInput("target/session_key required for focus".into())
                })?;
                Ok(serde_json::json!({
                    "action": "subagents_focus",
                    "target": key,
                    "target_kind": args.target_kind,
                    "channel": args.channel,
                    "to": args.to,
                    "account_id": account_id,
                    "thread_id": thread_id,
                    "parent_conversation_id": parent_conversation_id
                }))
            }
            "unfocus" => Ok(serde_json::json!({
                "action": "subagents_unfocus",
                "target": target,
                "binding_id": binding_id
            })),
            other => Err(crate::ToolError::InvalidInput(format!(
                "Unknown subagents action: {}",
                other
            ))),
        }
    }
}

impl Default for SubagentsTool {
    fn default() -> Self {
        Self::new()
    }
}

// --- SessionStatusTool: get session status ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatusTool;

impl SessionStatusTool {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(default)]
            session_key: Option<String>,
            #[serde(default)]
            #[serde(rename = "sessionKey")]
            session_key_alias: Option<String>,
            #[serde(default)]
            model: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;
        let session_key = args.session_key.or(args.session_key_alias);

        Ok(serde_json::json!({
            "action": "session_status",
            "session_key": session_key,
            "model": args.model,
            "status": "unknown",
            "note": "Populated by orchestrator at runtime"
        }))
    }
}

impl Default for SessionStatusTool {
    fn default() -> Self {
        Self::new()
    }
}

// --- TtsTool: text-to-speech conversion ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsTool {
    pub default_provider: String,
}

impl TtsTool {
    pub fn new() -> Self {
        Self {
            default_provider: "openai".to_string(),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            text: String,
            #[serde(default)]
            provider: Option<String>,
            #[serde(default)]
            voice: Option<String>,
            #[serde(default)]
            model: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        if args.text.is_empty() {
            return Err(crate::ToolError::InvalidInput(
                "text must not be empty".into(),
            ));
        }

        let provider = args
            .provider
            .unwrap_or_else(|| self.default_provider.clone());

        match provider.as_str() {
            "openai" => {
                self.tts_openai(&args.text, args.voice.as_deref(), args.model.as_deref())
                    .await
            }
            "elevenlabs" => {
                self.tts_elevenlabs(&args.text, args.voice.as_deref(), args.model.as_deref())
                    .await
            }
            "edge" => self.tts_edge(&args.text, args.voice.as_deref()).await,
            other => Err(crate::ToolError::InvalidInput(format!(
                "Unknown TTS provider: {}",
                other
            ))),
        }
    }

    async fn tts_openai(
        &self,
        text: &str,
        voice: Option<&str>,
        model: Option<&str>,
    ) -> ToolResult<serde_json::Value> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| crate::ToolError::ExecutionFailed("OPENAI_API_KEY not set".into()))?;

        let voice = voice.unwrap_or("alloy");
        let model = model.unwrap_or("tts-1");

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let body = serde_json::json!({
            "model": model,
            "input": text,
            "voice": voice,
            "response_format": "mp3"
        });

        let resp = client
            .post("https://api.openai.com/v1/audio/speech")
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                crate::ToolError::ExecutionFailed(format!("OpenAI TTS request failed: {}", e))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(crate::ToolError::ExecutionFailed(format!(
                "OpenAI TTS error ({}): {}",
                status, text
            )));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let tmp = std::env::temp_dir().join(format!("oclaw-tts-{}.mp3", uuid::Uuid::new_v4()));
        tokio::fs::write(&tmp, &bytes).await.map_err(|e| {
            crate::ToolError::ExecutionFailed(format!("Failed to write audio: {}", e))
        })?;

        Ok(serde_json::json!({
            "provider": "openai",
            "voice": voice,
            "model": model,
            "audio_path": tmp.to_string_lossy(),
            "size_bytes": bytes.len(),
            "format": "mp3"
        }))
    }

    async fn tts_elevenlabs(
        &self,
        text: &str,
        voice: Option<&str>,
        model: Option<&str>,
    ) -> ToolResult<serde_json::Value> {
        let api_key = std::env::var("ELEVENLABS_API_KEY")
            .map_err(|_| crate::ToolError::ExecutionFailed("ELEVENLABS_API_KEY not set".into()))?;

        let voice_id = voice.unwrap_or("21m00Tcm4TlvDq8ikWAM");
        let model_id = model.unwrap_or("eleven_monolingual_v1");

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let body = serde_json::json!({
            "text": text,
            "model_id": model_id,
            "voice_settings": { "stability": 0.5, "similarity_boost": 0.75 }
        });

        let url = format!("https://api.elevenlabs.io/v1/text-to-speech/{}", voice_id);

        let resp = client
            .post(&url)
            .header("xi-api-key", &api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                crate::ToolError::ExecutionFailed(format!("ElevenLabs TTS request failed: {}", e))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(crate::ToolError::ExecutionFailed(format!(
                "ElevenLabs TTS error ({}): {}",
                status, text
            )));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let tmp = std::env::temp_dir().join(format!("oclaw-tts-{}.mp3", uuid::Uuid::new_v4()));
        tokio::fs::write(&tmp, &bytes).await.map_err(|e| {
            crate::ToolError::ExecutionFailed(format!("Failed to write audio: {}", e))
        })?;

        Ok(serde_json::json!({
            "provider": "elevenlabs",
            "voice_id": voice_id,
            "model_id": model_id,
            "audio_path": tmp.to_string_lossy(),
            "size_bytes": bytes.len(),
            "format": "mp3"
        }))
    }

    async fn tts_edge(&self, text: &str, voice: Option<&str>) -> ToolResult<serde_json::Value> {
        let voice = voice.unwrap_or("en-US-AriaNeural");
        // Edge TTS uses a local CLI tool (edge-tts) if available
        let tmp = std::env::temp_dir().join(format!("oclaw-tts-{}.mp3", uuid::Uuid::new_v4()));

        let output = tokio::process::Command::new("edge-tts")
            .args([
                "--voice",
                voice,
                "--text",
                text,
                "--write-media",
                &tmp.to_string_lossy(),
            ])
            .output()
            .await
            .map_err(|e| {
                crate::ToolError::ExecutionFailed(format!(
                    "edge-tts not found or failed: {}. Install with: pip install edge-tts",
                    e
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(crate::ToolError::ExecutionFailed(format!(
                "edge-tts failed: {}",
                stderr
            )));
        }

        let size = tokio::fs::metadata(&tmp)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(serde_json::json!({
            "provider": "edge",
            "voice": voice,
            "audio_path": tmp.to_string_lossy(),
            "size_bytes": size,
            "format": "mp3"
        }))
    }
}

impl Default for TtsTool {
    fn default() -> Self {
        Self::new()
    }
}

// --- WorkspaceTool: agent self-modification via workspace files ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceTool {
    /// Root directory of the agent workspace.
    pub workspace_root: String,
}

impl WorkspaceTool {
    pub fn new(workspace_root: impl Into<String>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }

    /// Resolve and validate a relative path within the workspace.
    fn resolve_path(&self, rel: &str) -> Result<std::path::PathBuf, crate::ToolError> {
        let root = std::path::Path::new(&self.workspace_root);
        let target = root.join(rel);

        // Canonicalize what exists, then check prefix
        let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        // For new files, check the parent
        let check_path = if target.exists() {
            target.canonicalize().unwrap_or_else(|_| target.clone())
        } else if let Some(parent) = target.parent() {
            let p = parent
                .canonicalize()
                .unwrap_or_else(|_| parent.to_path_buf());
            p.join(target.file_name().unwrap_or_default())
        } else {
            target.clone()
        };

        if !check_path.starts_with(&canonical_root) {
            return Err(crate::ToolError::InvalidInput(
                "Path escapes workspace directory".into(),
            ));
        }
        Ok(target)
    }

    pub async fn execute(
        &self,
        arguments: serde_json::Value,
    ) -> crate::error::ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            action: String,
            #[serde(default)]
            path: Option<String>,
            #[serde(default)]
            content: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        match args.action.as_str() {
            "read" => self.action_read(args.path).await,
            "write" => self.action_write(args.path, args.content).await,
            "append" => self.action_append(args.path, args.content).await,
            "list" => self.action_list(args.path).await,
            other => Err(crate::ToolError::InvalidInput(format!(
                "Unknown workspace action: {}",
                other
            ))),
        }
    }

    async fn action_read(
        &self,
        path: Option<String>,
    ) -> crate::error::ToolResult<serde_json::Value> {
        let rel =
            path.ok_or_else(|| crate::ToolError::InvalidInput("path required for read".into()))?;
        let full = self.resolve_path(&rel)?;
        let content = tokio::fs::read_to_string(&full)
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Read failed: {}", e)))?;
        Ok(serde_json::json!({
            "action": "read", "path": rel,
            "content": content, "size": content.len(),
        }))
    }

    async fn action_write(
        &self,
        path: Option<String>,
        content: Option<String>,
    ) -> crate::error::ToolResult<serde_json::Value> {
        let rel =
            path.ok_or_else(|| crate::ToolError::InvalidInput("path required for write".into()))?;
        let content = content
            .ok_or_else(|| crate::ToolError::InvalidInput("content required for write".into()))?;
        let full = self.resolve_path(&rel)?;
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| crate::ToolError::ExecutionFailed(format!("mkdir failed: {}", e)))?;
        }
        tokio::fs::write(&full, &content)
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Write failed: {}", e)))?;
        Ok(serde_json::json!({
            "action": "write", "path": rel,
            "bytes_written": content.len(),
        }))
    }

    async fn action_append(
        &self,
        path: Option<String>,
        content: Option<String>,
    ) -> crate::error::ToolResult<serde_json::Value> {
        let rel =
            path.ok_or_else(|| crate::ToolError::InvalidInput("path required for append".into()))?;
        let content = content
            .ok_or_else(|| crate::ToolError::InvalidInput("content required for append".into()))?;
        let full = self.resolve_path(&rel)?;
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| crate::ToolError::ExecutionFailed(format!("mkdir failed: {}", e)))?;
        }
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&full)
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Open failed: {}", e)))?;
        file.write_all(content.as_bytes())
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Append failed: {}", e)))?;
        Ok(serde_json::json!({
            "action": "append", "path": rel,
            "bytes_appended": content.len(),
        }))
    }

    async fn action_list(
        &self,
        path: Option<String>,
    ) -> crate::error::ToolResult<serde_json::Value> {
        let rel = path.unwrap_or_default();
        let full = if rel.is_empty() {
            std::path::PathBuf::from(&self.workspace_root)
        } else {
            self.resolve_path(&rel)?
        };
        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&full)
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("List failed: {}", e)))?;
        while let Some(entry) = dir
            .next_entry()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?
        {
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = entry.metadata().await.ok();
            entries.push(serde_json::json!({
                "name": name,
                "is_dir": meta.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                "size": meta.as_ref().map(|m| m.len()).unwrap_or(0),
            }));
        }
        Ok(serde_json::json!({
            "action": "list",
            "path": if rel.is_empty() { "." } else { &rel },
            "entries": entries, "count": entries.len(),
        }))
    }
}

#[cfg(test)]
mod message_tool_tests {
    use super::MessageTool;

    #[tokio::test]
    async fn thread_reply_uses_target_as_thread_and_preserves_reply_to() {
        let tool = MessageTool::new();
        let result = tool
            .execute(serde_json::json!({
                "action": "thread_reply",
                "channel": "discord",
                "target": "thread-123",
                "replyTo": "msg-9",
                "text": "hello"
            }))
            .await
            .expect("thread_reply intent should be built");

        assert_eq!(
            result.get("action").and_then(|v| v.as_str()),
            Some("send_thread_reply")
        );
        assert_eq!(
            result.get("thread_id").and_then(|v| v.as_str()),
            Some("thread-123")
        );
        assert_eq!(
            result.get("reply_to").and_then(|v| v.as_str()),
            Some("msg-9")
        );
    }

    #[tokio::test]
    async fn send_uses_thread_id_as_reply_anchor_when_reply_to_missing() {
        let tool = MessageTool::new();
        let result = tool
            .execute(serde_json::json!({
                "action": "send",
                "channel": "discord",
                "target": "chan-1",
                "threadId": "thread-42",
                "text": "hello"
            }))
            .await
            .expect("send intent should be built");

        assert_eq!(
            result.get("action").and_then(|v| v.as_str()),
            Some("send_message")
        );
        assert_eq!(
            result.get("reply_to").and_then(|v| v.as_str()),
            Some("thread-42")
        );
    }
}
