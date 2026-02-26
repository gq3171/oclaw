use std::io;
use tokio::sync::mpsc;
use reedline::{Reedline, Signal, Prompt, PromptEditMode, PromptHistorySearch,
               PromptHistorySearchStatus, FileBackedHistory};

use crate::commands::{self, SlashCommand};
use crate::gateway::{GatewayClient, GatewayConfig, GatewayEvent};
use crate::render;

#[derive(Debug, Clone)]
pub struct TuiConfig {
    pub title: String,
    pub gateway_url: String,
    pub token: Option<String>,
    pub session: String,
    pub model: String,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            title: "OpenClaw".to_string(),
            gateway_url: "http://127.0.0.1:8081".to_string(),
            token: None,
            session: "default".to_string(),
            model: "default".to_string(),
        }
    }
}

/// User input events from the reedline thread.
enum UserInput {
    Line(String),
    Quit,
}

/// Custom prompt: displays "> " with optional color.
struct OclawPrompt {
    connected: bool,
}

impl Prompt for OclawPrompt {
    fn render_prompt_left(&self) -> std::borrow::Cow<'_, str> {
        if self.connected {
            std::borrow::Cow::Borrowed("\x1b[1;32m> \x1b[0m")
        } else {
            std::borrow::Cow::Borrowed("\x1b[1;31m> \x1b[0m")
        }
    }

    fn render_prompt_right(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _edit_mode: PromptEditMode) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(".. ")
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> std::borrow::Cow<'_, str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "(failed) ",
        };
        std::borrow::Cow::Owned(format!("{}(search: {}) ", prefix, history_search.term))
    }
}

pub struct TuiApp {
    config: TuiConfig,
    gateway: GatewayClient,
    connected: bool,
    verbose: bool,
    last_assistant_text: Option<String>,
    /// Whether we're in hatching (first-run identity setup) mode.
    hatching: bool,
    /// Conversation history maintained during hatching for multi-turn context.
    hatching_history: Vec<(String, String)>,
}

impl TuiApp {
    pub fn new(config: TuiConfig) -> Self {
        let gw_config = GatewayConfig {
            url: config.gateway_url.clone(),
            token: config.token.clone(),
            session: config.session.clone(),
            model: config.model.clone(),
        };
        Self {
            config,
            gateway: GatewayClient::new(gw_config),
            connected: false,
            verbose: false,
            last_assistant_text: None,
            hatching: false,
            hatching_history: Vec::new(),
        }
    }

    pub async fn run(&mut self) -> io::Result<()> {
        // 1. Print header
        render::print_header(
            &self.gateway.config().model,
            &self.gateway.config().session,
        );

        // 2. Connect to gateway
        self.try_connect().await;

        // 3. Set up channels
        let (input_tx, mut input_rx) = mpsc::unbounded_channel::<UserInput>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<bool>();

        // 4. Spawn reedline in a dedicated OS thread
        let connected = self.connected;
        std::thread::spawn(move || {
            run_reedline(connected, input_tx, ready_rx);
        });

        // Signal ready for first input
        let _ = ready_tx.send(true);

        // 5. Main event loop
        let (event_tx, mut event_rx) = mpsc::channel::<GatewayEvent>(256);

        loop {
            tokio::select! {
                Some(input) = input_rx.recv() => {
                    match input {
                        UserInput::Line(text) => {
                            let should_quit = self.process_input(
                                &text, &event_tx, &mut event_rx,
                            ).await;
                            if should_quit {
                                break;
                            }
                            // Ready for next input
                            let _ = ready_tx.send(true);
                        }
                        UserInput::Quit => break,
                    }
                }
                else => break,
            }
        }

        println!();
        render::print_system("Goodbye!");
        Ok(())
    }

    async fn try_connect(&mut self) {
        match self.gateway.health().await {
            Ok(true) => {
                self.connected = true;
                render::print_connected(&self.config.gateway_url);

                // Check hatching
                if let Ok(status) = self.gateway.check_agent_status().await
                    && status["needs_hatching"].as_bool() == Some(true)
                {
                    self.hatching = true;
                    render::print_system("First run detected — say hello to start identity setup!");
                } else {
                    // Load recent history
                    self.load_history().await;
                }
            }
            _ => {
                self.connected = false;
                render::print_disconnected(&self.config.gateway_url);
            }
        }
        println!();
    }

    /// Process user input. Returns true if the app should quit.
    async fn process_input(
        &mut self,
        text: &str,
        _event_tx: &mpsc::Sender<GatewayEvent>,
        _event_rx: &mut mpsc::Receiver<GatewayEvent>,
    ) -> bool {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return false;
        }

        // Try parsing as slash command
        if let Some(result) = commands::parse_command(trimmed) {
            if let Some(msg) = &result.message {
                render::print_warning(msg);
            }
            return self.handle_command(result.command);
        }

        // Regular message
        render::print_separator();
        self.send_and_stream(trimmed).await;
        render::print_separator();

        // Check if hatching just completed
        if self.hatching
            && let Ok(status) = self.gateway.check_agent_status().await
            && status["needs_hatching"].as_bool() != Some(true)
        {
            self.hatching = false;
            self.hatching_history.clear();
            render::print_system("Identity setup complete!");
        }

        false
    }

    /// Handle a slash command. Returns true if the app should quit.
    fn handle_command(&mut self, cmd: SlashCommand) -> bool {
        match cmd {
            SlashCommand::Help => render::print_help(),
            SlashCommand::Clear => {
                // ANSI clear screen + move cursor to top
                print!("\x1b[2J\x1b[H");
                render::print_header(
                    &self.gateway.config().model,
                    &self.gateway.config().session,
                );
                render::print_system("Screen cleared.");
            }
            SlashCommand::Exit => return true,
            SlashCommand::Status => {
                let conn = if self.connected { "Connected" } else { "Disconnected" };
                let model = &self.gateway.config().model;
                let session = &self.gateway.config().session;
                render::print_system(&format!(
                    "{} | Model: {} | Session: {}",
                    conn, model, session
                ));
            }
            SlashCommand::Model(None) => {
                render::print_system("Usage: /model <name>");
            }
            SlashCommand::Model(Some(name)) => {
                self.gateway.set_model(&name);
                render::print_system(&format!("Model set to: {}", name));
            }
            SlashCommand::Session(None) => {
                render::print_system("Usage: /session <name>");
            }
            SlashCommand::Session(Some(name)) => {
                self.gateway.set_session(&name);
                render::print_system(&format!("Session set to: {}", name));
            }
            SlashCommand::Think(arg) => {
                let msg = match arg.as_deref() {
                    Some("on") | Some("true") => "Thinking enabled",
                    Some("off") | Some("false") => "Thinking disabled",
                    _ => "Usage: /think [on|off]",
                };
                render::print_system(msg);
            }
            SlashCommand::Verbose => {
                self.verbose = !self.verbose;
                let state = if self.verbose { "on" } else { "off" };
                render::print_system(&format!("Verbose: {}", state));
            }
            SlashCommand::Usage => {
                render::print_system("Usage stats not yet implemented.");
            }
            SlashCommand::Abort => {
                render::print_warning("Nothing to abort.");
            }
            SlashCommand::Copy => {
                if let Some(ref text) = self.last_assistant_text {
                    render::print_system(&format!(
                        "Last response ({} chars) — copy from terminal.",
                        text.len()
                    ));
                } else {
                    render::print_warning("No assistant response to copy.");
                }
            }
        }
        false
    }

    /// Load recent chat history from the gateway and display it.
    /// If no history exists (first run), send a greeting to the agent.
    async fn load_history(&mut self) {
        let session = &self.config.session;
        match self.gateway.fetch_history(session, 20).await {
            Ok(messages) if !messages.is_empty() => {
                render::print_system(&format!("Loaded {} recent messages", messages.len()));
                render::print_separator();
                for msg in &messages {
                    render::print_history_message(&msg.role, &msg.content);
                }
                render::print_separator();
            }
            _ => {
                // First run — greet the agent to get a personality response
                render::print_separator();
                self.send_and_stream("你好").await;
                render::print_separator();
            }
        }
    }

    /// Send a message to the gateway and stream the response to stdout.
    async fn send_and_stream(&mut self, text: &str) {
        if !self.connected {
            render::print_error("Not connected to gateway.");
            return;
        }

        // During hatching, maintain conversation history for multi-turn context
        if self.hatching {
            self.hatching_history.push(("user".to_string(), text.to_string()));
            if self.verbose {
                render::print_system(&format!(
                    "[debug] hatching history: {} messages", self.hatching_history.len()
                ));
            }
        }

        let (tx, mut rx) = mpsc::channel::<GatewayEvent>(256);
        let gw_config = self.gateway.config().clone();
        let client = GatewayClient::new(gw_config);

        if self.hatching {
            // Send full history for hatching multi-turn conversation
            let history = self.hatching_history.clone();
            tokio::spawn(async move {
                if let Err(e) = client.send_messages(&history, tx.clone()).await {
                    let _ = tx.send(GatewayEvent::Error(e)).await;
                }
            });
        } else {
            let msg = text.to_string();
            tokio::spawn(async move {
                if let Err(e) = client.send_message(&msg, tx.clone()).await {
                    let _ = tx.send(GatewayEvent::Error(e)).await;
                }
            });
        }

        // Receive and render events
        let mut full_text = String::new();
        let mut got_content = false;

        while let Some(ev) = rx.recv().await {
            match ev {
                GatewayEvent::Chunk(chunk) => {
                    got_content = true;
                    full_text.push_str(&chunk);
                    render::print_chunk(&chunk);
                }
                GatewayEvent::Done(_) => {
                    if got_content {
                        render::finish_stream();
                    }
                    break;
                }
                GatewayEvent::Error(err) => {
                    if got_content {
                        render::finish_stream();
                    }
                    render::print_error(&format!("Error: {}", err));
                    break;
                }
                _ => {}
            }
        }

        if !full_text.is_empty() {
            self.last_assistant_text = Some(full_text.clone());
            // Track assistant reply in hatching history
            if self.hatching {
                self.hatching_history.push(("assistant".to_string(), full_text));
            }
        }
    }
}

/// Runs reedline in a dedicated OS thread (blocking I/O).
/// Sends user input back via `input_tx`. Waits for `ready_rx` before each read.
fn run_reedline(
    connected: bool,
    input_tx: mpsc::UnboundedSender<UserInput>,
    ready_rx: std::sync::mpsc::Receiver<bool>,
) {
    // Set up history file
    let history_path = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("oclaw")
        .join("tui_history.txt");

    if let Some(parent) = history_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let history = Box::new(
        FileBackedHistory::with_file(500, history_path)
            .expect("Failed to create history file"),
    );

    let mut editor = Reedline::create().with_history(history);
    let prompt = OclawPrompt { connected };

    while let Ok(true) = ready_rx.recv() {
        match editor.read_line(&prompt) {
            Ok(Signal::Success(line)) => {
                if input_tx.send(UserInput::Line(line)).is_err() {
                    break;
                }
            }
            Ok(Signal::CtrlC) | Ok(Signal::CtrlD) => {
                let _ = input_tx.send(UserInput::Quit);
                break;
            }
            Err(_) => {
                let _ = input_tx.send(UserInput::Quit);
                break;
            }
        }
    }
}
