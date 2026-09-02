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

    /// The configured model name, surfaced by the provider test result. It is
    /// part of the non-sensitive settings, never a secret.
    pub fn model(&self) -> &str {
        &self.model
    }

    pub(crate) fn temperature(&self) -> f32 {
        self.temperature
    }

    /// Streaming chat/completions. The request sets `stream:true` and
    /// `stream_options.include_usage` so usage arrives in the final chunk.
    /// Each content delta is forwarded to `on_chunk`; tool_calls are reassembled
    /// across deltas by `index`. Errors map to stable codes: 401/403 ->
    /// auth_failed (terminal), 429 -> rate_limited (retried), client timeout ->
    /// timeout (retried), malformed/empty streams -> protocol_error (terminal),
    /// other network/5xx failures -> request_failed (retried up to MAX_ATTEMPTS
    /// with 1s/2s backoff). Once streaming starts, a mid-stream error is
    /// terminal (chunks may already be emitted).
    pub async fn chat_stream(
        &self,
        messages: &[ProviderMessage],
        tools: &[Value],
        on_chunk: &mut (dyn FnMut(&str) + Send),
    ) -> Result<ProviderResponse, AgentError> {
        let body = Self::serialize_request_body(&self.model, messages, self.temperature, tools);
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

    /// Serialize the exact request body `chat_stream` would send. Exposed so
    /// the Planner can enforce `MAX_REQUEST_BYTES` (Task 6) before anything
    /// leaves the machine: the tool schema is part of the body, so the content
    /// caps alone cannot bound it.
    pub(crate) fn serialize_request_body(
        model: &str,
        messages: &[ProviderMessage],
        temperature: f32,
        tools: &[Value],
    ) -> Value {
        let mut body = json!({
            "model": model,
            "messages": messages,
            "temperature": temperature,
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        if !tools.is_empty() {
            body["tools"] = serde_json::Value::Array(tools.to_vec());
        }
        body
    }

    /// Fixed diagnostic prompt for the text-stream capability probe. It
    /// contains no business data — no exams, plans, records, wrong questions,
    /// or conversation content — so it may be sent before any data-export
    /// consent, and it must NOT trigger a tool call.
    pub const DIAGNOSTIC_TEXT_PROMPT: &'static str = "请只回复 OK，不要调用工具。";

    /// Fixed diagnostic prompt for the tool-call capability probe: it must
    /// trigger the `diagnostics_ping` tool and nothing else. Using a
    /// tool-calling prompt for the tool probe (never a "don't use tools"
    /// prompt) keeps the two probes unambiguous. The name carries no dot:
    /// DeepSeek only accepts function names matching `^[a-zA-Z0-9_-]+$`.
    pub const DIAGNOSTIC_TOOL_PROMPT: &'static str = "请调用 diagnostics_ping，不要输出文本。";

    /// The static `diagnostics_ping` tool schema offered during capability
    /// tests. Providers must accept it without any business context attached.
    /// No dot in the name: DeepSeek rejects `^[a-zA-Z0-9_.-]`-style names.
    pub fn diagnostics_ping_tool() -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "diagnostics_ping",
                "description": "固定诊断工具：收到后调用它并返回 pong。",
                "parameters": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }
            }
        })
    }

    /// Probe provider capabilities with fixed diagnostic traffic only:
    ///
    /// 1. a text-stream request (`DIAGNOSTIC_TEXT_PROMPT`) that must yield at
    ///    least one content delta,
    /// 2. a `diagnostics_ping` tool-call request (`DIAGNOSTIC_TOOL_PROMPT`)
    ///    that must return a legal `diagnostics_ping` tool_call.
    ///
    /// A missing/interrupted text stream or a tool probe without a real
    /// `diagnostics_ping` call is a `provider_protocol_error` — never a
    /// success with false capabilities. Never attaches exam/plan/record/
    /// wrong-question/session data.
    pub async fn probe_capabilities(&self) -> Result<CapabilityProbe, AgentError> {
        let text_message = ProviderMessage {
            role: "user".into(),
            content: Some(Self::DIAGNOSTIC_TEXT_PROMPT.to_owned()),
            tool_calls: None,
            tool_call_id: None,
        };
        let mut deltas = 0_usize;
        let text_response = self
            .chat_stream(std::slice::from_ref(&text_message), &[], &mut |_| {
                deltas += 1
            })
            .await?;
        let text_stream = deltas > 0
            && !text_response
                .content
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty();
        if !text_stream {
            return Err(AgentError::ProviderProtocolError);
        }

        let tool_message = ProviderMessage {
            role: "user".into(),
            content: Some(Self::DIAGNOSTIC_TOOL_PROMPT.to_owned()),
            tool_calls: None,
            tool_call_id: None,
        };
        let tool_response = self
            .chat_stream(
                std::slice::from_ref(&tool_message),
                &[Self::diagnostics_ping_tool()],
                &mut |_| {},
            )
            .await?;
        let tool_call = tool_response
            .tool_calls
            .iter()
            .any(|call| call.function.name == "diagnostics_ping");
        if !tool_call {
            return Err(AgentError::ProviderProtocolError);
        }
        Ok(CapabilityProbe {
            text_stream: true,
            tool_call: true,
        })
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
            .map_err(|error| {
                // Temporary diagnostic (probe only): surface the transport
                // error classification + source for the manual-test debugging
                // session. No key, URL, request body, or response body is
                // printed (reqwest's own Display embeds the URL, so only the
                // source chain is logged).
                #[cfg(not(test))]
                eprintln!(
                    "[probe] provider send failed: request={} connect={} timeout={} status={} source={}",
                    error.is_request(),
                    error.is_connect(),
                    error.is_timeout(),
                    error.status().map(|status| status.as_u16()).unwrap_or(0),
                    std::error::Error::source(&error)
                        .map(|source| source.to_string())
                        .unwrap_or_default()
                );
                if error.is_timeout() {
                    CallOutcome::retryable(AgentError::ProviderTimeout)
                } else {
                    CallOutcome::retryable(AgentError::ProviderRequestFailed)
                }
            })?;
        let status = response.status().as_u16();
        if status == 401 || status == 403 {
            #[cfg(not(test))]
            eprintln!("[probe] provider http status: {status}");
            return Err(CallOutcome::terminal(AgentError::ProviderAuthFailed));
        }
        if status == 429 {
            #[cfg(not(test))]
            eprintln!("[probe] provider http status: {status}");
            return Err(CallOutcome::retryable(AgentError::ProviderRateLimited));
        }
        if !response.status().is_success() {
            // Temporary diagnostic (probe only): capture the provider's error
            // body so the manual-test session can see why the request was
            // rejected (e.g. invalid model, unsupported field). Truncated;
            // the body is a provider error message, never the API key.
            #[cfg(not(test))]
            {
                let body_text = response.text().await.unwrap_or_default();
                let body_text: String = body_text.chars().take(300).collect();
                eprintln!("[probe] provider http status: {status} body: {body_text}");
            }
            return Err(CallOutcome::retryable(AgentError::ProviderRequestFailed));
        }
        // From here chunks may have been emitted; a failure is terminal, not retried.
        parse_sse(&mut response, on_chunk)
            .await
            .map_err(|error| match error {
                AgentError::ProviderTimeout => CallOutcome::terminal(AgentError::ProviderTimeout),
                _ => CallOutcome::terminal(AgentError::ProviderProtocolError),
            })
    }

    /// Test-only: shrink the client timeout so timeout tests don't wait 60s.
    #[cfg(test)]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("reqwest client with rustls must build");
        self
    }

    /// Test-only: shrink backoff so retry tests don't sleep for real seconds.
    #[cfg(test)]
    pub fn with_retry_delay(mut self, delay: Duration) -> Self {
        self.retry_delay = delay;
        self
    }
}

/// Outcome of a provider capability probe: whether the provider streams text
/// and whether it returns a legal `diagnostics_ping` tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityProbe {
    pub text_stream: bool,
    pub tool_call: bool,
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
///
/// Errors: a client timeout while reading the body maps to `ProviderTimeout`;
/// a body that ends without any content, tool call, or usage (empty response)
/// maps to `ProviderProtocolError`.
async fn parse_sse(
    response: &mut reqwest::Response,
    on_chunk: &mut (dyn FnMut(&str) + Send),
) -> Result<ProviderResponse, AgentError> {
    let mut content = String::new();
    let mut tool_calls: Vec<ProviderToolCall> = Vec::new();
    let mut usage = ProviderUsage::default();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        if error.is_timeout() {
            AgentError::ProviderTimeout
        } else {
            AgentError::ProviderProtocolError
        }
    })? {
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
    if content.is_empty()
        && tool_calls.is_empty()
        && usage.prompt_tokens == 0
        && usage.completion_tokens == 0
    {
        return Err(AgentError::ProviderProtocolError);
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
        prompt_cache_hit_tokens: value
            .get("prompt_cache_hit_tokens")
            .or_else(|| value.get("cached_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        prompt_cache_miss_tokens: value
            .get("prompt_cache_miss_tokens")
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

        assert_eq!(error.code(), "provider_auth_failed");
        assert!(!error.to_string().contains("sk-test"));
        assert!(!error.to_string().contains(&server.base_url()));
        assert_eq!(mock.hits(), 1);
    }

    #[tokio::test]
    async fn does_not_retry_on_403_and_maps_to_auth_failed() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(Method::POST).path("/chat/completions");
            then.status(403).body("forbidden");
        });

        let error = provider(&server)
            .chat_stream(&[user_message("看今天")], &[], &mut |_| {})
            .await
            .unwrap_err();

        assert_eq!(error.code(), "provider_auth_failed");
        assert_eq!(mock.hits(), 1);
    }

    #[tokio::test]
    async fn retries_429_three_times_then_reports_rate_limited() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(Method::POST).path("/chat/completions");
            then.status(429).body("rate limited");
        });

        let error = provider(&server)
            .chat_stream(&[user_message("看今天")], &[], &mut |_| {})
            .await
            .unwrap_err();

        assert_eq!(error.code(), "provider_rate_limited");
        assert_eq!(mock.hits(), 3);
    }

    #[tokio::test]
    async fn client_timeout_maps_to_provider_timeout() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(Method::POST).path("/chat/completions");
            then.delay(std::time::Duration::from_millis(500))
                .status(200)
                .body("data: [DONE]\n\n");
        });

        let error = provider(&server)
            .with_timeout(std::time::Duration::from_millis(100))
            .chat_stream(&[user_message("看今天")], &[], &mut |_| {})
            .await
            .unwrap_err();

        assert_eq!(error.code(), "provider_timeout");
    }

    #[tokio::test]
    async fn connection_failure_maps_to_provider_request_failed() {
        // Port 1 is practically never listening; the request must fail at the
        // network layer instead of returning an HTTP response.
        let provider = OpenAiCompatibleProvider::new(
            "http://127.0.0.1:1".into(),
            "test-model".into(),
            "sk-test".into(),
            0.2,
        )
        .with_retry_delay(std::time::Duration::from_millis(5));

        let error = provider
            .chat_stream(&[user_message("看今天")], &[], &mut |_| {})
            .await
            .unwrap_err();

        assert_eq!(error.code(), "provider_request_failed");
        assert!(!error.to_string().contains("sk-test"));
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
    async fn empty_stream_maps_to_protocol_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(Method::POST).path("/chat/completions");
            then.status(200).body("data: [DONE]\n\n");
        });

        let error = provider(&server)
            .chat_stream(&[user_message("看今天")], &[], &mut |_| {})
            .await
            .unwrap_err();

        assert_eq!(error.code(), "provider_protocol_error");
    }

    #[tokio::test]
    async fn garbage_sse_maps_to_protocol_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(Method::POST).path("/chat/completions");
            then.status(200)
                .body("data: not-json-at-all\n\ndata: [DONE]\n\n");
        });

        let error = provider(&server)
            .chat_stream(&[user_message("看今天")], &[], &mut |_| {})
            .await
            .unwrap_err();

        assert_eq!(error.code(), "provider_protocol_error");
    }

    #[tokio::test]
    async fn probe_capabilities_reports_text_stream_and_tool_call() {
        let server = MockServer::start();
        // Text probe: the fixed text prompt, no tools offered.
        server.mock(|when, then| {
            when.method(Method::POST)
                .path("/chat/completions")
                .matches(|request| {
                    let body = String::from_utf8_lossy(request.body.as_deref().unwrap_or(b""));
                    body.contains("请只回复 OK，不要调用工具") && !body.contains("diagnostics_ping")
                });
            then.status(200).body(concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK\"}}]}\n\n",
                "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n",
            ));
        });
        // Tool probe: the fixed tool prompt plus the diagnostics_ping schema.
        server.mock(|when, then| {
            when.method(Method::POST)
                .path("/chat/completions")
                .body_contains("diagnostics_ping")
                .body_contains("请调用 diagnostics_ping");
            then.status(200).body(concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_ping\",\"function\":{\"name\":\"diagnostics_ping\",\"arguments\":\"{}\"}}]}}]}\n\n",
                "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
                "data: [DONE]\n\n",
            ));
        });

        let probe = provider(&server).probe_capabilities().await.unwrap();
        assert!(probe.text_stream);
        assert!(probe.tool_call);
    }

    #[tokio::test]
    async fn probe_fails_with_protocol_error_when_the_text_stream_is_empty() {
        let server = MockServer::start();
        // The text probe streams no content delta at all (only a finish).
        server.mock(|when, then| {
            when.method(Method::POST).path("/chat/completions");
            then.status(200).body(concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n",
            ));
        });

        let error = provider(&server).probe_capabilities().await.unwrap_err();
        assert_eq!(error.code(), "provider_protocol_error");
    }

    #[tokio::test]
    async fn probe_fails_with_protocol_error_when_the_stream_is_interrupted() {
        let server = MockServer::start();
        // The text probe starts streaming but the SSE body ends mid-line (no
        // terminating newline), so the parser discards the unfinished line and
        // the probe sees no completed content — a protocol error, never a
        // silent "no text capability".
        server.mock(|when, then| {
            when.method(Method::POST).path("/chat/completions");
            then.status(200)
                .body("data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK\"}}]}");
        });

        let error = provider(&server).probe_capabilities().await.unwrap_err();
        assert_eq!(error.code(), "provider_protocol_error");
    }

    #[tokio::test]
    async fn probe_fails_with_protocol_error_when_the_tool_probe_returns_no_tool_call() {
        let server = MockServer::start();
        // Text probe succeeds (content delta present).
        server.mock(|when, then| {
            when.method(Method::POST)
                .path("/chat/completions")
                .matches(|request| {
                    let body = String::from_utf8_lossy(request.body.as_deref().unwrap_or(b""));
                    !body.contains("diagnostics_ping")
                });
            then.status(200).body(concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK\"}}]}\n\n",
                "data: [DONE]\n\n",
            ));
        });
        // Tool probe responds with plain text instead of calling the tool.
        server.mock(|when, then| {
            when.method(Method::POST)
                .path("/chat/completions")
                .body_contains("diagnostics_ping");
            then.status(200).body(concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"我不会调用工具\"}}]}\n\n",
                "data: [DONE]\n\n",
            ));
        });

        let error = provider(&server).probe_capabilities().await.unwrap_err();
        assert_eq!(error.code(), "provider_protocol_error");
    }

    #[tokio::test]
    async fn probe_requests_never_carry_business_data() {
        let server = MockServer::start();
        // The mock matches only diagnostic traffic: the fixed prompts, with
        // none of the business category words and no key material in the body.
        let mock = server.mock(|when, then| {
            when.method(Method::POST)
                .path("/chat/completions")
                .matches(|request| {
                    let body = String::from_utf8_lossy(request.body.as_deref().unwrap_or(b""));
                    body.contains("请只回复 OK")
                        && !body.contains("diagnostics_ping")
                        && ![
                            "exam", "plan", "record", "wrong", "study", "session", "sk-test",
                        ]
                        .iter()
                        .any(|word| body.to_lowercase().contains(word))
                });
            then.status(200).body(concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK\"}}]}\n\n",
                "data: [DONE]\n\n",
            ));
        });
        // The tool probe needs its own response with a legal ping tool call;
        // the text probe matcher above would swallow it otherwise.
        server.mock(|when, then| {
            when.method(Method::POST)
                .path("/chat/completions")
                .body_contains("diagnostics_ping");
            then.status(200).body(concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_ping\",\"function\":{\"name\":\"diagnostics_ping\",\"arguments\":\"{}\"}}]}}]}\n\n",
                "data: [DONE]\n\n",
            ));
        });

        let probe = provider(&server).probe_capabilities().await.unwrap();
        assert!(probe.text_stream);
        assert!(probe.tool_call);
        // The no-business-data matcher saw exactly the text probe; the tool
        // probe carried only the fixed ping schema and prompt.
        assert_eq!(mock.hits(), 1);
    }
}
