use async_trait::async_trait;

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>>;
    fn dimensions(&self) -> usize;
}

pub struct OpenAIEmbedding {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenAIEmbedding {
    pub fn new(api_key: &str, model: Option<&str>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            model: model.unwrap_or("text-embedding-3-small").to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
        }
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAIEmbedding {
    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let url = format!("{}/embeddings", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "input": texts,
        });

        let resp = self.client.post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("OpenAI embeddings API error {}: {}", status, text);
        }

        let parsed: serde_json::Value = serde_json::from_str(&text)?;
        let data = parsed["data"].as_array()
            .ok_or_else(|| anyhow::anyhow!("missing data field"))?;

        let mut results = Vec::with_capacity(data.len());
        for item in data {
            let embedding = item["embedding"].as_array()
                .ok_or_else(|| anyhow::anyhow!("missing embedding"))?
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();
            results.push(embedding);
        }
        Ok(results)
    }

    fn dimensions(&self) -> usize {
        1536
    }
}

/// Factory: create an embedding provider from config.
pub fn create_embedding_provider(
    _provider: &str,
    api_key: &str,
    model: Option<&str>,
) -> Box<dyn EmbeddingProvider> {
    Box::new(OpenAIEmbedding::new(api_key, model))
}
