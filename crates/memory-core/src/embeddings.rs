use async_trait::async_trait;
use serde_json::Value;
use tracing::{debug, warn};

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>>;
    fn dimensions(&self) -> usize;
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim_end_matches('/').to_string()
}

fn parse_embedding_vector(v: &Value) -> anyhow::Result<Vec<f32>> {
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("embedding is not an array"))?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let n = item
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("embedding element is not numeric"))?;
        out.push(n as f32);
    }
    Ok(out)
}

fn parse_embeddings_response(parsed: &Value) -> anyhow::Result<Vec<Vec<f32>>> {
    // OpenAI/Voyage shape: { data: [{ embedding: [...] }, ...] }
    if let Some(data) = parsed.get("data").and_then(Value::as_array) {
        let mut out = Vec::with_capacity(data.len());
        for item in data {
            out.push(parse_embedding_vector(
                item.get("embedding")
                    .ok_or_else(|| anyhow::anyhow!("missing data[].embedding"))?,
            )?);
        }
        return Ok(out);
    }

    // Alternate shape: { embeddings: [[...], [...]] }
    if let Some(embeddings) = parsed.get("embeddings").and_then(Value::as_array) {
        let mut out = Vec::with_capacity(embeddings.len());
        for emb in embeddings {
            out.push(parse_embedding_vector(emb)?);
        }
        return Ok(out);
    }

    anyhow::bail!("unsupported embeddings response shape");
}

async fn post_json_with_optional_bearer(
    client: &reqwest::Client,
    url: &str,
    body: &Value,
    api_key: &str,
) -> anyhow::Result<Value> {
    let req = client.post(url).json(body);
    let req = if api_key.trim().is_empty() {
        req
    } else {
        req.bearer_auth(api_key)
    };
    let resp = req.send().await?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        anyhow::bail!("embeddings api error {}: {}", status, text);
    }
    Ok(serde_json::from_str(&text)?)
}

pub struct OpenAIEmbedding {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenAIEmbedding {
    pub fn new(api_key: &str, model: Option<&str>) -> Self {
        Self::with_base_url(
            api_key,
            model.unwrap_or("text-embedding-3-small"),
            "https://api.openai.com/v1",
        )
    }

    pub fn with_base_url(api_key: &str, model: &str, base_url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            base_url: normalize_base_url(base_url),
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/embeddings", self.base_url)
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAIEmbedding {
    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let body = serde_json::json!({
            "model": self.model,
            "input": texts,
        });
        let parsed =
            post_json_with_optional_bearer(&self.client, &self.endpoint(), &body, &self.api_key)
                .await?;
        parse_embeddings_response(&parsed)
    }

    fn dimensions(&self) -> usize {
        if self.model.contains("3-large") {
            3072
        } else {
            1536
        }
    }
}

pub struct VoyageEmbedding {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl VoyageEmbedding {
    pub fn new(api_key: &str, model: Option<&str>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            model: model.unwrap_or("voyage-3-lite").to_string(),
            base_url: "https://api.voyageai.com/v1".to_string(),
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/embeddings", self.base_url)
    }
}

#[async_trait]
impl EmbeddingProvider for VoyageEmbedding {
    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let body = serde_json::json!({
            "model": self.model,
            "input": texts,
            "input_type": "document",
            "truncation": true
        });
        let parsed =
            post_json_with_optional_bearer(&self.client, &self.endpoint(), &body, &self.api_key)
                .await?;
        parse_embeddings_response(&parsed)
    }

    fn dimensions(&self) -> usize {
        if self.model.contains("lite") {
            512
        } else {
            1024
        }
    }
}

pub struct LocalEmbedding {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl LocalEmbedding {
    pub fn new(base_url: &str, api_key: Option<&str>, model: Option<&str>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.unwrap_or_default().to_string(),
            model: model.unwrap_or("nomic-embed-text").to_string(),
            base_url: normalize_base_url(base_url),
        }
    }

    fn openai_endpoint_candidates(&self) -> [String; 2] {
        if self.base_url.ends_with("/v1") {
            [
                format!("{}/embeddings", self.base_url),
                format!("{}/v1/embeddings", self.base_url.trim_end_matches("/v1")),
            ]
        } else {
            [
                format!("{}/v1/embeddings", self.base_url),
                format!("{}/embeddings", self.base_url),
            ]
        }
    }

    async fn try_openai_compatible(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let body = serde_json::json!({
            "model": self.model,
            "input": texts,
        });
        let endpoints = self.openai_endpoint_candidates();
        let mut last_err: Option<anyhow::Error> = None;
        for endpoint in &endpoints {
            match post_json_with_optional_bearer(&self.client, endpoint, &body, &self.api_key).await
            {
                Ok(parsed) => return parse_embeddings_response(&parsed),
                Err(e) => {
                    debug!(
                        "local openai-compatible endpoint {} failed: {}",
                        endpoint, e
                    );
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no local openai-compatible endpoint")))
    }

    async fn try_ollama_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let endpoint = format!("{}/api/embed", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "input": texts,
        });
        let parsed = post_json_with_optional_bearer(&self.client, &endpoint, &body, "").await?;
        parse_embeddings_response(&parsed)
    }

    async fn try_ollama_single(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let endpoint = format!("{}/api/embeddings", self.base_url);
        let mut out = Vec::with_capacity(texts.len());
        for text in texts {
            let body = serde_json::json!({
                "model": self.model,
                "prompt": text,
            });
            let parsed = post_json_with_optional_bearer(&self.client, &endpoint, &body, "").await?;
            let emb = parsed
                .get("embedding")
                .ok_or_else(|| anyhow::anyhow!("missing embedding in ollama response"))?;
            out.push(parse_embedding_vector(emb)?);
        }
        Ok(out)
    }
}

#[async_trait]
impl EmbeddingProvider for LocalEmbedding {
    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        match self.try_openai_compatible(texts).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                debug!("local openai-compatible embeddings failed: {}", e);
            }
        }

        match self.try_ollama_batch(texts).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                debug!("local ollama batch embeddings failed: {}", e);
            }
        }

        self.try_ollama_single(texts).await
    }

    fn dimensions(&self) -> usize {
        768
    }
}

/// Factory: create an embedding provider from config.
pub fn create_embedding_provider(
    provider: &str,
    api_key: &str,
    model: Option<&str>,
) -> Box<dyn EmbeddingProvider> {
    let provider = provider.trim();
    let provider_lower = provider.to_lowercase();

    match provider_lower.as_str() {
        "" | "openai" => Box::new(OpenAIEmbedding::new(api_key, model)),
        "voyage" | "voyageai" => Box::new(VoyageEmbedding::new(api_key, model)),
        "local" => {
            let local_url = std::env::var("OCLAWS_LOCAL_EMBEDDING_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
            Box::new(LocalEmbedding::new(&local_url, Some(api_key), model))
        }
        "openai-compatible" => {
            let compat_url = std::env::var("OCLAWS_EMBEDDING_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:11434/v1".to_string());
            Box::new(OpenAIEmbedding::with_base_url(
                api_key,
                model.unwrap_or("text-embedding-3-small"),
                &compat_url,
            ))
        }
        raw if raw.starts_with("http://") || raw.starts_with("https://") => {
            Box::new(LocalEmbedding::new(raw, Some(api_key), model))
        }
        other => {
            warn!(
                "Unsupported embedding provider '{}', falling back to OpenAI-compatible embeddings",
                other
            );
            Box::new(OpenAIEmbedding::new(api_key, model))
        }
    }
}
