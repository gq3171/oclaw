//! TUI Screens

/// Screen identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScreenId(pub String);

/// Base screen trait
pub trait Screen {
    fn title(&self) -> &str;
}

/// Screen manager with navigation
pub struct ScreenManager {
    screens: Vec<Box<dyn Screen>>,
    current: usize,
}

impl ScreenManager {
    pub fn new() -> Self {
        Self {
            screens: vec![
                Box::new(DashboardScreen),
                Box::new(SessionsScreen),
                Box::new(SettingsScreen),
                Box::new(HelpScreen),
            ],
            current: 0,
        }
    }

    pub fn current_screen(&self) -> Option<&dyn Screen> {
        self.screens.get(self.current).map(|b| &**b)
    }

    pub fn current_screen_mut(&mut self) -> Option<&mut Box<dyn Screen>> {
        self.screens.get_mut(self.current)
    }

    pub fn current_index(&self) -> usize {
        self.current
    }

    pub fn goto(&mut self, index: usize) {
        if index < self.screens.len() {
            self.current = index;
        }
    }

    pub fn next(&mut self) {
        if !self.screens.is_empty() {
            self.current = (self.current + 1) % self.screens.len();
        }
    }

    pub fn previous(&mut self) {
        if !self.screens.is_empty() {
            self.current = self.current.checked_sub(1).unwrap_or(self.screens.len() - 1);
        }
    }

    pub fn len(&self) -> usize {
        self.screens.len()
    }

    pub fn is_empty(&self) -> bool {
        self.screens.is_empty()
    }
}

impl Default for ScreenManager {
    fn default() -> Self {
        Self::new()
    }
}

pub struct DashboardScreen;
impl Screen for DashboardScreen {
    fn title(&self) -> &str { "Dashboard" }
}

pub struct SessionsScreen;
impl Screen for SessionsScreen {
    fn title(&self) -> &str { "Sessions" }
}

pub struct SettingsScreen;
impl Screen for SettingsScreen {
    fn title(&self) -> &str { "Settings" }
}

pub struct HelpScreen;
impl Screen for HelpScreen {
    fn title(&self) -> &str { "Help" }
}
