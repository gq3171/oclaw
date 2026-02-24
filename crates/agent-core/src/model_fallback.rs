use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::time::Instant;
use tokio::sync::RwLock;
use oclaws_llm_core::providers::ProviderType;
use crate::{Agent, AgentResult, AgentError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FallbackConfig {
    pub enabled: bool,
    pub max_retries: i32,
    pub retry_delay_ms: i64,
    pub fallback_on_error: bool,
    #[serde(default = "default_cooldown")]
    pub cooldown_secs: u64,
    #[serde(default)]
    pub candidates: Vec<ModelCandidate>,
}

fn default_cooldown() -> u64 { 60 }

impl Default for FallbackConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: 3,
            retry_delay_ms: 1000,
            fallback_on_error: true,
            cooldown_secs: 60,
            candidates: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCandidate {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct FallbackAttempt {
    pub provider: String,
    pub model: String,
    pub error: String,
}

#[derive(Debug)]
pub struct FallbackResult<T> {
    pub result: T,
    pub provider: String,
    pub model: String,
    pub attempts: Vec<FallbackAttempt>,
}

/// Cooldown tracker for failed candidates.
#[derive(Default)]
pub struct CooldownTracker {
    failures: Mutex<HashMap<String, Instant>>,
}

impl CooldownTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_failed(&self, key: &str) {
        self.failures.lock().unwrap().insert(key.to_string(), Instant::now());
    }

    pub fn is_cooled_down(&self, key: &str, cooldown_secs: u64) -> bool {
        let map = self.failures.lock().unwrap();
        match map.get(key) {
            None => true,
            Some(t) => t.elapsed().as_secs() >= cooldown_secs,
        }
    }

    pub fn clear(&self, key: &str) {
        self.failures.lock().unwrap().remove(key);
    }
}

/// Generic fallback executor: tries each candidate in order, skipping cooled-down ones.
pub async fn run_with_fallback<T, F, Fut>(
    config: &FallbackConfig,
    cooldowns: &CooldownTracker,
    run: F,
) -> Result<FallbackResult<T>, AgentError>
where
    F: Fn(&str, &str) -> Fut,
    Fut: Future<Output = Result<T, AgentError>>,
{
    let mut attempts = Vec::new();

    for candidate in &config.candidates {
        let key = format!("{}:{}", candidate.provider, candidate.model);
        if !cooldowns.is_cooled_down(&key, config.cooldown_secs) {
            continue;
        }

        match run(&candidate.provider, &candidate.model).await {
            Ok(result) => {
                cooldowns.clear(&key);
                return Ok(FallbackResult {
                    result,
                    provider: candidate.provider.clone(),
                    model: candidate.model.clone(),
                    attempts,
                });
            }
            Err(e) => {
                let err_str = e.to_string();
                tracing::warn!(
                    "Fallback candidate {}/{}: failed: {}",
                    candidate.provider, candidate.model, err_str
                );
                cooldowns.mark_failed(&key);
                attempts.push(FallbackAttempt {
                    provider: candidate.provider.clone(),
                    model: candidate.model.clone(),
                    error: err_str,
                });

                if config.retry_delay_ms > 0 {
                    tokio::time::sleep(
                        tokio::time::Duration::from_millis(config.retry_delay_ms as u64)
                    ).await;
                }
            }
        }
    }

    Err(AgentError::ModelError(format!(
        "All {} fallback candidates exhausted",
        attempts.len()
    )))
}

#[derive(Debug, Clone)]
pub struct ModelChainEntry {
    pub provider_type: ProviderType,
    pub model: String,
    pub priority: i32,
}

pub struct ModelChain {
    entries: Vec<ModelChainEntry>,
    current_index: usize,
}

impl ModelChain {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            current_index: 0,
        }
    }

    pub fn add_model(&mut self, provider: ProviderType, model: &str, priority: i32) {
        self.entries.push(ModelChainEntry {
            provider_type: provider,
            model: model.to_string(),
            priority,
        });
        self.entries.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    pub fn current(&self) -> Option<&ModelChainEntry> {
        self.entries.get(self.current_index)
    }

    pub fn advance(&mut self) -> bool {
        if self.current_index < self.entries.len() - 1 {
            self.current_index += 1;
            true
        } else {
            false
        }
    }

    pub fn reset(&mut self) {
        self.current_index = 0;
    }

    pub fn is_exhausted(&self) -> bool {
        self.current_index >= self.entries.len()
    }

    pub fn entries(&self) -> &[ModelChainEntry] {
        &self.entries
    }
}

impl Default for ModelChain {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ModelFallback {
    chain: RwLock<ModelChain>,
    config: FallbackConfig,
}

impl ModelFallback {
    pub fn new(config: FallbackConfig) -> Self {
        Self {
            chain: RwLock::new(ModelChain::new()),
            config,
        }
    }

    pub fn with_chain(mut self, chain: ModelChain) -> Self {
        self.chain = RwLock::new(chain);
        self
    }

    pub async fn add_model(&self, provider: ProviderType, model: &str, priority: i32) {
        self.chain.write().await.add_model(provider, model, priority);
    }

    pub async fn execute_with_agent(
        &self,
        agent: &mut Agent,
        input: &str,
    ) -> AgentResult<String> {
        if !self.config.enabled {
            return Err(AgentError::ModelError("Fallback not enabled".to_string()));
        }

        let mut chain = self.chain.write().await;
        chain.reset();

        let max_attempts = self.config.max_retries * chain.entries.len() as i32;
        let mut attempts = 0;

        while !chain.is_exhausted() && attempts < max_attempts {
            if let Some(entry) = chain.current() {
                attempts += 1;

                tracing::info!(
                    "Trying model {} (attempt {})",
                    entry.model,
                    attempts
                );

                match agent.run(input).await {
                    Ok(result) => {
                        chain.reset();
                        return Ok(result);
                    }
                    Err(e) => {
                        tracing::warn!("Model {} failed: {}", entry.model, e);

                        if !chain.advance() {
                            break;
                        }

                        if self.config.retry_delay_ms > 0 {
                            tokio::time::sleep(tokio::time::Duration::from_millis(
                                self.config.retry_delay_ms as u64,
                            )).await;
                        }
                    }
                }
            }
        }

        Err(AgentError::ModelError(format!(
            "All models in chain failed after {} attempts",
            attempts
        )))
    }

    pub async fn get_chain_status(&self) -> Vec<(String, String, bool)> {
        let chain = self.chain.read().await;
        let current_idx = chain.current_index;
        
        chain.entries()
            .iter()
            .enumerate()
            .map(|(i, e)| {
                (e.model.clone(), format!("{:?}", e.provider_type), i == current_idx)
            })
            .collect()
    }
}
