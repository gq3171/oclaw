use ratatui::style::{Color, Modifier, Style};

pub struct Theme {
    pub accent: Color,
    pub text: Color,
    pub text_muted: Color,
    pub bg: Color,
    pub border: Color,
    pub border_focus: Color,
    pub error: Color,
    pub success: Color,
    pub warning: Color,
    pub info: Color,
    pub user_msg: Color,
    pub assistant_msg: Color,
    pub tool_pending: Color,
    pub tool_success: Color,
    pub tool_error: Color,
    pub header_bg: Color,
    pub status_bg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            accent: Color::Cyan,
            text: Color::White,
            text_muted: Color::DarkGray,
            bg: Color::Reset,
            border: Color::DarkGray,
            border_focus: Color::Cyan,
            error: Color::Red,
            success: Color::Green,
            warning: Color::Yellow,
            info: Color::Blue,
            user_msg: Color::Green,
            assistant_msg: Color::White,
            tool_pending: Color::Blue,
            tool_success: Color::Green,
            tool_error: Color::Red,
            header_bg: Color::DarkGray,
            status_bg: Color::DarkGray,
        }
    }
}

impl Theme {
    pub fn style(&self) -> Style {
        Style::default().fg(self.text)
    }

    pub fn accent_style(&self) -> Style {
        Style::default().fg(self.accent)
    }

    pub fn muted_style(&self) -> Style {
        Style::default().fg(self.text_muted)
    }

    pub fn error_style(&self) -> Style {
        Style::default().fg(self.error)
    }

    pub fn success_style(&self) -> Style {
        Style::default().fg(self.success)
    }

    pub fn border_style(&self) -> Style {
        Style::default().fg(self.border)
    }

    pub fn border_focus_style(&self) -> Style {
        Style::default().fg(self.border_focus)
    }

    pub fn header_style(&self) -> Style {
        Style::default().fg(self.accent).bg(self.header_bg).add_modifier(Modifier::BOLD)
    }

    pub fn status_style(&self) -> Style {
        Style::default().fg(self.text_muted).bg(self.status_bg)
    }

    pub fn user_style(&self) -> Style {
        Style::default().fg(self.user_msg).add_modifier(Modifier::BOLD)
    }

    pub fn assistant_style(&self) -> Style {
        Style::default().fg(self.assistant_msg)
    }
}
