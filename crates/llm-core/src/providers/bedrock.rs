//! AWS Bedrock provider with SigV4 signing

use async_trait::async_trait;
use hmac::{Hmac, Mac};
use sha2::{Sha256, Digest};
use crate::chat::*;
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::{LlmError, LlmResult};
use super::{LlmProvider, ProviderType};

pub struct BedrockProvider {
    region: String,
    access_key: String,
    secret_key: String,
    client: reqwest::Client,
}

type HmacSha256 = Hmac<Sha256>;

impl BedrockProvider {
    pub fn new(access_key: &str, secret_key: &str, region: Option<&str>) -> LlmResult<Self> {
        Ok(Self {
            region: region.unwrap_or("us-east-1").into(),
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            client: reqwest::Client::new(),
        })
    }

    fn endpoint(&self, model_id: &str) -> String {
        format!(
            "https://bedrock-runtime.{}.amazonaws.com/model/{}/invoke",
            self.region, model_id
        )
    }

    fn host(&self) -> String {
        format!("bedrock-runtime.{}.amazonaws.com", self.region)
    }

    fn role_str(role: &MessageRole) -> &'static str {
        match role {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        }
    }

    fn sign_request(&self, method: &str, uri: &str, body: &[u8], timestamp: &str, date: &str) -> Vec<(String, String)> {
        let host = self.host();
        let service = "bedrock";
        let payload_hash = hex::encode(Sha256::digest(body));

        // Canonical request (must include empty query string line and content-type)
        let canonical_headers = format!(
            "content-type:application/json\nhost:{}\nx-amz-content-sha256:{}\nx-amz-date:{}\n",
            host, payload_hash, timestamp
        );
        let signed_headers = "content-type;host;x-amz-content-sha256;x-amz-date";
        let canonical_request = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            method, uri, "", canonical_headers, signed_headers, payload_hash
        );

        // String to sign
        let scope = format!("{}/{}/{}/aws4_request", date, self.region, service);
        let canonical_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));
        let string_to_sign = format!("AWS4-HMAC-SHA256\n{}\n{}\n{}", timestamp, scope, canonical_hash);

        // Signing key
        let k_date = hmac_sha256(format!("AWS4{}", self.secret_key).as_bytes(), date.as_bytes());
        let k_region = hmac_sha256(&k_date, self.region.as_bytes());
        let k_service = hmac_sha256(&k_region, service.as_bytes());
        let k_signing = hmac_sha256(&k_service, b"aws4_request");
        let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

        let auth = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.access_key, scope, signed_headers, signature
        );

        vec![
            ("Authorization".into(), auth),
            ("x-amz-date".into(), timestamp.into()),
            ("x-amz-content-sha256".into(), payload_hash),
            ("host".into(), host),
        ]
    }
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

#[async_trait]
impl LlmProvider for BedrockProvider {
    fn provider_type(&self) -> ProviderType { ProviderType::Bedrock }

    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatCompletion> {
        let model = &request.model;
        let url = self.endpoint(model);
        let uri = format!("/model/{}/invoke", model);

        let messages: Vec<serde_json::Value> = request.messages.iter().map(|m| {
            serde_json::json!({ "role": Self::role_str(&m.role), "content": m.content })
        }).collect();

        let body = serde_json::json!({
            "anthropic_version": "bedrock-2023-10-16",
            "max_tokens": request.max_tokens.unwrap_or(4096),
            "messages": messages,
        });

        let body_bytes = serde_json::to_vec(&body).map_err(|e| LlmError::ParseError(e.to_string()))?;
        let now = chrono::Utc::now();
        let timestamp = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();

        let sig_headers = self.sign_request("POST", &uri, &body_bytes, &timestamp, &date);

        let mut req = self.client.post(&url)
            .header("Content-Type", "application/json")
            .body(body_bytes);

        for (k, v) in &sig_headers {
            if k != "host" {
                req = req.header(k.as_str(), v.as_str());
            }
        }

        let resp = req.send().await.map_err(|e| LlmError::NetworkError(e.to_string()))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if !status.is_success() {
            return Err(LlmError::ApiError(format!("{}: {}", status, text)));
        }

        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| LlmError::ParseError(e.to_string()))?;

        let content = json["content"][0]["text"].as_str().unwrap_or("").to_string();

        Ok(ChatCompletion {
            id: json["id"].as_str().unwrap_or("bedrock").into(),
            object: "chat.completion".into(),
            model: model.clone(),
            created: chrono::Utc::now().timestamp(),
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: MessageRole::Assistant,
                    content,
                    name: None, tool_calls: None, tool_call_id: None,
                },
                finish_reason: json["stop_reason"].as_str().map(|s| s.into()),
            }],
            usage: Some(Usage {
                prompt_tokens: json["usage"]["input_tokens"].as_i64().unwrap_or(0) as i32,
                completion_tokens: json["usage"]["output_tokens"].as_i64().unwrap_or(0) as i32,
                total_tokens: 0,
            }),
            system_fingerprint: None,
        })
    }

    async fn embeddings(&self, _request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        Err(LlmError::UnsupportedModel("Use Bedrock embedding models directly".into()))
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "anthropic.claude-3-5-sonnet-20241022-v2:0".into(),
            "anthropic.claude-3-haiku-20240307-v1:0".into(),
            "amazon.titan-text-express-v1".into(),
        ]
    }

    fn default_model(&self) -> &str { "anthropic.claude-3-5-sonnet-20241022-v2:0" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sigv4_signature_deterministic() {
        let provider = BedrockProvider::new("AKIAIOSFODNN7EXAMPLE", "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY", Some("us-east-1")).unwrap();
        let body = b"{}";
        let headers1 = provider.sign_request("POST", "/model/test/invoke", body, "20240101T000000Z", "20240101");
        let headers2 = provider.sign_request("POST", "/model/test/invoke", body, "20240101T000000Z", "20240101");
        // Same inputs produce same signature
        assert_eq!(headers1.iter().find(|(k,_)| k == "Authorization").unwrap().1,
                   headers2.iter().find(|(k,_)| k == "Authorization").unwrap().1);
    }

    #[test]
    fn test_endpoint_format() {
        let provider = BedrockProvider::new("key", "secret", Some("us-west-2")).unwrap();
        assert_eq!(provider.endpoint("anthropic.claude-3-haiku-20240307-v1:0"),
                   "https://bedrock-runtime.us-west-2.amazonaws.com/model/anthropic.claude-3-haiku-20240307-v1:0/invoke");
    }
}
