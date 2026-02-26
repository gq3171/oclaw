//! System prompt builder — assembles dynamic system prompts from workspace files.

use crate::files::Workspace;
use crate::identity::AgentIdentity;
use crate::soul::Soul;

/// Per-file character budget for bootstrap files (aligns with Node DEFAULT_BOOTSTRAP_MAX_CHARS).
pub const BOOTSTRAP_MAX_CHARS_PER_FILE: usize = 20_000;
/// Total character budget across all bootstrap files (aligns with Node DEFAULT_BOOTSTRAP_TOTAL_MAX_CHARS).
pub const BOOTSTRAP_TOTAL_MAX_CHARS: usize = 150_000;

const HEAD_RATIO: f64 = 0.7;
const TAIL_RATIO: f64 = 0.2;

/// Truncate a bootstrap file, keeping the first 70% and last 20% with a gap marker.
/// Matches Node's trimBootstrapContent behaviour.
pub fn trim_bootstrap_content(content: &str, file_name: &str, max_chars: usize) -> String {
    if content.len() <= max_chars {
        return content.to_string();
    }
    let head_chars = (max_chars as f64 * HEAD_RATIO) as usize;
    let tail_chars = (max_chars as f64 * TAIL_RATIO) as usize;
    // Clamp to valid char boundaries
    let head_end = content
        .char_indices()
        .take_while(|(i, _)| *i < head_chars)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let tail_start = content.len().saturating_sub(tail_chars);
    let tail_start = content
        .char_indices()
        .map(|(i, _)| i)
        .filter(|&i| i >= tail_start)
        .next()
        .unwrap_or(content.len());
    format!(
        "{}\n\n[...truncated, read {} for full content...]\
         \n…(truncated {}: kept {}+{} chars of {})…\n\n{}",
        &content[..head_end],
        file_name,
        file_name,
        head_end,
        content.len() - tail_start,
        content.len(),
        &content[tail_start..]
    )
}

/// Controls which sections are included in the system prompt.
/// Mirrors Node's PromptMode: "full" | "minimal" | "none".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptMode {
    /// Full prompt for the main agent (all sections).
    #[default]
    Full,
    /// Reduced prompt for subagents (Tooling + Runtime only).
    Minimal,
    /// Bare identity line for internal tool-call contexts.
    None,
}

/// Runtime information injected into the system prompt so the agent
/// understands its own execution environment (model, OS, tools, etc.).
#[derive(Debug, Clone, Default)]
pub struct RuntimeInfo {
    pub agent_id: Option<String>,
    pub model: Option<String>,
    pub default_model: Option<String>,
    pub os: Option<String>,
    pub arch: Option<String>,
    pub host: Option<String>,
    pub shell: Option<String>,
    pub channel: Option<String>,
    pub workspace_dir: Option<String>,
    pub version: Option<String>,
}

impl RuntimeInfo {
    /// Format as a single-line runtime descriptor.
    pub fn to_line(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = &self.agent_id { parts.push(format!("agent={}", v)); }
        if let Some(v) = &self.model { parts.push(format!("model={}", v)); }
        if let Some(v) = &self.default_model {
            if self.model.as_deref() != Some(v) {
                parts.push(format!("default_model={}", v));
            }
        }
        if let Some(os) = &self.os {
            let arch_str = self.arch.as_deref().unwrap_or("unknown");
            parts.push(format!("os={} ({})", os, arch_str));
        }
        if let Some(v) = &self.host { parts.push(format!("host={}", v)); }
        if let Some(v) = &self.shell { parts.push(format!("shell={}", v)); }
        if let Some(v) = &self.channel { parts.push(format!("channel={}", v)); }
        if let Some(v) = &self.workspace_dir { parts.push(format!("workspace={}", v)); }
        if let Some(v) = &self.version { parts.push(format!("version={}", v)); }
        parts.join(" | ")
    }
}

/// Builds a complete system prompt by injecting workspace context.
pub struct SystemPromptBuilder {
    mode: PromptMode,
    base_prompt: Option<String>,
    soul: Option<Soul>,
    identity: Option<AgentIdentity>,
    user_context: Option<String>,
    memory_hint: bool,
    heartbeat_mode: bool,
    available_tools: Vec<String>,
    runtime: Option<RuntimeInfo>,
    bootstrap_files: Vec<(String, String)>, // (filename, content)
    bootstrap_max_chars_per_file: usize,
    bootstrap_total_max_chars: usize,
}

impl SystemPromptBuilder {
    pub fn new() -> Self {
        Self {
            mode: PromptMode::Full,
            base_prompt: None,
            soul: None,
            identity: None,
            user_context: None,
            memory_hint: false,
            heartbeat_mode: false,
            available_tools: Vec::new(),
            runtime: None,
            bootstrap_files: Vec::new(),
            bootstrap_max_chars_per_file: BOOTSTRAP_MAX_CHARS_PER_FILE,
            bootstrap_total_max_chars: BOOTSTRAP_TOTAL_MAX_CHARS,
        }
    }

    pub fn with_mode(mut self, mode: PromptMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_base_prompt(mut self, prompt: &str) -> Self {
        self.base_prompt = Some(prompt.to_string());
        self
    }

    pub fn with_soul(mut self, soul: Soul) -> Self {
        self.soul = Some(soul);
        self
    }

    pub fn with_identity(mut self, identity: AgentIdentity) -> Self {
        self.identity = Some(identity);
        self
    }

    pub fn with_user_context(mut self, content: String) -> Self {
        self.user_context = Some(content);
        self
    }

    pub fn with_memory_hint(mut self, enabled: bool) -> Self {
        self.memory_hint = enabled;
        self
    }

    pub fn with_heartbeat_mode(mut self, enabled: bool) -> Self {
        self.heartbeat_mode = enabled;
        self
    }

    pub fn with_available_tools(mut self, tools: Vec<String>) -> Self {
        self.available_tools = tools;
        self
    }

    pub fn with_runtime(mut self, runtime: RuntimeInfo) -> Self {
        self.runtime = Some(runtime);
        self
    }

    /// Add an extra bootstrap file to inject into the "## Extra Context" section.
    pub fn with_bootstrap_file(mut self, name: impl Into<String>, content: impl Into<String>) -> Self {
        self.bootstrap_files.push((name.into(), content.into()));
        self
    }

    /// Assemble the final system prompt string.
    pub fn build(&self) -> String {
        match self.mode {
            PromptMode::None    => self.build_none(),
            PromptMode::Minimal => self.build_minimal(),
            PromptMode::Full    => self.build_full(),
        }
    }

    /// Bare identity line (for internal tool-call contexts).
    fn build_none(&self) -> String {
        if let Some(id) = &self.identity {
            format!("You are {}.", id.display_name())
        } else {
            "You are a helpful assistant.".to_string()
        }
    }

    /// Reduced prompt for subagents (Runtime + Tools only).
    fn build_minimal(&self) -> String {
        let mut sections: Vec<String> = Vec::new();

        if let Some(id) = &self.identity {
            sections.push(format!("You are {}.", id.display_name()));
        }

        if let Some(ref rt) = self.runtime {
            let line = rt.to_line();
            if !line.is_empty() {
                sections.push(format!("## Runtime\n{}", line));
            }
        }

        if !self.available_tools.is_empty() {
            sections.push(format!(
                "## Available Tools\n{}",
                self.available_tools.join(", ")
            ));
        }

        if self.memory_hint {
            sections.push(
                "## Memory Recall\n\
                 Before answering about prior work, decisions, or preferences: \
                 run memory_search first."
                    .to_string(),
            );
        }

        sections.join("\n\n")
    }

    /// Full system prompt for the main agent.
    fn build_full(&self) -> String {
        let mut sections: Vec<String> = Vec::new();
        let mut total_bootstrap_chars = 0usize;

        // Identity line
        if let Some(id) = &self.identity {
            sections.push(format!("## Identity\nYou are {}.", id.display_name()));
        }

        // Soul section (with truncation)
        if let Some(soul) = &self.soul {
            let s = soul.to_prompt_section();
            if !s.is_empty() {
                let trimmed = trim_bootstrap_content(&s, "SOUL.md", self.bootstrap_max_chars_per_file);
                total_bootstrap_chars += trimmed.len();
                sections.push(trimmed);
            }
        }

        // User context (USER.md)
        if let Some(ref uc) = self.user_context {
            if total_bootstrap_chars < self.bootstrap_total_max_chars {
                let budget = (self.bootstrap_max_chars_per_file)
                    .min(self.bootstrap_total_max_chars - total_bootstrap_chars);
                let trimmed = trim_bootstrap_content(uc, "USER.md", budget);
                total_bootstrap_chars += trimmed.len();
                sections.push(format!("## About Your Human\n{}", trimmed));
            }
        }

        // Memory recall section
        if self.memory_hint {
            sections.push(
                "## Memory Recall\n\
                 Before answering anything about prior work, decisions, dates, \
                 people, preferences, or todos: run memory_search on MEMORY.md + memory/*.md; \
                 then use memory_get to pull only the needed lines.\n\
                 If low confidence after search, say you checked."
                    .to_string(),
            );
        }

        // Heartbeat section
        if self.heartbeat_mode {
            sections.push(
                "## Heartbeat\n\
                 This is a periodic heartbeat check. Read HEARTBEAT.md if it \
                 exists and follow it strictly. Do not infer or repeat old tasks. \
                 If nothing needs attention, reply HEARTBEAT_OK."
                    .to_string(),
            );
        }

        // Runtime self-awareness
        if let Some(ref rt) = self.runtime {
            let line = rt.to_line();
            if !line.is_empty() {
                sections.push(format!("## Runtime\n{}", line));
            }
        }

        // Available tools
        if !self.available_tools.is_empty() {
            sections.push(format!(
                "## Available Tools\n{}",
                self.available_tools.join(", ")
            ));
        }

        // Extra bootstrap files
        for (name, content) in &self.bootstrap_files {
            if total_bootstrap_chars >= self.bootstrap_total_max_chars {
                break;
            }
            let budget = self.bootstrap_max_chars_per_file
                .min(self.bootstrap_total_max_chars - total_bootstrap_chars);
            let trimmed = trim_bootstrap_content(content, name, budget);
            total_bootstrap_chars += trimmed.len();
            sections.push(format!("## {}\n{}", name, trimmed));
        }

        // Soul evolution hint
        sections.push(
            "## Continuity\n\
             You have a `workspace` tool. Your workspace files (SOUL.md, IDENTITY.md, \
             MEMORY.md, HEARTBEAT.md) define who you are across sessions. You may read \
             and update them as you grow. If you change SOUL.md, mention it to the user."
                .to_string(),
        );

        // Base prompt (user-configured system prompt)
        if let Some(base) = &self.base_prompt {
            sections.push(base.clone());
        }

        // Current date/time
        let now = chrono::Local::now();
        sections.push(format!(
            "## Current Date & Time\n{}",
            now.format("%Y-%m-%d %H:%M %Z")
        ));

        sections.join("\n\n")
    }
}

impl Default for SystemPromptBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Load workspace files and build a system prompt in one call.
pub async fn load_and_build(
    ws: &Workspace,
    base_prompt: Option<&str>,
    heartbeat_mode: bool,
) -> anyhow::Result<String> {
    load_and_build_with_runtime(ws, base_prompt, heartbeat_mode, None, &[]).await
}

/// Load workspace files and build a system prompt with runtime info and tools.
pub async fn load_and_build_with_runtime(
    ws: &Workspace,
    base_prompt: Option<&str>,
    heartbeat_mode: bool,
    runtime: Option<RuntimeInfo>,
    tools: &[String],
) -> anyhow::Result<String> {
    let soul = Soul::load(ws).await?;
    let identity = AgentIdentity::load(ws).await?;
    let user_context = ws.read_file(&ws.user_path()).await?;

    let mut builder = SystemPromptBuilder::new()
        .with_memory_hint(true)
        .with_heartbeat_mode(heartbeat_mode);

    if let Some(bp) = base_prompt {
        builder = builder.with_base_prompt(bp);
    }
    if let Some(s) = soul {
        builder = builder.with_soul(s);
    }
    if let Some(id) = identity {
        builder = builder.with_identity(id);
    }
    if let Some(uc) = user_context {
        builder = builder.with_user_context(uc);
    }
    if let Some(rt) = runtime {
        builder = builder.with_runtime(rt);
    }
    if !tools.is_empty() {
        builder = builder.with_available_tools(tools.to_vec());
    }

    Ok(builder.build())
}

/// Build a minimal prompt for subagents.
pub async fn load_and_build_minimal(
    ws: &Workspace,
    runtime: Option<RuntimeInfo>,
    tools: &[String],
) -> anyhow::Result<String> {
    let identity = AgentIdentity::load(ws).await?;
    let mut builder = SystemPromptBuilder::new()
        .with_mode(PromptMode::Minimal)
        .with_memory_hint(true);

    if let Some(id) = identity {
        builder = builder.with_identity(id);
    }
    if let Some(rt) = runtime {
        builder = builder.with_runtime(rt);
    }
    if !tools.is_empty() {
        builder = builder.with_available_tools(tools.to_vec());
    }

    Ok(builder.build())
}
