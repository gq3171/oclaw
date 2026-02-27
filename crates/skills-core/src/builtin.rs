use crate::skill::{
    Skill, SkillDefinition, SkillExample, SkillInput, SkillOutput, SkillParameter, SkillReturnType,
};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use std::collections::HashMap;

pub struct CalculatorSkill;

impl Default for CalculatorSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl CalculatorSkill {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Skill for CalculatorSkill {
    fn definition(&self) -> &SkillDefinition {
        &SKILL_DEFINITION
    }

    async fn execute(&self, input: SkillInput) -> crate::SkillResult<SkillOutput> {
        let expr = input
            .parameters
            .get("expression")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'expression' parameter".to_string())
            })?;

        let result = evaluate_expression(expr).map_err(crate::SkillError::ExecutionError)?;

        Ok(SkillOutput {
            success: true,
            result: Some(serde_json::json!({ "result": result })),
            error: None,
            metadata: HashMap::new(),
            execution_time_ms: 0,
        })
    }
}

fn evaluate_expression(expr: &str) -> Result<f64, String> {
    let expr = expr.replace(" ", "");

    if expr.contains('+') {
        let parts: Vec<&str> = expr.split('+').collect();
        let mut sum = 0.0;
        for part in parts {
            sum += evaluate_expression(part)?;
        }
        return Ok(sum);
    }

    if expr.contains('-') && !expr.starts_with('-') {
        let parts: Vec<&str> = expr.split('-').collect();
        let mut result = evaluate_expression(parts[0])?;
        for part in &parts[1..] {
            result -= evaluate_expression(part)?;
        }
        return Ok(result);
    }

    if expr.contains('*') {
        let parts: Vec<&str> = expr.split('*').collect();
        let mut result = 1.0;
        for part in parts {
            result *= evaluate_expression(part)?;
        }
        return Ok(result);
    }

    if expr.contains('/') {
        let parts: Vec<&str> = expr.split('/').collect();
        let mut result = evaluate_expression(parts[0])?;
        for part in &parts[1..] {
            let divisor = evaluate_expression(part)?;
            if divisor == 0.0 {
                return Err("Division by zero".to_string());
            }
            result /= divisor;
        }
        return Ok(result);
    }

    expr.parse::<f64>()
        .map_err(|_| "Invalid number".to_string())
}

static SKILL_DEFINITION: Lazy<SkillDefinition> = Lazy::new(|| SkillDefinition {
    id: "calculator".to_string(),
    name: "Calculator".to_string(),
    description: "Perform mathematical calculations".to_string(),
    category: "utilities".to_string(),
    tags: vec![
        "math".to_string(),
        "calculator".to_string(),
        "arithmetic".to_string(),
    ],
    parameters: vec![SkillParameter {
        name: "expression".to_string(),
        param_type: "string".to_string(),
        description: "Mathematical expression (e.g., '2+2*3')".to_string(),
        required: true,
        default: None,
    }],
    returns: SkillReturnType {
        param_type: "number".to_string(),
        description: "Result of the calculation".to_string(),
    },
    examples: vec![SkillExample {
        input: serde_json::json!({ "expression": "2+2" }),
        output: serde_json::json!({ "result": 4.0 }),
        description: "Simple addition".to_string(),
    }],
    permissions: vec![],
    rate_limit: None,
    version: "1.0.0".to_string(),
});

pub struct TextTransformSkill;

impl Default for TextTransformSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl TextTransformSkill {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Skill for TextTransformSkill {
    fn definition(&self) -> &SkillDefinition {
        &TEXT_TRANSFORM_DEFINITION
    }

    async fn execute(&self, input: SkillInput) -> crate::SkillResult<SkillOutput> {
        let text = input
            .parameters
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'text' parameter".to_string())
            })?;

        let transform = input
            .parameters
            .get("transform")
            .and_then(|v| v.as_str())
            .unwrap_or("uppercase");

        let result = match transform {
            "uppercase" => text.to_uppercase(),
            "lowercase" => text.to_lowercase(),
            "titlecase" => to_title_case(text),
            "reverse" => text.chars().rev().collect(),
            "trim" => text.trim().to_string(),
            "reverse_words" => text.split_whitespace().rev().collect::<Vec<_>>().join(" "),
            _ => {
                return Err(crate::SkillError::ValidationError(format!(
                    "Unknown transform: {}",
                    transform
                )));
            }
        };

        Ok(SkillOutput {
            success: true,
            result: Some(serde_json::json!({ "result": result })),
            error: None,
            metadata: HashMap::new(),
            execution_time_ms: 0,
        })
    }
}

fn to_title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first
                    .to_uppercase()
                    .chain(chars.flat_map(|c| c.to_lowercase()))
                    .collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

static TEXT_TRANSFORM_DEFINITION: Lazy<SkillDefinition> = Lazy::new(|| SkillDefinition {
    id: "text_transform".to_string(),
    name: "Text Transform".to_string(),
    description: "Transform text with various operations".to_string(),
    category: "text".to_string(),
    tags: vec![
        "text".to_string(),
        "transform".to_string(),
        "string".to_string(),
    ],
    parameters: vec![
        SkillParameter {
            name: "text".to_string(),
            param_type: "string".to_string(),
            description: "Text to transform".to_string(),
            required: true,
            default: None,
        },
        SkillParameter {
            name: "transform".to_string(),
            param_type: "string".to_string(),
            description:
                "Transform type: uppercase, lowercase, titlecase, reverse, trim, reverse_words"
                    .to_string(),
            required: false,
            default: Some(serde_json::json!("uppercase")),
        },
    ],
    returns: SkillReturnType {
        param_type: "string".to_string(),
        description: "Transformed text".to_string(),
    },
    examples: vec![],
    permissions: vec![],
    rate_limit: None,
    version: "1.0.0".to_string(),
});

pub struct JsonFormatterSkill;

impl Default for JsonFormatterSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonFormatterSkill {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Skill for JsonFormatterSkill {
    fn definition(&self) -> &SkillDefinition {
        &JSON_FORMATTER_DEFINITION
    }

    async fn execute(&self, input: SkillInput) -> crate::SkillResult<SkillOutput> {
        let json_str = input
            .parameters
            .get("json")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'json' parameter".to_string())
            })?;

        let pretty = input
            .parameters
            .get("pretty")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let value: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| crate::SkillError::ExecutionError(e.to_string()))?;

        let formatted = if pretty {
            serde_json::to_string_pretty(&value)
        } else {
            serde_json::to_string(&value)
        }
        .map_err(|e| crate::SkillError::ExecutionError(e.to_string()))?;

        Ok(SkillOutput {
            success: true,
            result: Some(serde_json::json!({ "result": formatted })),
            error: None,
            metadata: HashMap::new(),
            execution_time_ms: 0,
        })
    }
}

static JSON_FORMATTER_DEFINITION: Lazy<SkillDefinition> = Lazy::new(|| SkillDefinition {
    id: "json_formatter".to_string(),
    name: "JSON Formatter".to_string(),
    description: "Format and validate JSON".to_string(),
    category: "data".to_string(),
    tags: vec![
        "json".to_string(),
        "format".to_string(),
        "validate".to_string(),
    ],
    parameters: vec![
        SkillParameter {
            name: "json".to_string(),
            param_type: "string".to_string(),
            description: "JSON string to format".to_string(),
            required: true,
            default: None,
        },
        SkillParameter {
            name: "pretty".to_string(),
            param_type: "boolean".to_string(),
            description: "Pretty print".to_string(),
            required: false,
            default: Some(serde_json::json!(true)),
        },
    ],
    returns: SkillReturnType {
        param_type: "string".to_string(),
        description: "Formatted JSON".to_string(),
    },
    examples: vec![],
    permissions: vec![],
    rate_limit: None,
    version: "1.0.0".to_string(),
});

pub struct DateTimeSkill;

impl Default for DateTimeSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl DateTimeSkill {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Skill for DateTimeSkill {
    fn definition(&self) -> &SkillDefinition {
        &DATETIME_DEFINITION
    }

    async fn execute(&self, input: SkillInput) -> crate::SkillResult<SkillOutput> {
        let operation = input
            .parameters
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'operation' parameter".to_string())
            })?;

        let now = chrono::Utc::now();

        let result = match operation {
            "now" => serde_json::json!({
                "iso": now.to_rfc3339(),
                "timestamp": now.timestamp(),
                "date": now.format("%Y-%m-%d").to_string(),
                "time": now.format("%H:%M:%S").to_string(),
            }),
            "timestamp" => {
                let ts = input
                    .parameters
                    .get("timestamp")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| {
                        crate::SkillError::ValidationError(
                            "Missing 'timestamp' for timestamp operation".to_string(),
                        )
                    })?;
                let dt = chrono::DateTime::from_timestamp(ts, 0).ok_or_else(|| {
                    crate::SkillError::ExecutionError("Invalid timestamp".to_string())
                })?;
                serde_json::json!({
                    "iso": dt.to_rfc3339(),
                    "date": dt.format("%Y-%m-%d").to_string(),
                    "time": dt.format("%H:%M:%S").to_string(),
                })
            }
            "format" => {
                let format = input
                    .parameters
                    .get("format")
                    .and_then(|v| v.as_str())
                    .unwrap_or("%Y-%m-%d %H:%M:%S");
                serde_json::json!({ "result": now.format(format).to_string() })
            }
            _ => {
                return Err(crate::SkillError::ValidationError(format!(
                    "Unknown operation: {}",
                    operation
                )));
            }
        };

        Ok(SkillOutput {
            success: true,
            result: Some(result),
            error: None,
            metadata: HashMap::new(),
            execution_time_ms: 0,
        })
    }
}

static DATETIME_DEFINITION: Lazy<SkillDefinition> = Lazy::new(|| SkillDefinition {
    id: "datetime".to_string(),
    name: "DateTime".to_string(),
    description: "Date and time operations".to_string(),
    category: "utilities".to_string(),
    tags: vec![
        "datetime".to_string(),
        "time".to_string(),
        "date".to_string(),
    ],
    parameters: vec![SkillParameter {
        name: "operation".to_string(),
        param_type: "string".to_string(),
        description: "Operation: now, timestamp, format".to_string(),
        required: true,
        default: None,
    }],
    returns: SkillReturnType {
        param_type: "object".to_string(),
        description: "Date/time result".to_string(),
    },
    examples: vec![],
    permissions: vec![],
    rate_limit: None,
    version: "1.0.0".to_string(),
});

pub struct UrlParserSkill;

impl Default for UrlParserSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl UrlParserSkill {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Skill for UrlParserSkill {
    fn definition(&self) -> &SkillDefinition {
        &URL_PARSER_DEFINITION
    }

    async fn execute(&self, input: SkillInput) -> crate::SkillResult<SkillOutput> {
        let url_str = input
            .parameters
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'url' parameter".to_string())
            })?;

        let url = url::Url::parse(url_str)
            .map_err(|e| crate::SkillError::ExecutionError(e.to_string()))?;

        let mut result = serde_json::json!({
            "scheme": url.scheme(),
            "host": url.host_str(),
            "port": url.port(),
            "path": url.path(),
            "query": url.query(),
            "fragment": url.fragment(),
        });

        let username = url.username();
        if !username.is_empty() {
            result["username"] = serde_json::json!(username);
        }
        if let Some(password) = url.password() {
            result["password"] = serde_json::json!(password);
        }

        Ok(SkillOutput {
            success: true,
            result: Some(result),
            error: None,
            metadata: HashMap::new(),
            execution_time_ms: 0,
        })
    }
}

static URL_PARSER_DEFINITION: Lazy<SkillDefinition> = Lazy::new(|| SkillDefinition {
    id: "url_parser".to_string(),
    name: "URL Parser".to_string(),
    description: "Parse and manipulate URLs".to_string(),
    category: "network".to_string(),
    tags: vec![
        "url".to_string(),
        "parser".to_string(),
        "network".to_string(),
    ],
    parameters: vec![SkillParameter {
        name: "url".to_string(),
        param_type: "string".to_string(),
        description: "URL to parse".to_string(),
        required: true,
        default: None,
    }],
    returns: SkillReturnType {
        param_type: "object".to_string(),
        description: "Parsed URL components".to_string(),
    },
    examples: vec![],
    permissions: vec![],
    rate_limit: None,
    version: "1.0.0".to_string(),
});

pub struct HashSkill;

impl Default for HashSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl HashSkill {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Skill for HashSkill {
    fn definition(&self) -> &SkillDefinition {
        &HASH_DEFINITION
    }

    async fn execute(&self, input: SkillInput) -> crate::SkillResult<SkillOutput> {
        let text = input
            .parameters
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'text' parameter".to_string())
            })?;

        let algorithm = input
            .parameters
            .get("algorithm")
            .and_then(|v| v.as_str())
            .unwrap_or("sha256");

        use sha2::{Digest, Sha256, Sha512};

        let hash = match algorithm {
            "sha256" => {
                let mut hasher = Sha256::new();
                hasher.update(text.as_bytes());
                format!("{:x}", hasher.finalize())
            }
            "sha512" => {
                let mut hasher = Sha512::new();
                hasher.update(text.as_bytes());
                format!("{:x}", hasher.finalize())
            }
            _ => {
                return Err(crate::SkillError::ValidationError(format!(
                    "Unknown algorithm: {}",
                    algorithm
                )));
            }
        };

        Ok(SkillOutput {
            success: true,
            result: Some(serde_json::json!({ "hash": hash, "algorithm": algorithm })),
            error: None,
            metadata: HashMap::new(),
            execution_time_ms: 0,
        })
    }
}

static HASH_DEFINITION: Lazy<SkillDefinition> = Lazy::new(|| SkillDefinition {
    id: "hash".to_string(),
    name: "Hash".to_string(),
    description: "Generate cryptographic hashes".to_string(),
    category: "crypto".to_string(),
    tags: vec![
        "hash".to_string(),
        "crypto".to_string(),
        "sha256".to_string(),
    ],
    parameters: vec![
        SkillParameter {
            name: "text".to_string(),
            param_type: "string".to_string(),
            description: "Text to hash".to_string(),
            required: true,
            default: None,
        },
        SkillParameter {
            name: "algorithm".to_string(),
            param_type: "string".to_string(),
            description: "Hash algorithm: sha256, sha512".to_string(),
            required: false,
            default: Some(serde_json::json!("sha256")),
        },
    ],
    returns: SkillReturnType {
        param_type: "object".to_string(),
        description: "Hash result".to_string(),
    },
    examples: vec![],
    permissions: vec![],
    rate_limit: None,
    version: "1.0.0".to_string(),
});

pub struct Base64Skill;

impl Default for Base64Skill {
    fn default() -> Self {
        Self::new()
    }
}

impl Base64Skill {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Skill for Base64Skill {
    fn definition(&self) -> &SkillDefinition {
        &BASE64_DEFINITION
    }

    async fn execute(&self, input: SkillInput) -> crate::SkillResult<SkillOutput> {
        let text = input
            .parameters
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'text' parameter".to_string())
            })?;

        let operation = input
            .parameters
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("encode");

        let result = match operation {
            "encode" => {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.encode(text.as_bytes())
            }
            "decode" => {
                use base64::Engine;
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(text)
                    .map_err(|e| crate::SkillError::ExecutionError(e.to_string()))?;
                String::from_utf8(decoded)
                    .map_err(|e| crate::SkillError::ExecutionError(e.to_string()))?
            }
            _ => {
                return Err(crate::SkillError::ValidationError(format!(
                    "Unknown operation: {}",
                    operation
                )));
            }
        };

        Ok(SkillOutput {
            success: true,
            result: Some(serde_json::json!({ "result": result })),
            error: None,
            metadata: HashMap::new(),
            execution_time_ms: 0,
        })
    }
}

static BASE64_DEFINITION: Lazy<SkillDefinition> = Lazy::new(|| SkillDefinition {
    id: "base64".to_string(),
    name: "Base64".to_string(),
    description: "Encode and decode Base64".to_string(),
    category: "crypto".to_string(),
    tags: vec![
        "base64".to_string(),
        "encode".to_string(),
        "decode".to_string(),
    ],
    parameters: vec![
        SkillParameter {
            name: "text".to_string(),
            param_type: "string".to_string(),
            description: "Text to encode/decode".to_string(),
            required: true,
            default: None,
        },
        SkillParameter {
            name: "operation".to_string(),
            param_type: "string".to_string(),
            description: "Operation: encode, decode".to_string(),
            required: false,
            default: Some(serde_json::json!("encode")),
        },
    ],
    returns: SkillReturnType {
        param_type: "string".to_string(),
        description: "Result of operation".to_string(),
    },
    examples: vec![],
    permissions: vec![],
    rate_limit: None,
    version: "1.0.0".to_string(),
});

pub struct RandomSkill;

impl Default for RandomSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl RandomSkill {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Skill for RandomSkill {
    fn definition(&self) -> &SkillDefinition {
        &RANDOM_DEFINITION
    }

    async fn execute(&self, input: SkillInput) -> crate::SkillResult<SkillOutput> {
        let operation = input
            .parameters
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'operation' parameter".to_string())
            })?;

        let result = match operation {
            "number" => {
                let min = input
                    .parameters
                    .get("min")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let max = input
                    .parameters
                    .get("max")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(100);
                let value = rand::random::<i64>() % (max - min + 1) + min;
                serde_json::json!({ "result": value })
            }
            "uuid" => {
                serde_json::json!({ "result": uuid::Uuid::new_v4().to_string() })
            }
            "string" => {
                use rand::Rng;
                let length = input
                    .parameters
                    .get("length")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(16) as usize;
                let mut rng = rand::rng();
                let chars: String = (0..length)
                    .map(|_| {
                        let idx = rng.random_range(0..62);
                        "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
                            .chars()
                            .nth(idx)
                            .unwrap_or('a')
                    })
                    .collect();
                serde_json::json!({ "result": chars })
            }
            "bool" => {
                serde_json::json!({ "result": rand::random::<bool>() })
            }
            _ => {
                return Err(crate::SkillError::ValidationError(format!(
                    "Unknown operation: {}",
                    operation
                )));
            }
        };

        Ok(SkillOutput {
            success: true,
            result: Some(result),
            error: None,
            metadata: HashMap::new(),
            execution_time_ms: 0,
        })
    }
}

static RANDOM_DEFINITION: Lazy<SkillDefinition> = Lazy::new(|| SkillDefinition {
    id: "random".to_string(),
    name: "Random".to_string(),
    description: "Generate random values".to_string(),
    category: "utilities".to_string(),
    tags: vec![
        "random".to_string(),
        "uuid".to_string(),
        "generator".to_string(),
    ],
    parameters: vec![SkillParameter {
        name: "operation".to_string(),
        param_type: "string".to_string(),
        description: "Operation: number, uuid, string, bool".to_string(),
        required: true,
        default: None,
    }],
    returns: SkillReturnType {
        param_type: "mixed".to_string(),
        description: "Random value".to_string(),
    },
    examples: vec![],
    permissions: vec![],
    rate_limit: None,
    version: "1.0.0".to_string(),
});

pub struct RegexSkill;

impl Default for RegexSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl RegexSkill {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Skill for RegexSkill {
    fn definition(&self) -> &SkillDefinition {
        &REGEX_DEFINITION
    }

    async fn execute(&self, input: SkillInput) -> crate::SkillResult<SkillOutput> {
        let pattern = input
            .parameters
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'pattern' parameter".to_string())
            })?;

        let text = input
            .parameters
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'text' parameter".to_string())
            })?;

        let operation = input
            .parameters
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("match");

        let re = regex::Regex::new(pattern)
            .map_err(|e| crate::SkillError::ValidationError(e.to_string()))?;

        let result = match operation {
            "match" => {
                let matches: Vec<&str> = re.find_iter(text).map(|m| m.as_str()).collect();
                serde_json::json!({ "matches": matches, "count": matches.len() })
            }
            "replace" => {
                let replacement = input
                    .parameters
                    .get("replacement")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let replaced = re.replace_all(text, replacement).to_string();
                serde_json::json!({ "result": replaced })
            }
            "split" => {
                let parts: Vec<&str> = re.split(text).collect();
                serde_json::json!({ "parts": parts, "count": parts.len() })
            }
            _ => {
                return Err(crate::SkillError::ValidationError(format!(
                    "Unknown operation: {}",
                    operation
                )));
            }
        };

        Ok(SkillOutput {
            success: true,
            result: Some(result),
            error: None,
            metadata: HashMap::new(),
            execution_time_ms: 0,
        })
    }
}

static REGEX_DEFINITION: Lazy<SkillDefinition> = Lazy::new(|| SkillDefinition {
    id: "regex".to_string(),
    name: "Regex".to_string(),
    description: "Regular expression operations".to_string(),
    category: "text".to_string(),
    tags: vec![
        "regex".to_string(),
        "pattern".to_string(),
        "match".to_string(),
    ],
    parameters: vec![
        SkillParameter {
            name: "pattern".to_string(),
            param_type: "string".to_string(),
            description: "Regular expression pattern".to_string(),
            required: true,
            default: None,
        },
        SkillParameter {
            name: "text".to_string(),
            param_type: "string".to_string(),
            description: "Text to operate on".to_string(),
            required: true,
            default: None,
        },
        SkillParameter {
            name: "operation".to_string(),
            param_type: "string".to_string(),
            description: "Operation: match, replace, split".to_string(),
            required: false,
            default: Some(serde_json::json!("match")),
        },
    ],
    returns: SkillReturnType {
        param_type: "object".to_string(),
        description: "Regex result".to_string(),
    },
    examples: vec![],
    permissions: vec![],
    rate_limit: None,
    version: "1.0.0".to_string(),
});

pub struct FilePathSkill;

impl Default for FilePathSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl FilePathSkill {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Skill for FilePathSkill {
    fn definition(&self) -> &SkillDefinition {
        &FILEPATH_DEFINITION
    }

    async fn execute(&self, input: SkillInput) -> crate::SkillResult<SkillOutput> {
        let path = input
            .parameters
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'path' parameter".to_string())
            })?;

        let operation = input
            .parameters
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("info");

        let path_obj = std::path::Path::new(path);

        let result = match operation {
            "info" => {
                serde_json::json!({
                    "exists": path_obj.exists(),
                    "is_file": path_obj.is_file(),
                    "is_dir": path_obj.is_dir(),
                    "is_symlink": path_obj.is_symlink(),
                    "extension": path_obj.extension().and_then(|e| e.to_str()),
                    "filename": path_obj.file_name().and_then(|n| n.to_str()),
                    "parent": path_obj.parent().and_then(|p| p.to_str()),
                    "stem": path_obj.file_stem().and_then(|s| s.to_str()),
                })
            }
            "join" => {
                let parts = input
                    .parameters
                    .get("parts")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| {
                        crate::SkillError::ValidationError("Missing 'parts' for join".to_string())
                    })?;

                let joined: std::path::PathBuf = parts.iter().filter_map(|p| p.as_str()).collect();

                serde_json::json!({ "path": joined.to_string_lossy() })
            }
            "normalize" => {
                let normalized = path_obj.to_string_lossy().to_string();
                serde_json::json!({ "path": normalized })
            }
            _ => {
                return Err(crate::SkillError::ValidationError(format!(
                    "Unknown operation: {}",
                    operation
                )));
            }
        };

        Ok(SkillOutput {
            success: true,
            result: Some(result),
            error: None,
            metadata: HashMap::new(),
            execution_time_ms: 0,
        })
    }
}

static FILEPATH_DEFINITION: Lazy<SkillDefinition> = Lazy::new(|| SkillDefinition {
    id: "filepath".to_string(),
    name: "File Path".to_string(),
    description: "File path operations".to_string(),
    category: "system".to_string(),
    tags: vec![
        "path".to_string(),
        "file".to_string(),
        "filesystem".to_string(),
    ],
    parameters: vec![
        SkillParameter {
            name: "path".to_string(),
            param_type: "string".to_string(),
            description: "File path".to_string(),
            required: true,
            default: None,
        },
        SkillParameter {
            name: "operation".to_string(),
            param_type: "string".to_string(),
            description: "Operation: info, join, normalize".to_string(),
            required: false,
            default: Some(serde_json::json!("info")),
        },
    ],
    returns: SkillReturnType {
        param_type: "object".to_string(),
        description: "Path information".to_string(),
    },
    examples: vec![],
    permissions: vec![],
    rate_limit: None,
    version: "1.0.0".to_string(),
});

pub struct UserAgentParserSkill;

impl Default for UserAgentParserSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl UserAgentParserSkill {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Skill for UserAgentParserSkill {
    fn definition(&self) -> &SkillDefinition {
        &USERAGENT_DEFINITION
    }

    async fn execute(&self, input: SkillInput) -> crate::SkillResult<SkillOutput> {
        let ua = input
            .parameters
            .get("user_agent")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'user_agent' parameter".to_string())
            })?;

        let mut result = serde_json::json!({
            "raw": ua,
        });

        if ua.contains("Firefox") {
            result["browser"] = serde_json::json!("Firefox");
        } else if ua.contains("Chrome") && !ua.contains("Edg") {
            result["browser"] = serde_json::json!("Chrome");
        } else if ua.contains("Edg") {
            result["browser"] = serde_json::json!("Edge");
        } else if ua.contains("Safari") {
            result["browser"] = serde_json::json!("Safari");
        }

        if ua.contains("Windows") {
            result["os"] = serde_json::json!("Windows");
        } else if ua.contains("Mac") {
            result["os"] = serde_json::json!("macOS");
        } else if ua.contains("Linux") {
            result["os"] = serde_json::json!("Linux");
        } else if ua.contains("Android") {
            result["os"] = serde_json::json!("Android");
        } else if ua.contains("iOS") || ua.contains("iPhone") {
            result["os"] = serde_json::json!("iOS");
        }

        if ua.contains("Mobile") {
            result["device"] = serde_json::json!("Mobile");
        } else {
            result["device"] = serde_json::json!("Desktop");
        }

        Ok(SkillOutput {
            success: true,
            result: Some(result),
            error: None,
            metadata: HashMap::new(),
            execution_time_ms: 0,
        })
    }
}

static USERAGENT_DEFINITION: Lazy<SkillDefinition> = Lazy::new(|| SkillDefinition {
    id: "useragent_parser".to_string(),
    name: "User Agent Parser".to_string(),
    description: "Parse user agent strings".to_string(),
    category: "network".to_string(),
    tags: vec![
        "useragent".to_string(),
        "parser".to_string(),
        "browser".to_string(),
    ],
    parameters: vec![SkillParameter {
        name: "user_agent".to_string(),
        param_type: "string".to_string(),
        description: "User agent string".to_string(),
        required: true,
        default: None,
    }],
    returns: SkillReturnType {
        param_type: "object".to_string(),
        description: "Parsed user agent info".to_string(),
    },
    examples: vec![],
    permissions: vec![],
    rate_limit: None,
    version: "1.0.0".to_string(),
});

pub struct ColorConverterSkill;

impl Default for ColorConverterSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl ColorConverterSkill {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Skill for ColorConverterSkill {
    fn definition(&self) -> &SkillDefinition {
        &COLOR_CONVERTER_DEFINITION
    }

    async fn execute(&self, input: SkillInput) -> crate::SkillResult<SkillOutput> {
        let color = input
            .parameters
            .get("color")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'color' parameter".to_string())
            })?;

        let from_hex = |hex: &str| -> Result<(u8, u8, u8), String> {
            let hex = hex.trim_start_matches('#');
            if hex.len() == 6 {
                let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| "Invalid hex")?;
                let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| "Invalid hex")?;
                let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| "Invalid hex")?;
                Ok((r, g, b))
            } else {
                Err("Invalid hex format".to_string())
            }
        };

        let (r, g, b) = from_hex(color).map_err(crate::SkillError::ValidationError)?;

        let to_hex = |r: u8, g: u8, b: u8| format!("#{:02x}{:02x}{:02x}", r, g, b);
        let to_hsl = |r: u8, g: u8, b: u8| {
            let r = r as f64 / 255.0;
            let g = g as f64 / 255.0;
            let b = b as f64 / 255.0;
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            let l = (max + min) / 2.0;

            if (max - min).abs() < f64::EPSILON {
                return (0.0, 0.0, l * 100.0);
            }

            let d = max - min;
            let s = if l > 0.5 {
                d / (2.0 - max - min)
            } else {
                d / (max + min)
            };

            let h = if (max - r).abs() < f64::EPSILON {
                ((g - b) / d + if g < b { 6.0 } else { 0.0 }) / 6.0
            } else if (max - g).abs() < f64::EPSILON {
                ((b - r) / d + 2.0) / 6.0
            } else {
                ((r - g) / d + 4.0) / 6.0
            };

            (h * 360.0, s * 100.0, l * 100.0)
        };

        let (h, s, l) = to_hsl(r, g, b);

        Ok(SkillOutput {
            success: true,
            result: Some(serde_json::json!({
                "hex": to_hex(r, g, b),
                "rgb": { "r": r, "g": g, "b": b },
                "hsl": { "h": h, "s": s, "l": l },
            })),
            error: None,
            metadata: HashMap::new(),
            execution_time_ms: 0,
        })
    }
}

static COLOR_CONVERTER_DEFINITION: Lazy<SkillDefinition> = Lazy::new(|| SkillDefinition {
    id: "color_converter".to_string(),
    name: "Color Converter".to_string(),
    description: "Convert between color formats".to_string(),
    category: "utilities".to_string(),
    tags: vec![
        "color".to_string(),
        "converter".to_string(),
        "hex".to_string(),
        "rgb".to_string(),
    ],
    parameters: vec![SkillParameter {
        name: "color".to_string(),
        param_type: "string".to_string(),
        description: "Color in hex format (#RRGGBB)".to_string(),
        required: true,
        default: None,
    }],
    returns: SkillReturnType {
        param_type: "object".to_string(),
        description: "Color in multiple formats".to_string(),
    },
    examples: vec![],
    permissions: vec![],
    rate_limit: None,
    version: "1.0.0".to_string(),
});

pub struct SlugifySkill;

impl Default for SlugifySkill {
    fn default() -> Self {
        Self::new()
    }
}

impl SlugifySkill {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Skill for SlugifySkill {
    fn definition(&self) -> &SkillDefinition {
        &SLUGIFY_DEFINITION
    }

    async fn execute(&self, input: SkillInput) -> crate::SkillResult<SkillOutput> {
        let text = input
            .parameters
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'text' parameter".to_string())
            })?;

        let separator = input
            .parameters
            .get("separator")
            .and_then(|v| v.as_str())
            .unwrap_or("-");

        let slug = text
            .to_lowercase()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() {
                    c
                } else {
                    separator.chars().next().unwrap_or('-')
                }
            })
            .collect::<String>()
            .split(separator)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(separator);

        Ok(SkillOutput {
            success: true,
            result: Some(serde_json::json!({ "slug": slug })),
            error: None,
            metadata: HashMap::new(),
            execution_time_ms: 0,
        })
    }
}

static SLUGIFY_DEFINITION: Lazy<SkillDefinition> = Lazy::new(|| SkillDefinition {
    id: "slugify".to_string(),
    name: "Slugify".to_string(),
    description: "Convert text to URL-friendly slug".to_string(),
    category: "text".to_string(),
    tags: vec!["slug".to_string(), "url".to_string(), "text".to_string()],
    parameters: vec![
        SkillParameter {
            name: "text".to_string(),
            param_type: "string".to_string(),
            description: "Text to slugify".to_string(),
            required: true,
            default: None,
        },
        SkillParameter {
            name: "separator".to_string(),
            param_type: "string".to_string(),
            description: "Separator character".to_string(),
            required: false,
            default: Some(serde_json::json!("-")),
        },
    ],
    returns: SkillReturnType {
        param_type: "string".to_string(),
        description: "Slugified text".to_string(),
    },
    examples: vec![],
    permissions: vec![],
    rate_limit: None,
    version: "1.0.0".to_string(),
});

pub struct CreditCardValidatorSkill;

impl Default for CreditCardValidatorSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl CreditCardValidatorSkill {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Skill for CreditCardValidatorSkill {
    fn definition(&self) -> &SkillDefinition {
        &CREDIT_CARD_VALIDATOR_DEFINITION
    }

    async fn execute(&self, input: SkillInput) -> crate::SkillResult<SkillOutput> {
        let card = input
            .parameters
            .get("card")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'card' parameter".to_string())
            })?;

        let digits: Vec<u32> = card
            .chars()
            .filter(|c| c.is_ascii_digit())
            .filter_map(|c| c.to_digit(10))
            .collect();

        if digits.len() < 13 || digits.len() > 19 {
            return Ok(SkillOutput {
                success: false,
                result: None,
                error: Some("Invalid card length".to_string()),
                metadata: HashMap::new(),
                execution_time_ms: 0,
            });
        }

        let mut sum = 0;
        let mut double = false;

        for &digit in digits.iter().rev() {
            let mut value = digit;
            if double {
                value *= 2;
                if value > 9 {
                    value -= 9;
                }
            }
            sum += value;
            double = !double;
        }

        let valid = sum % 10 == 0;

        let card_type = if card.starts_with("4") {
            "Visa"
        } else if card.starts_with("5") || (card.starts_with("2") && digits.len() >= 2) {
            let start = format!(
                "{}{}",
                card.chars().next().unwrap_or('0'),
                card.chars().nth(1).unwrap_or('0')
            );
            if ["51", "52", "53", "54", "55"].contains(&start.as_str()) {
                "Mastercard"
            } else {
                "Unknown"
            }
        } else if card.starts_with("34") || card.starts_with("37") {
            "Amex"
        } else if card.starts_with("6") {
            "Discover"
        } else {
            "Unknown"
        };

        Ok(SkillOutput {
            success: valid,
            result: Some(serde_json::json!({
                "valid": valid,
                "type": card_type,
                "last4": digits.iter().rev().take(4).collect::<Vec<_>>().iter().rev().map(|&d| d.to_string()).collect::<String>(),
            })),
            error: if valid {
                None
            } else {
                Some("Invalid card number".to_string())
            },
            metadata: HashMap::new(),
            execution_time_ms: 0,
        })
    }
}

static CREDIT_CARD_VALIDATOR_DEFINITION: Lazy<SkillDefinition> = Lazy::new(|| SkillDefinition {
    id: "credit_card_validator".to_string(),
    name: "Credit Card Validator".to_string(),
    description: "Validate credit card numbers using Luhn algorithm".to_string(),
    category: "validation".to_string(),
    tags: vec![
        "creditcard".to_string(),
        "validator".to_string(),
        "luhn".to_string(),
    ],
    parameters: vec![SkillParameter {
        name: "card".to_string(),
        param_type: "string".to_string(),
        description: "Credit card number".to_string(),
        required: true,
        default: None,
    }],
    returns: SkillReturnType {
        param_type: "object".to_string(),
        description: "Validation result".to_string(),
    },
    examples: vec![],
    permissions: vec![],
    rate_limit: None,
    version: "1.0.0".to_string(),
});

pub struct EmailValidatorSkill;

impl Default for EmailValidatorSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl EmailValidatorSkill {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Skill for EmailValidatorSkill {
    fn definition(&self) -> &SkillDefinition {
        &EMAIL_VALIDATOR_DEFINITION
    }

    async fn execute(&self, input: SkillInput) -> crate::SkillResult<SkillOutput> {
        let email = input
            .parameters
            .get("email")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'email' parameter".to_string())
            })?;

        let email_regex = regex::Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$")
            .map_err(|e| crate::SkillError::ExecutionError(e.to_string()))?;

        let valid = email_regex.is_match(email);

        let parts: Vec<&str> = email.split('@').collect();
        let (local_part, domain) = if parts.len() == 2 {
            (parts[0].to_string(), parts[1].to_string())
        } else {
            (String::new(), String::new())
        };

        Ok(SkillOutput {
            success: valid,
            result: Some(serde_json::json!({
                "valid": valid,
                "local_part": local_part,
                "domain": domain,
            })),
            error: if valid {
                None
            } else {
                Some("Invalid email format".to_string())
            },
            metadata: HashMap::new(),
            execution_time_ms: 0,
        })
    }
}

static EMAIL_VALIDATOR_DEFINITION: Lazy<SkillDefinition> = Lazy::new(|| SkillDefinition {
    id: "email_validator".to_string(),
    name: "Email Validator".to_string(),
    description: "Validate email addresses".to_string(),
    category: "validation".to_string(),
    tags: vec![
        "email".to_string(),
        "validator".to_string(),
        "validation".to_string(),
    ],
    parameters: vec![SkillParameter {
        name: "email".to_string(),
        param_type: "string".to_string(),
        description: "Email address to validate".to_string(),
        required: true,
        default: None,
    }],
    returns: SkillReturnType {
        param_type: "object".to_string(),
        description: "Validation result".to_string(),
    },
    examples: vec![],
    permissions: vec![],
    rate_limit: None,
    version: "1.0.0".to_string(),
});

pub struct TimezoneConverterSkill;

impl Default for TimezoneConverterSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl TimezoneConverterSkill {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Skill for TimezoneConverterSkill {
    fn definition(&self) -> &SkillDefinition {
        &TIMEZONE_CONVERTER_DEFINITION
    }

    async fn execute(&self, input: SkillInput) -> crate::SkillResult<SkillOutput> {
        let timestamp = input
            .parameters
            .get("timestamp")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| {
                crate::SkillError::ValidationError("Missing 'timestamp' parameter".to_string())
            })?;

        let from_tz = input
            .parameters
            .get("from")
            .and_then(|v| v.as_str())
            .unwrap_or("UTC");

        let to_tz = input
            .parameters
            .get("to")
            .and_then(|v| v.as_str())
            .unwrap_or("UTC");

        let dt = chrono::DateTime::from_timestamp(timestamp, 0)
            .ok_or_else(|| crate::SkillError::ExecutionError("Invalid timestamp".to_string()))?;

        Ok(SkillOutput {
            success: true,
            result: Some(serde_json::json!({
                "timestamp": timestamp,
                "from_timezone": from_tz,
                "to_timezone": to_tz,
                "converted": dt.to_rfc3339(),
            })),
            error: None,
            metadata: HashMap::new(),
            execution_time_ms: 0,
        })
    }
}

static TIMEZONE_CONVERTER_DEFINITION: Lazy<SkillDefinition> = Lazy::new(|| SkillDefinition {
    id: "timezone_converter".to_string(),
    name: "Timezone Converter".to_string(),
    description: "Convert timestamps between timezones".to_string(),
    category: "utilities".to_string(),
    tags: vec![
        "timezone".to_string(),
        "time".to_string(),
        "convert".to_string(),
    ],
    parameters: vec![
        SkillParameter {
            name: "timestamp".to_string(),
            param_type: "number".to_string(),
            description: "Unix timestamp".to_string(),
            required: true,
            default: None,
        },
        SkillParameter {
            name: "from".to_string(),
            param_type: "string".to_string(),
            description: "Source timezone".to_string(),
            required: false,
            default: Some(serde_json::json!("UTC")),
        },
        SkillParameter {
            name: "to".to_string(),
            param_type: "string".to_string(),
            description: "Target timezone".to_string(),
            required: false,
            default: Some(serde_json::json!("UTC")),
        },
    ],
    returns: SkillReturnType {
        param_type: "object".to_string(),
        description: "Converted time".to_string(),
    },
    examples: vec![],
    permissions: vec![],
    rate_limit: None,
    version: "1.0.0".to_string(),
});
