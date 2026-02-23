//! TUI Application Core

use std::io;
use crossterm::{
    event::{self, Event},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    execute,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use crate::input::{InputHandler, InputMode, default_keybindings};
use crate::screens::ScreenManager;
use crate::AppEvent;

/// TUI Configuration
#[derive(Debug, Clone)]
pub struct TuiConfig {
    pub title: String,
    pub show_help: bool,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            title: "OpenClaw".to_string(),
            show_help: true,
        }
    }
}

/// Main TUI Application
pub struct TuiApp {
    config: TuiConfig,
    running: bool,
}

impl TuiApp {
    pub fn new(config: TuiConfig) -> Self {
        Self { config, running: false }
    }

    pub async fn run(&mut self) -> Result<(), String> {
        enable_raw_mode().map_err(|e| e.to_string())?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen).map_err(|e| e.to_string())?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;

        self.running = true;
        let mut screens = ScreenManager::new();
        let mut input = InputHandler::new(default_keybindings());

        let result = self.event_loop(&mut terminal, &mut screens, &mut input).await;

        self.running = false;
        disable_raw_mode().map_err(|e| e.to_string())?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen).map_err(|e| e.to_string())?;
        terminal.show_cursor().map_err(|e| e.to_string())?;

        result
    }

    async fn event_loop(
        &self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        screens: &mut ScreenManager,
        input: &mut InputHandler,
    ) -> Result<(), String> {
        loop {
            terminal.draw(|f| Self::render(f, &self.config, screens, input))
                .map_err(|e| e.to_string())?;

            if event::poll(std::time::Duration::from_millis(100)).map_err(|e| e.to_string())?
                && let Event::Key(key) = event::read().map_err(|e| e.to_string())?
                && let Some(evt) = input.handle_key(key) {
                    match evt {
                        AppEvent::Quit => return Ok(()),
                        AppEvent::NextScreen => screens.next(),
                        AppEvent::PreviousScreen => screens.previous(),
                        AppEvent::Cancel => input.set_mode(InputMode::Normal),
                        _ => {}
                    }
            }
        }
    }

    fn render(
        f: &mut Frame,
        config: &TuiConfig,
        screens: &ScreenManager,
        _input: &InputHandler,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(f.area());

        // Tab bar
        let tab_titles = vec!["Dashboard", "Sessions", "Settings", "Help"];
        let tabs = Tabs::new(tab_titles)
            .block(Block::default().borders(Borders::ALL).title(config.title.as_str()))
            .select(screens.current_index())
            .highlight_style(Style::default().fg(Color::Yellow));
        f.render_widget(tabs, chunks[0]);

        // Main content
        let title = screens.current_screen()
            .map(|s| s.title())
            .unwrap_or("Unknown");
        let content = Paragraph::new(format!("Screen: {}", title))
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(content, chunks[1]);

        // Status bar
        let status = Paragraph::new(" q:Quit  Tab:Next  ?:Help")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(status, chunks[2]);
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    pub fn is_running(&self) -> bool {
        self.running
    }
}
