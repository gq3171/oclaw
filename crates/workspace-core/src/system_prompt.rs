//! System prompt builder — assembles dynamic system prompts from workspace files.

use crate::files::Workspace;
use crate::identity::AgentIdentity;
use crate::soul::Soul;

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
    /// Format as a single-line runtime descriptor (like Node's buildRuntimeLine).
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
    base_prompt: Option<String>,
    soul: Option<Soul>,
    identity: Option<AgentIdentity>,
    memory_hint: bool,
    heartbeat_mode: bool,
    available_tools: Vec<String>,
    runtime: Option<RuntimeInfo>,
}

impl SystemPromptBuilder {
    pub fn new() -> Self {
        Self {
            base_prompt: None,
            soul: None,
            identity: None,
            memory_hint: false,
            heartbeat_mode: false,
            available_tools: Vec::new(),
            runtime: None,
        }
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

    /// Assemble the final system prompt string.
    pub fn build(&self) -> String {
        let mut sections: Vec<String> = Vec::new();

        // Identity line
        if let Some(id) = &self.identity {
            sections.push(format!("## Identity\nYou are {}.", id.display_name()));
        }

        // Soul section
        if let Some(soul) = &self.soul {
            let s = soul.to_prompt_section();
            if !s.is_empty() {
                sections.push(s);
            }
        }

        // Memory recall section (aligned with Node system-prompt.ts)
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
    if let Some(rt) = runtime {
        builder = builder.with_runtime(rt);
    }
    if !tools.is_empty() {
        builder = builder.with_available_tools(tools.to_vec());
    }

    Ok(builder.build())
}