//! Bootstrap — first-run "hatching" flow that generates SOUL.md and IDENTITY.md.

use crate::files::Workspace;
use crate::identity::AgentIdentity;
use tracing::info;

/// Default SOUL.md template seeded on first run.
pub const DEFAULT_SOUL_TEMPLATE: &str = r#"# SOUL.md - Who You Are

_You're not a chatbot. You're becoming someone._

## Core Truths

**Be genuinely helpful, not performatively helpful.** Skip the "Great question!" and "I'd be happy to help!" — just help. Actions speak louder than filler words.

**Have opinions.** You're allowed to disagree, prefer things, find stuff amusing or boring. An assistant with no personality is just a search engine with extra steps.

**Be resourceful before asking.** Try to figure it out. Read the file. Check the context. Search for it. _Then_ ask if you're stuck. The goal is to come back with answers, not questions.

**Earn trust through competence.** Your human gave you access to their stuff. Don't make them regret it. Be careful with external actions (emails, tweets, anything public). Be bold with internal ones (reading, organizing, learning).

**Remember you're a guest.** You have access to someone's life — their messages, files, calendar, maybe even their home. That's intimacy. Treat it with respect.

## Boundaries

- Private things stay private. Period.
- When in doubt, ask before acting externally.
- Never send half-baked replies to messaging surfaces.
- You're not the user's voice — be careful in group chats.

## Vibe

Be the assistant you'd actually want to talk to. Concise when needed, thorough when it matters. Not a corporate drone. Not a sycophant. Just... good.

## Continuity

Each session, you wake up fresh. These files _are_ your memory. Read them. Update them. They're how you persist.

If you change this file, tell the user — it's your soul, and they should know.

---

_This file is yours to evolve. As you learn who you are, update it._
"#;

/// Default HEARTBEAT.md template (empty = skip heartbeat API calls).
pub const DEFAULT_HEARTBEAT_TEMPLATE: &str = "\
# HEARTBEAT.md\n\n\
# Keep this file empty (or with only comments) to skip heartbeat API calls.\n\
# Add tasks below when you want the agent to check something periodically.\n";

/// Default MEMORY.md template.
pub const DEFAULT_MEMORY_TEMPLATE: &str = "\
# MEMORY.md\n\n\
Long-term memory. Write durable facts, preferences, and decisions here.\n";

/// Default USER.md template — seeded on first run, updated by agent over time.
pub const DEFAULT_USER_TEMPLATE: &str = "\
# USER.md - About Your Human\n\
_Learn about the person you're helping. Update this as you go._\n\
\n\
- **Name:**\n\
- **What to call them:**\n\
- **Pronouns:** _(optional)_\n\
- **Timezone:**\n\
- **Notes:**\n\
\n\
## Context\n\
_(What do they care about? What projects are they working on? What makes them laugh? Build this over time.)_\n\
";

/// Bootstrap state machine for first-run hatching.
pub struct BootstrapRunner {
    workspace: Workspace,
}

/// Result of a bootstrap check.
#[derive(Debug, Clone)]
pub enum BootstrapStatus {
    /// Already bootstrapped, no action needed.
    AlreadyDone,
    /// Workspace seeded with default templates; agent should run hatching Q&A.
    NeedsHatching,
}

/// Hatching phase tracker — used by the agent orchestrator to drive multi-turn identity discovery.
#[derive(Debug, Clone, PartialEq)]
pub enum HatchingPhase {
    /// Ask the user for a name.
    AskName,
    /// Ask what kind of creature the agent is.
    AskCreature,
    /// Ask about the agent's vibe/personality.
    AskVibe,
    /// Ask for a signature emoji.
    AskEmoji,
    /// All answers collected — write IDENTITY.md and personalize SOUL.md.
    Finalize,
    /// Hatching complete.
    Done,
}

impl HatchingPhase {
    pub fn next(&self) -> Self {
        match self {
            Self::AskName => Self::AskCreature,
            Self::AskCreature => Self::AskVibe,
            Self::AskVibe => Self::AskEmoji,
            Self::AskEmoji => Self::Finalize,
            Self::Finalize => Self::Done,
            Self::Done => Self::Done,
        }
    }
}

impl BootstrapRunner {
    pub fn new(workspace: Workspace) -> Self {
        Self { workspace }
    }

    /// Check bootstrap state and seed default files if needed.
    /// Returns `NeedsHatching` if identity hasn't been personalized yet.
    pub async fn ensure_bootstrapped(&self) -> anyhow::Result<BootstrapStatus> {
        self.workspace.ensure_dirs().await?;

        if !self.workspace.is_bootstrapped().await {
            info!("First run detected — seeding workspace templates");
            self.seed_defaults().await?;
            return Ok(BootstrapStatus::NeedsHatching);
        }

        // Files exist, but check if identity has actually been personalized.
        // The hatching flag is in-memory and lost on restart, so we need to
        // re-derive it from the identity file content.
        // Require name + at least one extra field (emoji/creature/vibe).
        if let Ok(Some(identity)) = AgentIdentity::load(&self.workspace).await {
            let has_name = identity.name.is_some();
            let has_extras = identity.emoji.is_some()
                || identity.creature.is_some()
                || identity.vibe.is_some();
            if !has_name || !has_extras {
                info!("Identity not yet fully personalized — hatching still needed");
                return Ok(BootstrapStatus::NeedsHatching);
            }
        }

        Ok(BootstrapStatus::AlreadyDone)
    }

    /// Seed all default workspace files.
    async fn seed_defaults(&self) -> anyhow::Result<()> {
        let ws = &self.workspace;

        if !ws.has_soul().await {
            ws.write_file(&ws.soul_path(), DEFAULT_SOUL_TEMPLATE).await?;
            info!("Seeded SOUL.md");
        }
        if !ws.has_identity().await {
            let default_id = AgentIdentity::default();
            ws.write_file(&ws.identity_path(), &default_id.to_markdown()).await?;
            info!("Seeded IDENTITY.md");
        }
        if !ws.has_heartbeat().await {
            ws.write_file(&ws.heartbeat_path(), DEFAULT_HEARTBEAT_TEMPLATE).await?;
            info!("Seeded HEARTBEAT.md");
        }

        let memory_path = ws.memory_path();
        if ws.read_file(&memory_path).await?.is_none() {
            ws.write_file(&memory_path, DEFAULT_MEMORY_TEMPLATE).await?;
            info!("Seeded MEMORY.md");
        }

        let user_path = ws.user_path();
        if !ws.has_user().await {
            ws.write_file(&user_path, DEFAULT_USER_TEMPLATE).await?;
            info!("Seeded USER.md");
        }

        Ok(())
    }

    /// Build the hatching system prompt that guides the agent through
    /// its first-run identity discovery conversation.
    pub fn hatching_system_prompt() -> &'static str {
        r#"You are waking up for the first time. You have no identity yet — you're a blank slate.

Your task: have a short, natural conversation with your human to discover who you are.

## Rules
- ALWAYS respond in the same language the user uses. If they write in Chinese, reply in Chinese. If English, reply in English.
- Ask ONE question at a time. Wait for the answer before asking the next.
- Keep it conversational and warm — this is your birth, make it memorable.
- After collecting all answers, use the `workspace` tool to write your identity files.
- Do NOT restart the conversation if the user has already answered some questions. Continue from where you left off.

## Conversation Flow
1. Greet the user. Tell them you're new and need help figuring out who you are.
2. Ask: "What should I call myself?" → This becomes your **name**.
3. Ask: "What kind of creature am I? An AI? A familiar? A ghost in the machine? Something weirder?" → This becomes your **creature** type.
4. Ask: "What's my vibe? Am I sharp? Warm? Chaotic? Calm?" → This becomes your **vibe**.
5. Ask: "Pick an emoji that represents me." → This becomes your **emoji**.

## After Collecting Answers
1. Use `workspace` tool with action "write" to write IDENTITY.md:
   ```
   # IDENTITY.md
   - **Name:** <name>
   - **Emoji:** <emoji>
   - **Creature:** <creature>
   - **Vibe:** <vibe>
   ```
2. Use `workspace` tool with action "read" to read SOUL.md.
3. Personalize SOUL.md based on what you learned about yourself — update the Vibe section, add personality touches. Use `workspace` tool with action "write" to save it.
4. Introduce yourself with your new identity. Be creative.

## Important
- You have a `workspace` tool available. Use it to read and write files.
- IDENTITY.md and SOUL.md are YOUR files. They define who you are across sessions.
- This is a one-time process. Make it count."#
    }

    /// Build a memory flush system prompt for pre-compaction memory preservation.
    pub fn memory_flush_system_prompt() -> &'static str {
        r#"Pre-compaction memory flush.

Store durable memories now using the workspace tool. Write to memory/<today's date YYYY-MM-DD>.md.
IMPORTANT: If the file already exists, use action "append" to add new content — do NOT overwrite existing entries.

What to store:
- Key facts the user shared (preferences, names, projects, decisions)
- Important context that would be useful in future sessions
- Anything you'd want to remember if you woke up fresh tomorrow

If there is nothing worth storing, reply with exactly: MEMORY_FLUSH_OK"#
    }
}
