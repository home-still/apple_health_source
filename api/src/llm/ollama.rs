use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::AppError;
use crate::llm::LlmClient;
use crate::llm::prompt::{SYSTEM_PROMPT, meal_json_schema};
use crate::models::meal::ParsedMeal;

/// Ollama Cloud documents ~10-15s cold starts; 45s covers cold + one completion.
const LLM_REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

pub struct OllamaClient {
    http: Client,
    base_url: String,
    model: String,
    api_key: String,
}

impl OllamaClient {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        let http = Client::builder()
            .timeout(LLM_REQUEST_TIMEOUT)
            .build()
            .expect("failed to build reqwest client");
        Self {
            http,
            base_url,
            model,
            api_key,
        }
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    response_format: serde_json::Value,
    temperature: f32,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    content: String,
}

#[async_trait]
impl LlmClient for OllamaClient {
    async fn parse_meal(&self, text: &str) -> Result<ParsedMeal, AppError> {
        let body = ChatRequest {
            model: &self.model,
            messages: vec![
                ChatMessage { role: "system", content: SYSTEM_PROMPT },
                ChatMessage { role: "user", content: text },
            ],
            response_format: json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "parsed_meal",
                    "strict": true,
                    "schema": meal_json_schema()
                }
            }),
            temperature: 0.0,
        };

        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "LLM request failed");
                AppError::Internal("LLM request failed".into())
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!(%status, body = %truncate(&body, 500), "LLM non-2xx response");
            return Err(AppError::Internal("LLM upstream error".into()));
        }

        let parsed: ChatResponse = resp.json().await.map_err(|e| {
            tracing::warn!(error = %e, "LLM response envelope parse failed");
            AppError::Internal("LLM response envelope parse failed".into())
        })?;

        let content = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Internal("LLM returned no choices".into()))?
            .message
            .content;

        serde_json::from_str::<ParsedMeal>(&content).map_err(|e| {
            tracing::warn!(error = %e, body = %truncate(&content, 500), "LLM schema violation");
            AppError::Internal("LLM returned non-schema-conforming output".into())
        })
    }
}

/// Truncate a string to at most `max` bytes on a char boundary for safe logging.
fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn parses_structured_meal() {
        let server = MockServer::start().await;
        let body = json!({
            "choices": [{
                "message": {
                    "content": "{\"items\":[{\"food_name\":\"grilled chicken breast\",\"quantity\":6,\"unit\":\"oz\",\"preparation_method\":\"grilled\",\"confidence\":\"high\",\"database_search_terms\":[\"grilled chicken breast\",\"chicken breast\",\"chicken\"]}]}"
                }
            }]
        });
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let client = OllamaClient::new(
            "test-key".into(),
            server.uri(),
            "gpt-oss:120b-cloud".into(),
        );
        let meal = client.parse_meal("6oz grilled chicken").await.unwrap();
        assert_eq!(meal.items.len(), 1);
        assert_eq!(meal.items[0].food_name, "grilled chicken breast");
        assert!((meal.items[0].quantity - 6.0).abs() < f32::EPSILON);
    }
}
