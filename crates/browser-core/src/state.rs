//! Page state tracking for browser automation.
//!
//! Tracks console messages, page errors, and network requests
//! per page, similar to Playwright's page state management.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_CONSOLE: usize = 500;
const MAX_ERRORS: usize = 200;
const MAX_REQUESTS: usize = 500;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleEntry {
    pub level: String,
    pub text: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageError {
    pub message: String,
    pub stack: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkEntry {
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub resource_type: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Default)]
pub struct PageState {
    pub console: VecDeque<ConsoleEntry>,
    pub errors: VecDeque<PageError>,
    pub requests: VecDeque<NetworkEntry>,
    pub target_id: Option<String>,
    pub url: Option<String>,
    pub title: Option<String>,
}

impl PageState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_console(&mut self, level: &str, text: &str) {
        if self.console.len() >= MAX_CONSOLE {
            self.console.pop_front();
        }
        self.console.push_back(ConsoleEntry {
            level: level.to_string(),
            text: text.to_string(),
            timestamp: now_ms(),
        });
    }

    pub fn push_error(&mut self, message: &str, stack: Option<&str>) {
        if self.errors.len() >= MAX_ERRORS {
            self.errors.pop_front();
        }
        self.errors.push_back(PageError {
            message: message.to_string(),
            stack: stack.map(String::from),
            timestamp: now_ms(),
        });
    }

    pub fn push_request(
        &mut self,
        method: &str,
        url: &str,
        status: Option<u16>,
        resource_type: Option<&str>,
    ) {
        if self.requests.len() >= MAX_REQUESTS {
            self.requests.pop_front();
        }
        self.requests.push_back(NetworkEntry {
            method: method.to_string(),
            url: url.to_string(),
            status,
            resource_type: resource_type.map(String::from),
            timestamp: now_ms(),
        });
    }

    pub fn recent_console(&self, n: usize) -> Vec<&ConsoleEntry> {
        self.console.iter().rev().take(n).collect()
    }

    pub fn recent_errors(&self, n: usize) -> Vec<&PageError> {
        self.errors.iter().rev().take(n).collect()
    }

    pub fn recent_requests(&self, n: usize) -> Vec<&NetworkEntry> {
        self.requests.iter().rev().take(n).collect()
    }
}
