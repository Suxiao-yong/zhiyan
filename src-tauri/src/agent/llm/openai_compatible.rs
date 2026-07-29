use serde_json::{json, Value};
use std::time::Duration;

use super::{ProviderFunction, ProviderMessage, ProviderResponse, ProviderToolCall, ProviderUsage};
use crate::agent::error::AgentError;

/// Chat completions timeout mirrors llm-adapter.ts (60s for tool calls).
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

    /// Streaming chat/completions. The request sets `stream:true` and
    /// `stream_options.include_usage` so usage arrives in the final chunk.
    /// Each content delta is forwarded to `on_chunk`; tool_calls are reassembled
    /// across deltas by `index`. 401/403 are terminal; pre-2xx failures (network,
    /// 429, 5xx) retry up to MAX_ATTEMPTS with 1s/2s backoff; once streaming
    /// starts, a mid-stream error is terminal (chunks may already be emitted).
    pub async fn chat_stream(
        &self,
        messages: &[ProviderMessage],
        tools: &[Value],
        on_chunk: &mut (dyn FnMut(&str) + Send),
    ) -> Result<ProviderResponse, AgentError> {
        let body = json!({
            "model": self.model,
            "messages": messages,
            "temperature": self.temperature,
            "stream": true,
            "stream_options": { "include_usage": true },
            "tools": tools,
        });
        let url = format!("{}/chat/completions", self.base_url);
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.call_stream_once(&url, &body, on_chunk).await {
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

    async fn call_stream_once(
        &self,
        url: &str,
        body: &Value,
        on_chunk: &mut (dyn FnMut(&str) + Send),
    ) -> Result<ProviderResponse, CallOutcome> {
        let mut response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await
            .map_err(|_| CallOutcome::retryable(AgentError::ProviderRequestFailed))?;
        let status = response.status().as_u16();
        if status == 401 || status == 403 {
            return Err(CallOutcome::terminal(AgentError::ProviderRequestFailed));
        }
        if !response.status().is_success() {
            return Err(CallOutcome::retryable(AgentError::ProviderRequestFailed));
        }
        // From here chunks may have been emitted; a failure is terminal, not retried.
        parse_sse(&mut response, on_chunk)
            .await
            .map_err(|_| CallOutcome::terminal(AgentError::ProviderRequestFailed))
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

/// Parse the Server-Sent Events body: accumulate `delta.content` (forwarding
/// each to `on_chunk`), reassemble `delta.tool_calls` by `index`, and capture
/// the final `usage` chunk. Lines are buffered by byte so a chunk boundary
/// never splits a line or a multi-byte UTF-8 character mid-line.
async fn parse_sse(
    response: &mut reqwest::Response,
    on_chunk: &mut (dyn FnMut(&str) + Send),
) -> Result<ProviderResponse, AgentError> {
    let mut content = String::new();
    let mut tool_calls: Vec<ProviderToolCall> = Vec::new();
    let mut usage = ProviderUsage::default();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| AgentError::ProviderRequestFailed)?
    {
        buf.extend_from_slice(&chunk);
        while let Some(pos) = buf.iter().position(|byte| *byte == b'\n') {
            let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
            let line = std::str::from_utf8(&line_bytes)
                .unwrap_or("")
                .trim_end_matches('\r');
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            if data.trim() == "[DONE]" {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if let Some(usage_value) = value.get("usage") {
                usage = parse_usage(usage_value);
            }
            let Some(delta) = value.pointer("/choices/0/delta") else {
                continue;
            };
            if let Some(text) = delta.get("content").and_then(Value::as_str) {
                content.push_str(text);
                on_chunk(text);
            }
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for call in calls {
                    let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                    while tool_calls.len() <= index {
                        tool_calls.push(ProviderToolCall {
                            id: String::new(),
                            kind: "function".to_owned(),
                            function: ProviderFunction {
                                name: String::new(),
                                arguments: String::new(),
                            },
                        });
                    }
                    let slot = &mut tool_calls[index];
                    if let Some(id) = call.get("id").and_then(Value::as_str) {
                        slot.id = id.to_owned();
                    }
                    if let Some(function) = call.get("function") {
                        if let Some(name) = function.get("name").and_then(Value::as_str) {
                            slot.function.name = name.to_owned();
                        }
                        if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                            slot.function.arguments.push_str(arguments);
                        }
                    }
                }
            }
        }
    }
    Ok(ProviderResponse {
        content: (!content.is_empty()).then_some(content),
        tool_calls,
        usage,
    })
}

fn parse_usage(value: &Value) -> ProviderUsage {
    ProviderUsage {
        prompt_tokens: value
            .get("prompt_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        completion_tokens: value
            .get("completion_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::OpenAiCompatibleProvider;
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

    #[tokio::test]
    async fn streams_content_tool_calls_and_usage_from_sse() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(Method::POST).path("/chat/completions");
            then.status(200).body(
                concat!(
                    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"今日计划如下\"}}]}\n\n",
                    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"plan.get_today\",\"arguments\":\"\"}}]}}]}\n\n",
                    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"exam_id\\\":\\\"e1\\\"}\"}}]}}]}\n\n",
                    "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
                    "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":120,\"completion_tokens\":8}}\n\n",
                    "data: [DONE]\n\n",
                ),
            );
        });

        let mut chunks = Vec::new();
        let resp = provider(&server)
            .chat_stream(&[user_message("看今天")], &[], &mut |c| {
                chunks.push(c.to_owned())
            })
            .await
            .unwrap();

        assert_eq!(resp.content.as_deref(), Some("今日计划如下"));
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].id, "call_1");
        assert_eq!(resp.tool_calls[0].function.name, "plan.get_today");
        assert_eq!(
            resp.tool_calls[0].function.arguments,
            "{\"exam_id\":\"e1\"}"
        );
        assert_eq!(resp.usage.prompt_tokens, 120);
        assert_eq!(resp.usage.completion_tokens, 8);
        assert_eq!(chunks, vec!["今日计划如下".to_owned()]);
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
            .chat_stream(&[user_message("看今天")], &[], &mut |_| {})
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
            .chat_stream(&[user_message("看今天")], &[], &mut |_| {})
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
            .chat_stream(&[user_message("看今天")], &[], &mut |_| {})
            .await
            .unwrap_err();

        assert_eq!(error.code(), "provider_request_failed");
        assert!(!error.to_string().contains(&server.base_url()));
        assert!(!error.to_string().contains("sk-test"));
        assert_eq!(mock.hits(), 3);
    }

    #[tokio::test]
    async fn empty_stream_defaults_to_no_content_and_zero_usage() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(Method::POST).path("/chat/completions");
            then.status(200).body("data: [DONE]\n\n");
        });

        let mut chunks = Vec::new();
        let resp = provider(&server)
            .chat_stream(&[user_message("看今天")], &[], &mut |c| {
                chunks.push(c.to_owned())
            })
            .await
            .unwrap();

        assert!(resp.content.is_none());
        assert!(resp.tool_calls.is_empty());
        assert_eq!(resp.usage.prompt_tokens, 0);
        assert_eq!(resp.usage.completion_tokens, 0);
        assert!(chunks.is_empty());
    }
}
