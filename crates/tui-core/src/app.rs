use std::io::{self, stdout};
use std::time::Duration;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    execute,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap, Clear, List, ListItem};
use tokio::sync::mpsc;

use crate::chat::{ChatLog, ChatMessage, SystemLevel, ToolStatus};
use crate::commands::{self, SlashCommand};
use crate::editor::TextEditor;
use crate::gateway::{GatewayClient, GatewayConfig, GatewayEvent};
use crate::overlay::{SelectList, SelectItem};
use crate::theme::Theme;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus { Editor, Chat }

enum OverlayTarget { Model, Session }

pub struct TuiApp {
    config: TuiConfig,
    theme: Theme,
    chat: ChatLog,
    editor: TextEditor,
    gateway: GatewayClient,
    model_picker: SelectList,
    session_picker: SelectList,
    focus: Focus,
    streaming: bool,
    connected: bool,
    verbose: bool,
    status_msg: String,
    should_quit: bool,
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
            theme: Theme::default(),
            chat: ChatLog::new(500),
            editor: TextEditor::new(),
            gateway: GatewayClient::new(gw_config),
            model_picker: SelectList::new("Select Model"),
            session_picker: SelectList::new("Select Session"),
            focus: Focus::Editor,
            streaming: false,
            connected: false,
            verbose: false,
            status_msg: String::new(),
            should_quit: false,
        }
    }

    pub async fn run(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout());
        let mut terminal = Terminal::new(backend)?;

        self.chat.add_system(
            "Welcome to OpenClaw TUI. Type /help for commands.",
            SystemLevel::Info,
        );

        let (tx, mut rx) = mpsc::channel::<GatewayEvent>(256);
        self.try_connect(&tx).await;

        loop {
            terminal.draw(|f| self.render(f))?;

            if event::poll(Duration::from_millis(30))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key_event(key, &tx).await;
                }
            }

            while let Ok(ev) = rx.try_recv() {
                self.handle_gateway_event(ev);
            }

            if self.should_quit {
                break;
            }
        }

        disable_raw_mode()?;
        execute!(stdout(), LeaveAlternateScreen)?;
        Ok(())
    }

    async fn try_connect(&mut self, tx: &mpsc::Sender<GatewayEvent>) {
        self.status_msg = "Connecting...".to_string();
        match self.gateway.health().await {
            Ok(true) => {
                self.connected = true;
                self.status_msg = "Connected".to_string();
                let _ = tx.send(GatewayEvent::Connected).await;
                self.load_models().await;
                self.load_sessions().await;
            }
            _ => {
                self.connected = false;
                self.status_msg = format!("Disconnected: {}", self.config.gateway_url);
                self.chat.add_system(
                    &format!("Failed to connect to gateway at {}", self.config.gateway_url),
                    SystemLevel::Error,
                );
            }
        }
    }

    async fn load_models(&mut self) {
        if let Ok(models) = self.gateway.list_models().await {
            let items: Vec<SelectItem> = models.iter().map(|m| SelectItem {
                label: m.clone(),
                description: String::new(),
                value: m.clone(),
            }).collect();
            self.model_picker.set_items(items);
        }
    }

    async fn load_sessions(&mut self) {
        if let Ok(sessions) = self.gateway.list_sessions().await {
            let items: Vec<SelectItem> = sessions.iter().map(|s| SelectItem {
                label: s.key.clone(),
                description: format!("{} messages", s.message_count),
                value: s.key.clone(),
            }).collect();
            self.session_picker.set_items(items);
        }
    }

    async fn handle_key_event(&mut self, key: KeyEvent, tx: &mpsc::Sender<GatewayEvent>) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        if self.model_picker.is_visible() {
            self.handle_overlay_key(key, OverlayTarget::Model);
            return;
        }
        if self.session_picker.is_visible() {
            self.handle_overlay_key(key, OverlayTarget::Session);
            return;
        }
        if key.code == KeyCode::Tab {
            self.focus = match self.focus {
                Focus::Editor => Focus::Chat,
                Focus::Chat => Focus::Editor,
            };
            return;
        }
        match self.focus {
            Focus::Editor => self.handle_editor_key(key, tx).await,
            Focus::Chat => self.handle_chat_key(key),
        }
    }

    async fn handle_editor_key(&mut self, key: KeyEvent, tx: &mpsc::Sender<GatewayEvent>) {
        match key.code {
            KeyCode::Enter if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                if self.editor.is_empty() { return; }
                let text = self.editor.submit();
                self.process_input(&text, tx).await;
            }
            KeyCode::Enter => self.editor.insert_newline(),
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.editor.kill_line();
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.editor.move_home();
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.editor.move_end();
            }
            KeyCode::Left => self.editor.move_left(),
            KeyCode::Right => self.editor.move_right(),
            KeyCode::Up if self.editor.line_count() == 1 => self.editor.history_up(),
            KeyCode::Up => self.editor.move_up(),
            KeyCode::Down if self.editor.line_count() == 1 => self.editor.history_down(),
            KeyCode::Down => self.editor.move_down(),
            KeyCode::Home => self.editor.move_home(),
            KeyCode::End => self.editor.move_end(),
            KeyCode::Backspace => self.editor.backspace(),
            KeyCode::Delete => self.editor.delete(),
            KeyCode::Char(c) => self.editor.insert_char(c),
            KeyCode::Esc => self.focus = Focus::Chat,
            _ => {}
        }
    }

    fn handle_chat_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.chat.scroll_up(1),
            KeyCode::Down | KeyCode::Char('j') => self.chat.scroll_down(1),
            KeyCode::PageUp => self.chat.scroll_up(10),
            KeyCode::PageDown => self.chat.scroll_down(10),
            KeyCode::Home => self.chat.scroll_up(usize::MAX / 2),
            KeyCode::End => self.chat.scroll_to_bottom(),
            KeyCode::Char('i') | KeyCode::Esc => self.focus = Focus::Editor,
            _ => {}
        }
    }

    fn handle_overlay_key(&mut self, key: KeyEvent, target: OverlayTarget) {
        let picker = match target {
            OverlayTarget::Model => &mut self.model_picker,
            OverlayTarget::Session => &mut self.session_picker,
        };
        match key.code {
            KeyCode::Esc => picker.hide(),
            KeyCode::Up => picker.move_up(),
            KeyCode::Down => picker.move_down(),
            KeyCode::Backspace => picker.backspace(),
            KeyCode::Char(c) => picker.type_char(c),
            KeyCode::Enter => {
                if let Some(item) = picker.confirm() {
                    match target {
                        OverlayTarget::Model => {
                            self.gateway.set_model(&item.value);
                            self.status_msg = format!("Model: {}", item.value);
                        }
                        OverlayTarget::Session => {
                            self.gateway.set_session(&item.value);
                            self.status_msg = format!("Session: {}", item.value);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    async fn process_input(&mut self, text: &str, tx: &mpsc::Sender<GatewayEvent>) {
        if let Some(result) = commands::parse_command(text) {
            if let Some(msg) = &result.message {
                self.chat.add_system(msg, SystemLevel::Warning);
            }
            self.handle_command(result.command, tx).await;
            return;
        }
        self.chat.add_user(text);
        self.send_message(text, tx).await;
    }

    async fn handle_command(&mut self, cmd: SlashCommand, _tx: &mpsc::Sender<GatewayEvent>) {
        match cmd {
            SlashCommand::Help => {
                self.chat.add_system(
                    "/help /clear /exit /status /model [name] /session [name] /think [on|off] /verbose /usage /abort",
                    SystemLevel::Info,
                );
            }
            SlashCommand::Clear => {
                self.chat.clear();
                self.chat.add_system("Chat cleared.", SystemLevel::Info);
            }
            SlashCommand::Exit => self.should_quit = true,
            SlashCommand::Status => {
                let conn = if self.connected { "Connected" } else { "Disconnected" };
                let model = self.gateway.config().model.clone();
                let session = self.gateway.config().session.clone();
                self.chat.add_system(
                    &format!("{} | Model: {} | Session: {}", conn, model, session),
                    SystemLevel::Info,
                );
            }
            SlashCommand::Model(None) => self.model_picker.show(),
            SlashCommand::Model(Some(name)) => {
                self.gateway.set_model(&name);
                self.status_msg = format!("Model: {}", name);
            }
            SlashCommand::Session(None) => self.session_picker.show(),
            SlashCommand::Session(Some(name)) => {
                self.gateway.set_session(&name);
                self.status_msg = format!("Session: {}", name);
            }
            SlashCommand::Think(arg) => {
                let msg = match arg.as_deref() {
                    Some("on") | Some("true") => "Thinking enabled",
                    Some("off") | Some("false") => "Thinking disabled",
                    _ => "Usage: /think [on|off]",
                };
                self.chat.add_system(msg, SystemLevel::Info);
            }
            SlashCommand::Verbose => {
                self.verbose = !self.verbose;
                let state = if self.verbose { "on" } else { "off" };
                self.chat.add_system(&format!("Verbose: {}", state), SystemLevel::Info);
            }
            SlashCommand::Usage => {
                self.chat.add_system("Usage stats not yet implemented.", SystemLevel::Info);
            }
            SlashCommand::Abort => {
                if self.streaming {
                    self.streaming = false;
                    self.chat.finish_assistant();
                    self.chat.add_system("Aborted.", SystemLevel::Warning);
                }
            }
        }
    }

    async fn send_message(&mut self, text: &str, tx: &mpsc::Sender<GatewayEvent>) {
        if !self.connected {
            self.chat.add_system("Not connected to gateway.", SystemLevel::Error);
            return;
        }
        self.streaming = true;
        self.chat.start_assistant(&self.gateway.config().model);
        let tx2 = tx.clone();
        let gw = self.gateway.config().clone();
        let client = GatewayClient::new(gw);
        let msg = text.to_string();
        tokio::spawn(async move {
            if let Err(e) = client.send_message(&msg, tx2.clone()).await {
                let _ = tx2.send(GatewayEvent::Error(e)).await;
            }
        });
    }

    fn handle_gateway_event(&mut self, ev: GatewayEvent) {
        match ev {
            GatewayEvent::Connected => {
                self.connected = true;
                self.status_msg = "Connected".to_string();
            }
            GatewayEvent::Chunk(text) => {
                self.chat.append_assistant_chunk(&text);
            }
            GatewayEvent::Done(_full) => {
                self.streaming = false;
                self.chat.finish_assistant();
            }
            GatewayEvent::Error(err) => {
                self.streaming = false;
                self.chat.finish_assistant();
                self.chat.add_system(&format!("Error: {}", err), SystemLevel::Error);
            }
            GatewayEvent::ModelsLoaded(models) => {
                let items: Vec<SelectItem> = models.iter().map(|m| SelectItem {
                    label: m.clone(),
                    description: String::new(),
                    value: m.clone(),
                }).collect();
                self.model_picker.set_items(items);
            }
            GatewayEvent::SessionsLoaded(sessions) => {
                let items: Vec<SelectItem> = sessions.iter().map(|s| SelectItem {
                    label: s.key.clone(),
                    description: format!("{} msgs", s.message_count),
                    value: s.key.clone(),
                }).collect();
                self.session_picker.set_items(items);
            }
        }
    }

    // ── Rendering ───────────────────────────────────────────────────

    fn render(&self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),   // header
                Constraint::Min(5),      // chat
                Constraint::Length(3),   // editor
                Constraint::Length(1),   // status
            ])
            .split(frame.area());

        self.render_header(frame, chunks[0]);
        self.render_chat(frame, chunks[1]);
        self.render_editor(frame, chunks[2]);
        self.render_status(frame, chunks[3]);

        // Overlay on top
        if self.model_picker.is_visible() {
            self.render_overlay(frame, &self.model_picker);
        }
        if self.session_picker.is_visible() {
            self.render_overlay(frame, &self.session_picker);
        }
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let conn_icon = if self.connected { "●" } else { "○" };
        let model = &self.gateway.config().model;
        let session = &self.gateway.config().session;
        let header_text = format!(
            " {} {} | {} | session: {}",
            conn_icon, self.config.title, model, session
        );
        let header = Paragraph::new(header_text).style(self.theme.header_style());
        frame.render_widget(header, area);
    }

    fn render_chat(&self, frame: &mut Frame, area: Rect) {
        let border_style = if self.focus == Focus::Chat {
            self.theme.border_focus_style()
        } else {
            self.theme.border_style()
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Chat ");

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.chat.is_empty() {
            let hint = Paragraph::new("No messages yet.")
                .style(self.theme.muted_style());
            frame.render_widget(hint, inner);
            return;
        }

        let mut lines: Vec<Line> = Vec::new();
        for msg in self.chat.messages().iter() {
            match msg {
                ChatMessage::User { text, .. } => {
                    lines.push(Line::from(vec![
                        Span::styled("You: ", self.theme.user_style()),
                        Span::styled(text.as_str(), self.theme.style()),
                    ]));
                }
                ChatMessage::Assistant { text, streaming, .. } => {
                    let suffix = if *streaming { " ▍" } else { "" };
                    lines.push(Line::from(vec![
                        Span::styled("AI: ", self.theme.assistant_style()),
                        Span::styled(
                            format!("{}{}", text, suffix),
                            self.theme.style(),
                        ),
                    ]));
                }
                ChatMessage::ToolCall { name, status, .. } => {
                    let (icon, style) = match status {
                        ToolStatus::Pending => ("◌", self.theme.muted_style()),
                        ToolStatus::Running => ("⟳", self.theme.accent_style()),
                        ToolStatus::Success => ("✓", self.theme.success_style()),
                        ToolStatus::Error(_) => ("✗", self.theme.error_style()),
                    };
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {} ", icon), style),
                        Span::styled(name.as_str(), style),
                    ]));
                }
                ChatMessage::System { text, level } => {
                    let style = match level {
                        SystemLevel::Info => self.theme.accent_style(),
                        SystemLevel::Warning => Style::default().fg(self.theme.warning),
                        SystemLevel::Error => self.theme.error_style(),
                    };
                    lines.push(Line::from(Span::styled(text.as_str(), style)));
                }
            }
        }

        // Apply scroll offset (scroll from bottom)
        let visible_height = inner.height as usize;
        let total = lines.len();
        let end = total.saturating_sub(self.chat.scroll_offset());
        let start = end.saturating_sub(visible_height);
        let visible: Vec<Line> = lines[start..end].to_vec();

        let paragraph = Paragraph::new(visible).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
    }

    fn render_editor(&self, frame: &mut Frame, area: Rect) {
        let border_style = if self.focus == Focus::Editor {
            self.theme.border_focus_style()
        } else {
            self.theme.border_style()
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Input (Enter to send, Shift+Enter for newline) ");

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let text: String = self.editor.lines().join("\n");
        let editor_para = Paragraph::new(text).style(self.theme.style());
        frame.render_widget(editor_para, inner);

        // Place cursor
        if self.focus == Focus::Editor {
            let (row, col) = self.editor.cursor();
            frame.set_cursor_position((
                inner.x + col as u16,
                inner.y + row as u16,
            ));
        }
    }

    fn render_status(&self, frame: &mut Frame, area: Rect) {
        let focus_hint = match self.focus {
            Focus::Editor => "Tab: chat | Esc: chat | /help",
            Focus::Chat => "Tab: editor | i: editor | j/k: scroll",
        };
        let streaming_hint = if self.streaming { " [streaming...]" } else { "" };
        let status_text = format!(
            " {} {} | {}",
            self.status_msg, streaming_hint, focus_hint
        );
        let status = Paragraph::new(status_text).style(self.theme.status_style());
        frame.render_widget(status, area);
    }

    fn render_overlay(&self, frame: &mut Frame, picker: &SelectList) {
        let area = frame.area();
        let width = (area.width / 2).max(30).min(area.width - 4);
        let height = (area.height / 2).max(10).min(area.height - 4);
        let x = (area.width - width) / 2;
        let y = (area.height - height) / 2;
        let popup_area = Rect::new(x, y, width, height);

        frame.render_widget(Clear, popup_area);

        let title = format!(" {} ", picker.title());
        let filter = picker.filter_text();
        let filter_line = if filter.is_empty() {
            "Type to filter...".to_string()
        } else {
            format!("Filter: {}", filter)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(self.theme.border_focus_style())
            .title(title);

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        // Filter line at top
        let filter_para = Paragraph::new(filter_line)
            .style(self.theme.muted_style());

        let filter_area = Rect::new(
            inner.x, inner.y, inner.width, 1,
        );
        frame.render_widget(filter_para, filter_area);

        // Items list below filter
        let list_area = Rect::new(
            inner.x,
            inner.y + 1,
            inner.width,
            inner.height.saturating_sub(1),
        );

        let items: Vec<ListItem> = picker
            .filtered_items()
            .iter()
            .map(|(vi, item)| {
                let style = if *vi == picker.selected_index() {
                    self.theme.accent_style()
                } else {
                    self.theme.style()
                };
                let prefix = if *vi == picker.selected_index() {
                    "> "
                } else {
                    "  "
                };
                let text = if item.description.is_empty() {
                    format!("{}{}", prefix, item.label)
                } else {
                    format!("{}{} - {}", prefix, item.label, item.description)
                };
                ListItem::new(text).style(style)
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, list_area);
    }
}