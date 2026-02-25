//! Model catalog — static registry of known model metadata.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::providers::ProviderType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub provider: ProviderType,
    pub context_window: usize,
    pub max_output_tokens: Option<usize>,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_streaming: bool,
    pub supports_thinking: bool,
    pub cost_per_1k_input: Option<f64>,
    pub cost_per_1k_output: Option<f64>,
}

pub struct ModelCatalog {
    models: HashMap<String, ModelInfo>,
}

impl ModelCatalog {
    pub fn new() -> Self {
        Self { models: HashMap::new() }
    }

    /// Pre-populated catalog with well-known models.
    pub fn builtin() -> Self {
        let mut cat = Self::new();
        cat.register_builtins();
        cat
    }

    pub fn register(&mut self, info: ModelInfo) {
        self.models.insert(info.id.clone(), info);
    }

    pub fn lookup(&self, model_id: &str) -> Option<&ModelInfo> {
        self.models.get(model_id)
    }

    pub fn context_window(&self, model_id: &str) -> usize {
        self.models.get(model_id)
            .map(|m| m.context_window)
            .unwrap_or(4096)
    }

    pub fn supports_tools(&self, model_id: &str) -> bool {
        self.models.get(model_id)
            .map(|m| m.supports_tools)
            .unwrap_or(false)
    }

    pub fn supports_vision(&self, model_id: &str) -> bool {
        self.models.get(model_id)
            .map(|m| m.supports_vision)
            .unwrap_or(false)
    }

    pub fn list_by_provider(&self, provider: ProviderType) -> Vec<&ModelInfo> {
        self.models.values()
            .filter(|m| m.provider == provider)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.models.len()
    }

    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    fn register_builtins(&mut self) {
        // Anthropic
        for (id, ctx, out, vision, thinking) in [
            ("claude-sonnet-4-20250514", 200_000, 8192, true, true),
            ("claude-opus-4-20250514", 200_000, 8192, true, true),
            ("claude-3-5-haiku-20241022", 200_000, 8192, false, false),
        ] {
            self.register(ModelInfo {
                id: id.to_string(),
                provider: ProviderType::Anthropic,
                context_window: ctx,
                max_output_tokens: Some(out),
                supports_tools: true,
                supports_vision: vision,
                supports_streaming: true,
                supports_thinking: thinking,
                cost_per_1k_input: None,
                cost_per_1k_output: None,
            });
        }

        // OpenAI
        for (id, ctx, out, vision) in [
            ("gpt-4o", 128_000, 4096, true),
            ("gpt-4o-mini", 128_000, 4096, true),
            ("gpt-4-turbo", 128_000, 4096, true),
            ("o1", 200_000, 100_000, true),
            ("o3-mini", 200_000, 100_000, false),
        ] {
            self.register(ModelInfo {
                id: id.to_string(),
                provider: ProviderType::OpenAi,
                context_window: ctx,
                max_output_tokens: Some(out),
                supports_tools: true,
                supports_vision: vision,
                supports_streaming: true,
                supports_thinking: false,
                cost_per_1k_input: None,
                cost_per_1k_output: None,
            });
        }

        // Google
        for (id, ctx) in [
            ("gemini-2.0-flash", 1_048_576),
            ("gemini-1.5-pro", 2_097_152),
        ] {
            self.register(ModelInfo {
                id: id.to_string(),
                provider: ProviderType::Google,
                context_window: ctx,
                max_output_tokens: Some(8192),
                supports_tools: true,
                supports_vision: true,
                supports_streaming: true,
                supports_thinking: false,
                cost_per_1k_input: None,
                cost_per_1k_output: None,
            });
        }
    }
}

impl Default for ModelCatalog {
    fn default() -> Self {
        Self::builtin()
    }
}
