use serde_json::{json, Value};
use std::time::Duration;

use super::{ProviderMessage, ProviderResponse, ProviderToolCall, ProviderUsage};
use crate::agent::error::AgentError;

/// Chat completions endpoint timeout mirrors llm-adapter.ts (60s for tool calls).
const TOOL_TIMEOUT: Duration = Duration::from_secs(60);
/// Retry budget and backoff mirror llm-adapter.ts::callWithRetry (3 tries, 1s/2s).
const MAX_ATTEMPTS: u32 = 3;

#[derive(Clone)]
pub struct OpenAiCompatibleProvider {
    base_url: String,
    model: String,
    api_key: String,
    temperature: f32,
    client: reqwest::Client,
    retry_delay: Duration,
}

impl OpenAiCompatibleProvider {
    pub fn new(base_url: String, model: String, api_key: String, temperature: f32) -> Self {
        let client = reqwest::Client::builder()
            .timeout(TOOL_TIMEOUT)
            .build()
            .expect("reqwest client with rustls must build");
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            model,
            api_key,
            temperature,
            client,
            retry_delay: Duration::from_secs(1),
        }
    }

    /// OpenAI-compatible chat/completions with tools. Mirrors
    /// llm-adapter.ts::callLLMWithTools: 401/403 are terminal, everything else
    /// (network, 429, 5xx, parse) retries up to MAX_ATTEMPTS with 1s/2s backoff.
    pub async fn chat(
        &self,
        messages: &[ProviderMessage],
        tools: &[Value],
    ) -> Result<ProviderResponse, AgentError> {
        let body = json!({
            "model": self.model,
            "messages": messages,
            "temperature": self.temperature,
            "stream": false,
            "tools": tools,
        });
        let url = format!("{}/chat/completions", self.base_url);
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.call_once(&url, &body).await {
                Ok(response) => return Ok(response),
                Err(outcome) => {
                    if !outcome.retryable || attempt >= MAX_ATTEMPTS {
                        return Err(outcome.error);
                    }
                    tokio::time::sleep(self.retry_delay * (1u32 << (attempt - 1))).await;
                }
            }
        }
    }

    async fn call_once(&self, url: &str, body: &Value) -> Result<ProviderResponse, CallOutcome> {
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await
            .map_err(|_| CallOutcome::retryable(AgentError::ProviderRequestFailed))?;
        let status = response.status().as_u16();
        // 401/403 = bad key/permission: terminal, do not retry (parity with TS).
        if status == 401 || status == 403 {
            return Err(CallOutcome::terminal(AgentError::ProviderRequestFailed));
        }
        if !response.status().is_success() {
            // 429 / 5xx / other 4xx: retryable (parity with TS callWithRetry).
            return Err(CallOutcome::retryable(AgentError::ProviderRequestFailed));
        }
        let data: Value = response
            .json()
            .await
            .map_err(|_| CallOutcome::retryable(AgentError::ProviderRequestFailed))?;
        Ok(parse_response(&data))
    }

    /// Test-only: shrink backoff so retry tests don't sleep for real seconds.
    #[cfg(test)]
    pub fn with_retry_delay(mut self, delay: Duration) -> Self {
        self.retry_delay = delay;
        self
    }
}

struct CallOutcome {
    error: AgentError,
    retryable: bool,
}

impl CallOutcome {
    fn terminal(error: AgentError) -> Self {
        Self {
            error,
            retryable: false,
        }
    }
    fn retryable(error: AgentError) -> Self {
        Self {
            error,
            retryable: true,
        }
    }
}

fn parse_response(data: &Value) -> ProviderResponse {
    let message = data.pointer("/choices/0/message").unwrap_or(&Value::Null);
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|array| array.iter().filter_map(parse_tool_call).collect())
        .unwrap_or_default();
    let usage = data
        .get("usage")
        .map(|usage| ProviderUsage {
            prompt_tokens: usage
                .get("prompt_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            completion_tokens: usage
                .get("completion_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0),
        })
        .unwrap_or_default();
    ProviderResponse {
        content,
        tool_calls,
        usage,
    }
}

fn parse_tool_call(value: &Value) -> Option<ProviderToolCall> {
    let id = value.get("id")?.as_str()?.to_owned();
    let function = value.get("function")?;
    let name = function.get("name")?.as_str()?.to_owned();
    let arguments = function
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}")
        .to_owned();
    Some(ProviderToolCall {
        id,
        kind: "function".to_owned(),
        function: super::ProviderFunction { name, arguments },
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_response, OpenAiCompatibleProvider};
    use crate::agent::llm::ProviderMessage;
    use httpmock::{Method, MockServer};
    use std::time::Duration;

    fn provider(server: &MockServer) -> OpenAiCompatibleProvider {
        OpenAiCompatibleProvider::new(
            server.base_url(),
            "test-model".into(),
            "sk-test".into(),
            0.2,
        )
        .with_retry_delay(Duration::from_millis(5))
    }

    fn user_message(content: &str) -> ProviderMessage {
        ProviderMessage {
            role: "user".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn parse_reads_content_tool_calls_and_usage() {
        let data = serde_json::json!({
            "choices": [{ "message": {
                "content": "hi",
                "tool_calls": [{ "id": "c1", "type": "function",
                    "function": { "name": "plan.get_today", "arguments": "{\"exam_id\":\"e1\"}" } }]
            }}],
            "usage": { "prompt_tokens": 10, "completion_tokens": 2 }
        });
        let resp = parse_response(&data);
        assert_eq!(resp.content.as_deref(), Some("hi"));
        assert_eq!(resp.tool_calls[0].id, "c1");
        assert_eq!(resp.tool_calls[0].function.name, "plan.get_today");
        assert_eq!(
            resp.tool_calls[0].function.arguments,
            "{\"exam_id\":\"e1\"}"
        );
        assert_eq!(resp.usage.prompt_tokens, 10);
        assert_eq!(resp.usage.completion_tokens, 2);
    }

    #[test]
    fn parse_defaults_missing_fields_to_empty_zero() {
        let data = serde_json::json!({ "choices": [{ "message": {} }], "usage": {} });
        let resp = parse_response(&data);
        assert!(resp.content.is_none());
        assert!(resp.tool_calls.is_empty());
        assert_eq!(resp.usage.prompt_tokens, 0);
        assert_eq!(resp.usage.completion_tokens, 0);
    }

    #[tokio::test]
    async fn parses_content_tool_calls_and_usage_from_chat_completions() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(Method::POST).path("/chat/completions");
            then.status(200).json_body(serde_json::json!({
                "choices": [{ "message": {
                    "content": "今日计划如下",
                    "tool_calls": [{ "id": "call_1", "type": "function",
                        "function": { "name": "plan.get_today", "arguments": "{\"exam_id\":\"e1\"}" } }]
                }}],
                "usage": { "prompt_tokens": 120, "completion_tokens": 8 }
            }));
        });

        let resp = provider(&server)
            .chat(&[user_message("看今天")], &[])
            .await
            .unwrap();

        assert_eq!(resp.content.as_deref(), Some("今日计划如下"));
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].function.name, "plan.get_today");
        assert_eq!(resp.usage.prompt_tokens, 120);
        assert_eq!(resp.usage.completion_tokens, 8);
        assert_eq!(mock.hits(), 1);
    }

    #[tokio::test]
    async fn does_not_retry_on_401_and_redacts_the_key() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(Method::POST).path("/chat/completions");
            then.status(401).body("unauthorized");
        });

        let error = provider(&server)
            .chat(&[user_message("看今天")], &[])
            .await
            .unwrap_err();

        assert_eq!(error.code(), "provider_request_failed");
        assert!(!error.to_string().contains("sk-test"));
        assert!(!error.to_string().contains(&server.base_url()));
        assert_eq!(mock.hits(), 1);
    }

    #[tokio::test]
    async fn retries_5xx_three_times_then_fails() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(Method::POST).path("/chat/completions");
            then.status(500).body("boom");
        });

        let error = provider(&server)
            .chat(&[user_message("看今天")], &[])
            .await
            .unwrap_err();

        assert_eq!(error.code(), "provider_request_failed");
        assert_eq!(mock.hits(), 3);
    }

    #[tokio::test]
    async fn redacts_url_and_key_from_repeated_5xx_failure() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(Method::POST).path("/chat/completions");
            then.status(500).body("boom");
        });

        let error = provider(&server)
            .chat(&[user_message("看今天")], &[])
            .await
            .unwrap_err();

        assert_eq!(error.code(), "provider_request_failed");
        assert!(!error.to_string().contains(&server.base_url()));
        assert!(!error.to_string().contains("sk-test"));
        assert_eq!(mock.hits(), 3);
    }
}
