use crate::types::{
    ChatMessage, HkuEmbeddingResult, ImageInput, ModelCapabilities, ModelInfo, QuantizationInfo,
    ReasoningEffort, ThinkingMode, ToolCall, ToolDefinition, UsageStats,
};
use base64::Engine;
use futures_util::StreamExt;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};
use thiserror::Error;
use uuid::Uuid;

// Large local models can spend a few minutes composing a single structured
// filesystem_write argument. Keep an absolute bound, but do not mistake that
// normal tool-argument pause for a dead stream.
const STREAM_DEADLINE: Duration = Duration::from_secs(300);
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(180);
const CLOUD_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Error)]
pub enum InferenceError {
    #[error("Inference provider is unreachable at {0}")]
    Unreachable(String),
    #[error("Inference provider returned {status}: {body}")]
    Api { status: StatusCode, body: String },
    #[error("invalid inference-provider response: {0}")]
    Parse(String),
    #[error("request cancelled")]
    Cancelled,
    #[error("Inference-provider stream was idle for {idle_seconds} seconds")]
    IdleTimeout { idle_seconds: u64 },
    #[error("transport error: {0}")]
    Transport(String),
}

#[derive(Clone)]
pub struct LmStudioProvider {
    pub base_url: String,
    pub client: Client,
    api_key: Option<String>,
    chat_completions_path: String,
    models_path: String,
    provider_name: String,
    reasoning_style: ReasoningStyle,
    openrouter_headers: bool,
    api_key_header: Option<String>,
    hku_claude_converse: bool,
    hku_route: Option<HkuRoute>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HkuRoute {
    DeepSeek,
    VertexChat,
    Gemini,
    AzureChat,
    AzureResponses,
    AzureImage,
    AzureEmbedding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReasoningStyle {
    Effort,
    DeepSeekThinking,
    OpenRouter,
    None,
}

#[derive(Debug, Default)]
pub struct StreamResult {
    pub text: String,
    pub reasoning: String,
    pub warning: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub usage: UsageStats,
    pub transport: String,
    pub images: Vec<ImageInput>,
}

impl LmStudioProvider {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client: Client::builder()
                .connect_timeout(std::time::Duration::from_secs(3))
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .expect("reqwest client should build"),
            api_key: None,
            chat_completions_path: "/v1/chat/completions".into(),
            models_path: "/api/v1/models".into(),
            provider_name: "LM Studio".into(),
            reasoning_style: ReasoningStyle::Effort,
            openrouter_headers: false,
            api_key_header: None,
            hku_claude_converse: false,
            hku_route: None,
        }
    }

    pub fn openai_compatible(
        base_url: impl Into<String>,
        api_key: String,
        chat_completions_path: impl Into<String>,
        models_path: impl Into<String>,
        provider_name: impl Into<String>,
        reasoning_style: ReasoningStyle,
        openrouter_headers: bool,
    ) -> Self {
        let mut provider = Self::new(base_url);
        provider.client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(600))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("compatible provider HTTP client should build");
        provider.api_key = Some(api_key);
        provider.chat_completions_path = chat_completions_path.into();
        provider.models_path = models_path.into();
        provider.provider_name = provider_name.into();
        provider.reasoning_style = reasoning_style;
        provider.openrouter_headers = openrouter_headers;
        provider
    }

    pub fn with_api_key_header(mut self, header: impl Into<String>) -> Self {
        self.api_key_header = Some(header.into());
        self
    }

    pub fn hku_claude_converse(base_url: impl Into<String>, api_key: String) -> Self {
        let mut provider = Self::openai_compatible(
            base_url,
            api_key,
            "/unused",
            "/unused",
            "HKU Bedrock Claude",
            ReasoningStyle::None,
            false,
        )
        .with_api_key_header("api-key");
        provider.hku_claude_converse = true;
        provider
    }

    pub fn hku_for_model(base_url: impl Into<String>, api_key: String, model: &str) -> Self {
        let route = if model.starts_with("DeepSeek-") {
            HkuRoute::DeepSeek
        } else if model.starts_with("qwen/") || model.starts_with("zai-org/") {
            HkuRoute::VertexChat
        } else if model.starts_with("gemini-") {
            HkuRoute::Gemini
        } else if model.starts_with("gpt-image-") {
            HkuRoute::AzureImage
        } else if model.starts_with("text-embedding-") {
            HkuRoute::AzureEmbedding
        } else if model == "gpt-5.3-codex" {
            HkuRoute::AzureResponses
        } else {
            HkuRoute::AzureChat
        };
        let path = match route {
            HkuRoute::DeepSeek => "/foundry/student/models/chat/completions",
            HkuRoute::VertexChat => "/vertexai/student/chat/completions",
            HkuRoute::Gemini => "/vertexai/student/models",
            HkuRoute::AzureChat
            | HkuRoute::AzureResponses
            | HkuRoute::AzureImage
            | HkuRoute::AzureEmbedding => "/openai/student",
        };
        let style = match route {
            HkuRoute::DeepSeek => ReasoningStyle::DeepSeekThinking,
            HkuRoute::AzureChat | HkuRoute::AzureResponses => ReasoningStyle::Effort,
            _ => ReasoningStyle::None,
        };
        let mut provider = Self::openai_compatible(
            base_url,
            api_key,
            path,
            "/unused",
            "HKU GenAI",
            style,
            false,
        )
        .with_api_key_header("api-key");
        provider.hku_route = Some(route);
        provider.hku_claude_converse = model.starts_with("claude-");
        if provider.hku_claude_converse {
            provider = Self::hku_claude_converse(
                provider.base_url.clone(),
                provider.api_key.clone().unwrap_or_default(),
            );
        }
        provider
    }

    fn authorize(&self, request: RequestBuilder) -> RequestBuilder {
        let request = if let (Some(api_key), Some(header)) =
            (self.api_key.as_deref(), self.api_key_header.as_deref())
        {
            request.header(header, api_key)
        } else if let Some(api_key) = self.api_key.as_deref() {
            request.bearer_auth(api_key)
        } else {
            request
        };
        if self.openrouter_headers {
            request.header("X-OpenRouter-Title", "JadX")
        } else {
            request
        }
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }

    fn redact_secret(&self, body: String) -> String {
        self.api_key
            .as_deref()
            .filter(|key| !key.is_empty())
            .map(|key| body.replace(key, "[redacted]"))
            .unwrap_or(body)
    }

    pub async fn list_compatible_models(&self) -> Result<Vec<ModelInfo>, InferenceError> {
        let response = self
            .authorize(self.client.get(self.endpoint(&self.models_path)))
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|error| InferenceError::Unreachable(error.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = self.redact_secret(response.text().await.unwrap_or_default());
            return Err(InferenceError::Api { status, body });
        }
        let value: Value = response
            .json()
            .await
            .map_err(|error| InferenceError::Parse(error.to_string()))?;
        let list = value
            .get("data")
            .or_else(|| value.get("models"))
            .and_then(Value::as_array)
            .ok_or_else(|| InferenceError::Parse("provider models array missing".to_string()))?;
        Ok(list
            .iter()
            .take(2_000)
            .filter_map(parse_model)
            .map(|mut model| {
                model.publisher = Some(self.provider_name.clone());
                model
            })
            .collect())
    }

    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, InferenceError> {
        let response = self
            .client
            .get(format!("{}/api/v1/models", self.base_url))
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => return Err(InferenceError::Unreachable(error.to_string())),
        };
        if !response.status().is_success() {
            return Err(InferenceError::Api {
                status: response.status(),
                body: response.text().await.unwrap_or_default(),
            });
        }
        let value: Value = response
            .json()
            .await
            .map_err(|e| InferenceError::Parse(e.to_string()))?;
        let list = value
            .get("models")
            .or_else(|| value.get("data"))
            .and_then(Value::as_array)
            .ok_or_else(|| InferenceError::Parse("models array missing".to_string()))?;
        Ok(list.iter().filter_map(parse_model).collect())
    }

    pub async fn load_model(
        &self,
        model: &str,
        context_length: u64,
    ) -> Result<Value, InferenceError> {
        let response = self
            .client
            .post(format!("{}/api/v1/models/load", self.base_url))
            .json(&json!({ "model": model, "context_length": context_length, "echo_load_config": true }))
            .send()
            .await
            .map_err(|e| InferenceError::Transport(e.to_string()))?;
        parse_json_response(response).await
    }

    pub async fn unload_model(&self, model: &str) -> Result<Value, InferenceError> {
        let response = self
            .client
            .post(format!("{}/api/v1/models/unload", self.base_url))
            .json(&json!({ "instance_id": model }))
            .send()
            .await
            .map_err(|e| InferenceError::Transport(e.to_string()))?;
        parse_json_response(response).await
    }

    pub async fn stream_response(
        &self,
        model: &str,
        messages: &[ChatMessage],
        temperature: f64,
        top_p: f64,
        max_output_tokens: u64,
        tools: &[ToolDefinition],
        thinking_mode: &ThinkingMode,
        reasoning_effort: &ReasoningEffort,
        mut on_event: impl FnMut(&str, Option<String>) + Send,
    ) -> Result<StreamResult, InferenceError> {
        if self.hku_route == Some(HkuRoute::AzureEmbedding) {
            return Err(InferenceError::Parse("This HKU deployment produces embeddings, not chat responses. Use the embedding panel in Settings → Providers.".into()));
        }
        if let Some(route @ (HkuRoute::Gemini | HkuRoute::AzureResponses | HkuRoute::AzureImage)) =
            self.hku_route
        {
            let result = self
                .complete_hku_adapter(
                    route,
                    model,
                    messages,
                    temperature,
                    top_p,
                    max_output_tokens,
                    tools,
                    thinking_mode,
                    reasoning_effort,
                )
                .await?;
            if !result.text.is_empty() {
                on_event("delta", Some(result.text.clone()));
            }
            return Ok(result);
        }
        if self.hku_claude_converse {
            if !tools.is_empty() {
                return Err(InferenceError::Parse(
                    "The HKU Claude Converse route is text-only in the published API schema. Switch to an HKU Vertex Qwen/GLM model to use JadX tools.".into(),
                ));
            }
            let result = self
                .complete_hku_claude_converse(
                    model,
                    messages,
                    temperature,
                    top_p,
                    max_output_tokens,
                )
                .await?;
            if !result.text.is_empty() {
                on_event("delta", Some(result.text.clone()));
            }
            return Ok(result);
        }
        // LM Studio documents images for Chat Completions.  Keep Responses as
        // the normal text path, but use the image-capable endpoint whenever a
        // request contains attachments.  This also avoids sending an empty
        // `tools` array through the Responses vision adapter on older LM
        // Studio builds.
        let has_images = messages.iter().any(|message| !message.images.is_empty());
        if self.api_key.is_some() {
            let result = self
                .stream_chat_completions(
                    model,
                    messages,
                    temperature,
                    top_p,
                    max_output_tokens,
                    tools,
                    thinking_mode,
                    reasoning_effort,
                    true,
                    &mut on_event,
                )
                .await;
            return match result {
                Err(InferenceError::Api { body, .. }) if unsupported_reasoning(&body) => {
                    self.stream_chat_completions(
                        model,
                        messages,
                        temperature,
                        top_p,
                        max_output_tokens,
                        tools,
                        &ThinkingMode::Off,
                        reasoning_effort,
                        false,
                        &mut on_event,
                    )
                    .await
                }
                other => other,
            };
        }
        let result = if tools.is_empty() && !has_images {
            self.stream_responses_api(
                model,
                messages,
                temperature,
                top_p,
                max_output_tokens,
                thinking_mode,
                reasoning_effort,
                true,
                &mut on_event,
            )
            .await
        } else {
            self.stream_chat_completions(
                model,
                messages,
                temperature,
                top_p,
                max_output_tokens,
                tools,
                thinking_mode,
                reasoning_effort,
                true,
                &mut on_event,
            )
            .await
        };
        match result {
            Err(InferenceError::Api { body, .. }) if unsupported_reasoning(&body) => {
                if tools.is_empty() && !has_images {
                    self.stream_responses_api(
                        model,
                        messages,
                        temperature,
                        top_p,
                        max_output_tokens,
                        &ThinkingMode::Off,
                        reasoning_effort,
                        false,
                        &mut on_event,
                    )
                    .await
                } else {
                    self.stream_chat_completions(
                        model,
                        messages,
                        temperature,
                        top_p,
                        max_output_tokens,
                        tools,
                        &ThinkingMode::Off,
                        reasoning_effort,
                        false,
                        &mut on_event,
                    )
                    .await
                }
            }
            other => other,
        }
    }

    pub async fn stream_chat_response(
        &self,
        model: &str,
        messages: &[ChatMessage],
        temperature: f64,
        top_p: f64,
        max_output_tokens: u64,
        thinking_mode: &ThinkingMode,
        reasoning_effort: &ReasoningEffort,
        mut on_event: impl FnMut(&str, Option<String>) + Send,
    ) -> Result<StreamResult, InferenceError> {
        if let Some(route @ (HkuRoute::Gemini | HkuRoute::AzureResponses | HkuRoute::AzureImage)) =
            self.hku_route
        {
            let result = self
                .complete_hku_adapter(
                    route,
                    model,
                    messages,
                    temperature,
                    top_p,
                    max_output_tokens,
                    &[],
                    thinking_mode,
                    reasoning_effort,
                )
                .await?;
            if !result.text.is_empty() {
                on_event("delta", Some(result.text.clone()));
            }
            return Ok(result);
        }
        if self.hku_claude_converse {
            let result = self
                .complete_hku_claude_converse(
                    model,
                    messages,
                    temperature,
                    top_p,
                    max_output_tokens,
                )
                .await?;
            if !result.text.is_empty() {
                on_event("delta", Some(result.text.clone()));
            }
            return Ok(result);
        }
        let result = self
            .stream_chat_completions(
                model,
                messages,
                temperature,
                top_p,
                max_output_tokens,
                &[],
                thinking_mode,
                reasoning_effort,
                true,
                &mut on_event,
            )
            .await;
        match result {
            Err(InferenceError::Api { body, .. }) if unsupported_reasoning(&body) => {
                self.stream_chat_completions(
                    model,
                    messages,
                    temperature,
                    top_p,
                    max_output_tokens,
                    &[],
                    &ThinkingMode::Off,
                    reasoning_effort,
                    false,
                    &mut on_event,
                )
                .await
            }
            other => other,
        }
    }

    pub async fn complete_chat_response(
        &self,
        model: &str,
        messages: &[ChatMessage],
        temperature: f64,
        top_p: f64,
        max_output_tokens: u64,
        thinking_mode: &ThinkingMode,
        reasoning_effort: &ReasoningEffort,
    ) -> Result<StreamResult, InferenceError> {
        if let Some(route @ (HkuRoute::Gemini | HkuRoute::AzureResponses | HkuRoute::AzureImage)) =
            self.hku_route
        {
            return self
                .complete_hku_adapter(
                    route,
                    model,
                    messages,
                    temperature,
                    top_p,
                    max_output_tokens,
                    &[],
                    thinking_mode,
                    reasoning_effort,
                )
                .await;
        }
        if self.hku_claude_converse {
            return self
                .complete_hku_claude_converse(
                    model,
                    messages,
                    temperature,
                    top_p,
                    max_output_tokens,
                )
                .await;
        }
        let mut payload = json!({
            "model": model,
            "messages": chat_api_messages(messages),
            "stream": false,
            "temperature": temperature,
            "top_p": top_p,
        });
        if self.openrouter_headers {
            payload["max_completion_tokens"] = json!(max_output_tokens);
        } else {
            payload["max_tokens"] = json!(max_output_tokens);
        }
        set_reasoning_payload(
            &mut payload,
            self.reasoning_style,
            thinking_mode,
            reasoning_effort,
        );
        let mut payload = payload;
        let endpoint = if self.hku_route == Some(HkuRoute::AzureChat) {
            payload.as_object_mut().map(|object| object.remove("model"));
            self.hku_azure_endpoint(model, &["chat", "completions"])?
        } else {
            url::Url::parse(&self.endpoint(&self.chat_completions_path))
                .map_err(|error| InferenceError::Parse(error.to_string()))?
        };
        let response = self
            .authorize(self.client.post(endpoint))
            .json(&payload)
            .send()
            .await
            .map_err(|e| InferenceError::Transport(e.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = self.redact_secret(response.text().await.unwrap_or_default());
            return Err(InferenceError::Api { status, body });
        }
        let value: Value = response
            .json()
            .await
            .map_err(|e| InferenceError::Parse(e.to_string()))?;
        let message = value
            .pointer("/choices/0/message")
            .ok_or_else(|| InferenceError::Parse("chat completion message missing".into()))?;
        Ok(StreamResult {
            text: message
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            reasoning: message
                .get("reasoning_content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            usage: value.get("usage").map(parse_usage).unwrap_or_default(),
            transport: "chat_completions".into(),
            ..Default::default()
        })
    }

    async fn complete_hku_claude_converse(
        &self,
        model: &str,
        messages: &[ChatMessage],
        temperature: f64,
        _top_p: f64,
        max_output_tokens: u64,
    ) -> Result<StreamResult, InferenceError> {
        if messages.iter().any(|message| !message.images.is_empty()) {
            return Err(InferenceError::Parse(
                "The published HKU Claude schema currently supports text content only. Disable image attachments or choose a vision-capable provider.".into(),
            ));
        }
        let deployment = model.trim();
        if !matches!(
            deployment,
            "claude-haiku-4.5" | "claude-sonnet-4.6" | "claude-sonnet-5" | "claude-opus-4.6"
        ) {
            return Err(InferenceError::Parse(
                "Choose a Claude deployment listed by HKU GenAI in Settings → Providers.".into(),
            ));
        }
        let base = url::Url::parse(&self.base_url)
            .map_err(|error| InferenceError::Parse(error.to_string()))?;
        let mut endpoint = base.clone();
        {
            let mut segments = endpoint.path_segments_mut().map_err(|_| {
                InferenceError::Parse("HKU provider URL cannot accept a model path.".into())
            })?;
            segments.pop_if_empty();
            segments.extend(["claude", "student", "model", deployment, "converse"]);
        }

        let mut system = Vec::new();
        let mut api_messages = Vec::new();
        for message in messages {
            if message.role == "system" {
                if !message.content.trim().is_empty() {
                    system.push(message.content.trim());
                }
                continue;
            }
            let (role, content) = if message.role == "assistant" {
                ("assistant", message.content.clone())
            } else if message.role == "tool" {
                (
                    "user",
                    format!(
                        "Tool result{}:\n{}",
                        message
                            .name
                            .as_deref()
                            .map(|name| format!(" from {name}"))
                            .unwrap_or_default(),
                        message.content
                    ),
                )
            } else {
                ("user", message.content.clone())
            };
            api_messages.push(json!({
                "role": role,
                "content": [{ "type": "text", "text": content }]
            }));
        }
        if api_messages.is_empty() {
            return Err(InferenceError::Parse(
                "HKU Claude needs at least one user or assistant text message.".into(),
            ));
        }
        system.push(
            "This HKU Claude endpoint is configured for text-only requests without JadX tools. Never claim you opened, inspected, or changed files or websites; ask the user to choose an HKU Vertex Qwen/GLM model when a tool action is required.",
        );
        // HKU's published request schema lists message content and inference
        // parameters but not a top-level system field; fold the system prompt
        // into the first user text block to stay within that gateway schema.
        if let Some(first_user) = api_messages
            .iter_mut()
            .find(|entry| entry["role"] == "user")
        {
            let text = first_user["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            first_user["content"][0]["text"] =
                json!(format!("{}\n\n{}", system.join("\n\n"), text));
        } else {
            api_messages.insert(
                0,
                json!({
                    "role": "user",
                    "content": [{ "type": "text", "text": system.join("\n\n") }]
                }),
            );
        }
        let payload = json!({
            "anthropic_version": "bedrock-2023-05-31",
            "max_tokens": max_output_tokens.clamp(1, 32_768),
            "temperature": temperature.clamp(0.0, 1.0),
            "messages": api_messages,
        });
        let started = Instant::now();
        let response = self
            .authorize(self.client.post(endpoint))
            .timeout(Duration::from_secs(600))
            .json(&payload)
            .send()
            .await
            .map_err(|error| InferenceError::Transport(error.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = self.redact_secret(response.text().await.unwrap_or_default());
            return Err(InferenceError::Api { status, body });
        }
        let value: Value = response
            .json()
            .await
            .map_err(|error| InferenceError::Parse(error.to_string()))?;
        let text = value
            .pointer("/output/message/content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("");
        if text.is_empty() {
            return Err(InferenceError::Parse(
                "HKU Claude returned no text in output.message.content.".into(),
            ));
        }
        let mut usage = value.get("usage").map(parse_usage).unwrap_or_default();
        usage.total_ms = Some(started.elapsed().as_millis().min(u64::MAX as u128) as u64);
        Ok(StreamResult {
            text,
            usage,
            transport: "hku_bedrock_converse".into(),
            ..Default::default()
        })
    }

    async fn complete_hku_adapter(
        &self,
        route: HkuRoute,
        model: &str,
        messages: &[ChatMessage],
        temperature: f64,
        top_p: f64,
        max_output_tokens: u64,
        tools: &[ToolDefinition],
        thinking_mode: &ThinkingMode,
        reasoning_effort: &ReasoningEffort,
    ) -> Result<StreamResult, InferenceError> {
        match route {
            HkuRoute::AzureImage => self.complete_hku_image(model, messages).await,
            HkuRoute::AzureEmbedding => Err(InferenceError::Parse(
                "Embedding deployments do not generate chat responses.".into(),
            )),
            HkuRoute::Gemini => {
                self.complete_hku_gemini(
                    model,
                    messages,
                    temperature,
                    top_p,
                    max_output_tokens,
                    tools,
                    thinking_mode,
                    reasoning_effort,
                )
                .await
            }
            HkuRoute::AzureResponses => {
                self.complete_hku_azure_responses(
                    model,
                    messages,
                    max_output_tokens,
                    tools,
                    thinking_mode,
                    reasoning_effort,
                )
                .await
            }
            _ => Err(InferenceError::Parse(
                "Unsupported non-streaming HKU route.".into(),
            )),
        }
    }

    fn hku_azure_endpoint(&self, model: &str, tail: &[&str]) -> Result<url::Url, InferenceError> {
        let mut endpoint = url::Url::parse(&self.base_url)
            .map_err(|error| InferenceError::Parse(error.to_string()))?;
        {
            let mut segments = endpoint.path_segments_mut().map_err(|_| {
                InferenceError::Parse("HKU endpoint cannot accept a deployment path.".into())
            })?;
            segments.pop_if_empty();
            segments.extend(["openai", "student", "deployments", model]);
            segments.extend(tail.iter().copied());
        }
        endpoint
            .query_pairs_mut()
            .append_pair("api-version", "2025-04-01-preview");
        Ok(endpoint)
    }

    pub async fn create_hku_embedding(
        &self,
        model: &str,
        input: &str,
    ) -> Result<HkuEmbeddingResult, InferenceError> {
        if self.hku_route != Some(HkuRoute::AzureEmbedding) || !model.starts_with("text-embedding-")
        {
            return Err(InferenceError::Parse(
                "Choose an HKU Azure OpenAI embedding deployment.".into(),
            ));
        }
        if input.trim().is_empty() || input.len() > 32_000 {
            return Err(InferenceError::Parse(
                "Embedding input must contain 1–32,000 bytes of text.".into(),
            ));
        }
        let endpoint = self.hku_azure_endpoint(model, &["embeddings"])?;
        let response = self
            .authorize(self.client.post(endpoint))
            .json(&json!({ "input": input }))
            .send()
            .await
            .map_err(|error| InferenceError::Transport(error.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = self.redact_secret(response.text().await.unwrap_or_default());
            return Err(InferenceError::Api { status, body });
        }
        let value: Value = response
            .json()
            .await
            .map_err(|error| InferenceError::Parse(error.to_string()))?;
        let embedding = value
            .pointer("/data/0/embedding")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                InferenceError::Parse("HKU embedding response omitted data[0].embedding.".into())
            })?
            .iter()
            .map(|value| {
                value.as_f64().map(|number| number as f32).ok_or_else(|| {
                    InferenceError::Parse("HKU returned a non-numeric embedding element.".into())
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if embedding.is_empty() || embedding.len() > 32_768 {
            return Err(InferenceError::Parse(
                "HKU returned an empty or oversized embedding vector.".into(),
            ));
        }
        Ok(HkuEmbeddingResult {
            model: model.into(),
            embedding,
            usage: value.get("usage").map(parse_usage).unwrap_or_default(),
            transport: "hku_azure_embeddings".into(),
        })
    }

    async fn complete_hku_image(
        &self,
        model: &str,
        messages: &[ChatMessage],
    ) -> Result<StreamResult, InferenceError> {
        let prompt = messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.content.trim())
            .filter(|prompt| !prompt.is_empty())
            .ok_or_else(|| {
                InferenceError::Parse("Enter a text prompt to generate an image.".into())
            })?;
        let mut endpoint = url::Url::parse(&self.base_url)
            .map_err(|error| InferenceError::Parse(error.to_string()))?;
        {
            let mut segments = endpoint.path_segments_mut().map_err(|_| {
                InferenceError::Parse("HKU image endpoint cannot accept a deployment path.".into())
            })?;
            segments.pop_if_empty();
            segments.extend(["openai", "student", model, "images", "generations"]);
        }
        endpoint
            .query_pairs_mut()
            .append_pair("api-version", "2025-04-01-preview");
        let response = self
            .authorize(self.client.post(endpoint))
            .json(&json!({ "prompt": prompt, "size": "1024x1024", "quality": "medium" }))
            .send()
            .await
            .map_err(|error| InferenceError::Transport(error.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = self.redact_secret(response.text().await.unwrap_or_default());
            return Err(InferenceError::Api { status, body });
        }
        let value: Value = response
            .json()
            .await
            .map_err(|error| InferenceError::Parse(error.to_string()))?;
        let first = value.pointer("/data/0").ok_or_else(|| {
            InferenceError::Parse("HKU image response did not contain an image.".into())
        })?;
        let encoded = first
            .get("b64_json")
            .and_then(Value::as_str)
            .ok_or_else(|| InferenceError::Parse("HKU image response omitted b64_json.".into()))?;
        if encoded.len() > 12 * 1024 * 1024 {
            return Err(InferenceError::Parse(
                "Generated image exceeded JadX's 9 MB decoded limit.".into(),
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| {
                InferenceError::Parse(format!("Invalid generated image data: {error}"))
            })?;
        let format = value
            .get("output_format")
            .and_then(Value::as_str)
            .unwrap_or("png");
        let mime_type = match format {
            "jpeg" | "jpg" => "image/jpeg",
            "webp" => "image/webp",
            _ => "image/png",
        };
        let image = ImageInput {
            name: format!(
                "generated-image.{}",
                match format {
                    "jpeg" => "jpg",
                    other => other,
                }
            ),
            mime_type: mime_type.into(),
            data_url: format!(
                "data:{mime_type};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&bytes)
            ),
            size_bytes: bytes.len() as u64,
        };
        Ok(StreamResult {
            text: "Generated image.".into(),
            images: vec![image],
            usage: value.get("usage").map(parse_usage).unwrap_or_default(),
            transport: "hku_azure_image_generation".into(),
            ..Default::default()
        })
    }

    async fn complete_hku_gemini(
        &self,
        model: &str,
        messages: &[ChatMessage],
        temperature: f64,
        top_p: f64,
        max_output_tokens: u64,
        tools: &[ToolDefinition],
        thinking_mode: &ThinkingMode,
        reasoning_effort: &ReasoningEffort,
    ) -> Result<StreamResult, InferenceError> {
        let mut endpoint = url::Url::parse(&self.base_url)
            .map_err(|error| InferenceError::Parse(error.to_string()))?;
        {
            let mut segments = endpoint.path_segments_mut().map_err(|_| {
                InferenceError::Parse("HKU Gemini endpoint cannot accept a model path.".into())
            })?;
            segments.pop_if_empty();
            segments.extend(["vertexai", "student", "models"]);
            segments.push(&format!("{model}:generateContent"));
        }
        let mut system = Vec::new();
        let mut contents = Vec::new();
        for message in messages {
            if message.role == "system" {
                if !message.content.trim().is_empty() {
                    system.push(message.content.trim());
                }
                continue;
            }
            if message.role == "tool" {
                contents.push(json!({
                    "role": "user",
                    "parts": [{ "text": format!("[Prior JadX tool result from {}]: {}", message.name.as_deref().unwrap_or("tool"), message.content) }]
                }));
                continue;
            }
            let mut parts = Vec::new();
            if !message.content.is_empty() {
                parts.push(json!({ "text": message.content }));
            }
            for image in &message.images {
                let (mime, bytes) = split_data_url(&image.data_url, &image.mime_type)?;
                parts.push(json!({ "inlineData": { "mimeType": mime, "data": bytes } }));
            }
            // Historic JadX function calls cannot be replayed as Gemini functionCall
            // parts when the HKU route has no matching function declarations.
            if !parts.is_empty() {
                contents.push(json!({ "role": if message.role == "assistant" { "model" } else { "user" }, "parts": parts }));
            }
        }
        if contents.is_empty() {
            return Err(InferenceError::Parse(
                "HKU Gemini needs at least one user message.".into(),
            ));
        }
        let image_model = model.contains("image");
        let mut generation_config = json!({
            "temperature": temperature.clamp(0.0, 2.0),
            "topP": top_p.clamp(0.0, 1.0),
            "maxOutputTokens": max_output_tokens.clamp(1, 65_536),
        });
        set_gemini_thinking_config(
            &mut generation_config,
            model,
            thinking_mode,
            reasoning_effort,
        );
        if image_model {
            generation_config["responseModalities"] = json!(["TEXT", "IMAGE"]);
        }
        let mut payload = json!({ "contents": contents, "generationConfig": generation_config });
        if !system.is_empty() {
            payload["systemInstruction"] = json!({ "parts": [{ "text": system.join("\n\n") }] });
        }
        // HKU's ITS Gemini offering currently rejects JadX function declarations;
        // only its own url_context tool is available, which JadX deliberately
        // does not enable implicitly because that would bypass network approval.
        let response = self
            .authorize(self.client.post(endpoint))
            .json(&payload)
            .send()
            .await
            .map_err(|error| InferenceError::Transport(error.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = self.redact_secret(response.text().await.unwrap_or_default());
            return Err(InferenceError::Api { status, body });
        }
        let value: Value = response
            .json()
            .await
            .map_err(|error| InferenceError::Parse(error.to_string()))?;
        let mut result = StreamResult {
            transport: "hku_vertex_gemini".into(),
            warning: (!tools.is_empty()).then(|| "HKU Gemini's ITS offering does not accept JadX function tools. JadX omitted them; choose a tool-enabled HKU model for workspace, browser, shell, or other harness actions.".into()),
            ..Default::default()
        };
        if let Some(parts) = value
            .pointer("/candidates/0/content/parts")
            .and_then(Value::as_array)
        {
            for part in parts {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    if part
                        .get("thought")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        result.reasoning.push_str(text);
                    } else {
                        result.text.push_str(text);
                    }
                }
                if let Some(inline) = part.get("inlineData").or_else(|| part.get("inline_data")) {
                    if let (Some(encoded), Some(mime)) = (
                        inline.get("data").and_then(Value::as_str),
                        inline
                            .get("mimeType")
                            .or_else(|| inline.get("mime_type"))
                            .and_then(Value::as_str),
                    ) {
                        let bytes = base64::engine::general_purpose::STANDARD
                            .decode(encoded)
                            .map_err(|error| {
                                InferenceError::Parse(format!("Invalid Gemini image data: {error}"))
                            })?;
                        if bytes.len() > 9 * 1024 * 1024 {
                            return Err(InferenceError::Parse(
                                "Gemini image exceeded JadX's 9 MB decoded limit.".into(),
                            ));
                        }
                        result.images.push(ImageInput {
                            name: format!(
                                "generated-image-{}.{}",
                                result.images.len() + 1,
                                image_extension(mime)
                            ),
                            mime_type: mime.into(),
                            data_url: format!(
                                "data:{mime};base64,{}",
                                base64::engine::general_purpose::STANDARD.encode(&bytes)
                            ),
                            size_bytes: bytes.len() as u64,
                        });
                    }
                }
                if let Some(call) = part
                    .get("functionCall")
                    .or_else(|| part.get("function_call"))
                {
                    if let Some(name) = call.get("name").and_then(Value::as_str) {
                        result.tool_calls.push(ToolCall {
                            id: Uuid::new_v4().to_string(),
                            name: name.into(),
                            arguments: call
                                .get("args")
                                .or_else(|| call.get("arguments"))
                                .cloned()
                                .unwrap_or_else(|| json!({})),
                        });
                    }
                }
            }
        }
        result.usage = value
            .get("usageMetadata")
            .or_else(|| value.get("usage_metadata"))
            .map(parse_usage)
            .unwrap_or_default();
        if result.text.is_empty() && result.images.is_empty() && result.tool_calls.is_empty() {
            return Err(InferenceError::Parse(
                "HKU Gemini returned no text, image, or function call.".into(),
            ));
        }
        if result.text.is_empty() && !result.images.is_empty() {
            result.text = "Generated image.".into();
        }
        Ok(result)
    }

    async fn complete_hku_azure_responses(
        &self,
        model: &str,
        messages: &[ChatMessage],
        max_output_tokens: u64,
        tools: &[ToolDefinition],
        thinking_mode: &ThinkingMode,
        reasoning_effort: &ReasoningEffort,
    ) -> Result<StreamResult, InferenceError> {
        let endpoint = self.hku_azure_endpoint(model, &["responses"])?;
        let mut input = Vec::new();
        for message in messages {
            if message.role == "tool" {
                input.push(json!({ "type": "function_call_output", "call_id": message.tool_call_id, "output": message.content }));
                continue;
            }
            if message.role == "assistant" {
                if !message.content.is_empty() {
                    input.push(json!({ "role": "assistant", "content": message.content }));
                }
                if let Some(calls) = &message.tool_calls {
                    for call in calls {
                        input.push(json!({ "type": "function_call", "call_id": call.id, "name": call.name, "arguments": call.arguments.to_string() }));
                    }
                }
                continue;
            }
            input.push(json!({ "role": message.role, "content": responses_content(message) }));
        }
        let input = repair_responses_function_sequence(input);
        let mut payload =
            json!({ "input": input, "max_output_tokens": max_output_tokens.clamp(1, 65_536) });
        if matches!(thinking_mode, ThinkingMode::On) {
            payload["reasoning"] =
                json!({ "effort": reasoning_request_value(thinking_mode, reasoning_effort) });
        }
        if !tools.is_empty() {
            payload["tools"] = json!(tools.iter().map(|tool| json!({ "type": "function", "name": tool.name, "description": tool.description, "parameters": tool.parameters, "strict": false })).collect::<Vec<_>>());
            payload["tool_choice"] = json!("auto");
        }
        let response = self
            .authorize(self.client.post(endpoint))
            .json(&payload)
            .send()
            .await
            .map_err(|error| InferenceError::Transport(error.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = self.redact_secret(response.text().await.unwrap_or_default());
            return Err(InferenceError::Api { status, body });
        }
        let value: Value = response
            .json()
            .await
            .map_err(|error| InferenceError::Parse(error.to_string()))?;
        let mut result = StreamResult {
            transport: "hku_azure_responses".into(),
            ..Default::default()
        };
        result.text = value
            .get("output_text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if let Some(output) = value.get("output").and_then(Value::as_array) {
            for item in output {
                if item.get("type").and_then(Value::as_str) == Some("function_call") {
                    let args = item.get("arguments").cloned().unwrap_or_else(|| json!({}));
                    result.tool_calls.push(ToolCall {
                        id: item
                            .get("call_id")
                            .or_else(|| item.get("id"))
                            .and_then(Value::as_str)
                            .unwrap_or("hku-response-call")
                            .into(),
                        name: item
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .into(),
                        arguments: normalize_tool_arguments(&args),
                    });
                }
                if let Some(parts) = item.get("content").and_then(Value::as_array) {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            result.text.push_str(text);
                        }
                    }
                }
            }
        }
        result.usage = value.get("usage").map(parse_usage).unwrap_or_default();
        if result.text.is_empty() && result.tool_calls.is_empty() {
            return Err(InferenceError::Parse(
                "HKU Responses API returned no output.".into(),
            ));
        }
        Ok(result)
    }

    async fn stream_responses_api(
        &self,
        model: &str,
        messages: &[ChatMessage],
        temperature: f64,
        top_p: f64,
        max_output_tokens: u64,
        thinking_mode: &ThinkingMode,
        reasoning_effort: &ReasoningEffort,
        include_reasoning: bool,
        on_event: &mut (impl FnMut(&str, Option<String>) + Send),
    ) -> Result<StreamResult, InferenceError> {
        let input = messages
            .iter()
            .map(|message| {
                json!({
                    "role": message.role,
                    "content": responses_content(message)
                })
            })
            .collect::<Vec<_>>();
        let mut payload = json!({
            "model": model,
            "input": input,
            "stream": true,
            "temperature": temperature,
            "top_p": top_p,
            "max_output_tokens": max_output_tokens
        });
        if include_reasoning {
            payload["reasoning"] = json!({
                "effort": reasoning_request_value(thinking_mode, reasoning_effort)
            });
        }
        let response = self
            .client
            .post(format!("{}/v1/responses", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| InferenceError::Transport(e.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(InferenceError::Api { status, body });
        }
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut result = StreamResult {
            transport: "responses".to_string(),
            ..Default::default()
        };
        let mut tool_arguments: BTreeMap<String, String> = BTreeMap::new();
        let deadline = Instant::now() + STREAM_DEADLINE;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(InferenceError::Transport(
                    "Inference-provider response exceeded the 300-second generation limit.".into(),
                ));
            }
            let chunk = tokio::time::timeout(remaining.min(STREAM_IDLE_TIMEOUT), stream.next())
                .await
                .map_err(|_| InferenceError::IdleTimeout {
                    idle_seconds: STREAM_IDLE_TIMEOUT.as_secs(),
                })?;
            let Some(chunk) = chunk else { break };
            let bytes = chunk.map_err(|e| InferenceError::Transport(e.to_string()))?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(index) = buffer.find('\n') {
                let line = buffer[..index].trim_end_matches('\r').to_string();
                buffer.drain(..=index);
                if let Some(payload) = line.strip_prefix("data:") {
                    let payload = payload.trim();
                    if payload.is_empty() || payload == "[DONE]" {
                        continue;
                    }
                    let event: Value = match serde_json::from_str(payload) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    let kind = event
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if kind.contains("reasoning")
                        && (kind.contains("delta") || kind.contains("summary"))
                    {
                        if let Some(delta) = event
                            .get("delta")
                            .or_else(|| event.get("content"))
                            .and_then(Value::as_str)
                        {
                            result.reasoning.push_str(delta);
                            on_event("reasoning", Some(delta.to_string()));
                        }
                    } else if kind.contains("output_text.delta") {
                        if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                            result.text.push_str(delta);
                            on_event("delta", Some(delta.to_string()));
                        }
                    } else if kind.contains("function_call_arguments.delta") {
                        on_event("tool-progress", None);
                        let id = event
                            .get("item_id")
                            .or_else(|| event.get("output_index"))
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "response-tool".to_string());
                        tool_arguments.entry(id).or_default().push_str(
                            event
                                .get("delta")
                                .and_then(Value::as_str)
                                .unwrap_or_default(),
                        );
                    } else if kind.contains("function_call_arguments.done") {
                        on_event("tool-progress", None);
                        let id = event
                            .get("item_id")
                            .or_else(|| event.get("output_index"))
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "response-tool".to_string());
                        if let Some(arguments) = event.get("arguments") {
                            if let Some(arguments) = arguments.as_str() {
                                tool_arguments.insert(id, arguments.to_string());
                            } else {
                                tool_arguments.insert(id, arguments.to_string());
                            }
                        }
                    } else if kind.contains("output_item.done") {
                        if let Some(item) = event.get("item") {
                            if item.get("type").and_then(Value::as_str) == Some("function_call") {
                                let id = item
                                    .get("call_id")
                                    .or_else(|| item.get("id"))
                                    .and_then(Value::as_str)
                                    .unwrap_or("response-tool")
                                    .to_string();
                                let name = item
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string();
                                let raw = item
                                    .get("arguments")
                                    .cloned()
                                    .or_else(|| tool_arguments.get(&id).cloned().map(Value::String))
                                    .unwrap_or_else(|| json!({}));
                                result.tool_calls.push(ToolCall {
                                    id,
                                    name,
                                    arguments: normalize_tool_arguments(&raw),
                                });
                            }
                        }
                    } else if kind == "response.completed" {
                        if let Some(usage) = event.pointer("/response/usage") {
                            result.usage = parse_usage(usage);
                        }
                    }
                }
            }
        }
        Ok(result)
    }

    fn stream_idle_timeout(&self) -> Duration {
        let is_loopback = url::Url::parse(&self.base_url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .is_some_and(|host| {
                host.eq_ignore_ascii_case("localhost")
                    || host.ends_with(".localhost")
                    || host
                        .parse::<IpAddr>()
                        .is_ok_and(|address| address.is_loopback())
            });
        if is_loopback {
            STREAM_IDLE_TIMEOUT
        } else {
            CLOUD_STREAM_IDLE_TIMEOUT
        }
    }

    async fn stream_chat_completions(
        &self,
        model: &str,
        messages: &[ChatMessage],
        temperature: f64,
        top_p: f64,
        max_output_tokens: u64,
        tools: &[ToolDefinition],
        thinking_mode: &ThinkingMode,
        reasoning_effort: &ReasoningEffort,
        include_reasoning: bool,
        on_event: &mut (impl FnMut(&str, Option<String>) + Send),
    ) -> Result<StreamResult, InferenceError> {
        let stream_idle_timeout = self.stream_idle_timeout();
        // HKU's Vertex open-model endpoint follows Google's OpenAPI-schema
        // subset rather than accepting every JSON Schema keyword emitted by
        // JadX's strict local validators. Strip unsupported schema keywords
        // at the provider boundary; validation in tools.rs remains strict.
        let preserve_reasoning = self.reasoning_style == ReasoningStyle::DeepSeekThinking;
        let api_messages = if self.hku_route == Some(HkuRoute::DeepSeek) && !tools.is_empty() {
            deepseek_tool_messages(messages, preserve_reasoning)
        } else {
            chat_api_messages_with_reasoning(messages, preserve_reasoning)
        };
        let api_tools = tools
            .iter()
            .map(|tool| {
                let parameters = if self.hku_route == Some(HkuRoute::VertexChat) {
                    vertex_openapi_tool_schema(&tool.parameters)
                } else {
                    tool.parameters.clone()
                };
                json!({
                    "type": "function",
                    "function": { "name": tool.name, "description": tool.description, "parameters": parameters }
                })
            })
            .collect::<Vec<_>>();
        let mut payload = json!({
            "model": model,
            "messages": api_messages,
            "stream": true,
            "stream_options": { "include_usage": true },
            "temperature": temperature,
            "top_p": top_p
        });
        if self.openrouter_headers {
            payload["max_completion_tokens"] = json!(max_output_tokens);
        } else {
            payload["max_tokens"] = json!(max_output_tokens);
        }
        if !api_tools.is_empty() {
            payload["tools"] = json!(api_tools);
            payload["tool_choice"] = json!("auto");
        }
        if include_reasoning {
            set_reasoning_payload(
                &mut payload,
                self.reasoning_style,
                thinking_mode,
                reasoning_effort,
            );
        }
        if self.hku_route == Some(HkuRoute::AzureChat) {
            payload.as_object_mut().map(|object| object.remove("model"));
        }
        let endpoint = if self.hku_route == Some(HkuRoute::AzureChat) {
            self.hku_azure_endpoint(model, &["chat", "completions"])?
        } else {
            url::Url::parse(&self.endpoint(&self.chat_completions_path))
                .map_err(|error| InferenceError::Parse(error.to_string()))?
        };
        let response = self
            .authorize(self.client.post(endpoint))
            .json(&payload)
            .send()
            .await
            .map_err(|e| InferenceError::Transport(e.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = self.redact_secret(response.text().await.unwrap_or_default());
            return Err(InferenceError::Api { status, body });
        }
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut result = StreamResult {
            transport: "chat_completions".to_string(),
            ..Default::default()
        };
        let mut calls: BTreeMap<usize, ToolCall> = BTreeMap::new();
        let deadline = Instant::now() + STREAM_DEADLINE;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(InferenceError::Transport(
                    "Inference-provider response exceeded the 300-second generation limit.".into(),
                ));
            }
            let chunk = tokio::time::timeout(remaining.min(stream_idle_timeout), stream.next())
                .await
                .map_err(|_| InferenceError::IdleTimeout {
                    idle_seconds: stream_idle_timeout.as_secs(),
                })?;
            let Some(chunk) = chunk else { break };
            let bytes = chunk.map_err(|e| InferenceError::Transport(e.to_string()))?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(index) = buffer.find('\n') {
                let line = buffer[..index].trim_end_matches('\r').to_string();
                buffer.drain(..=index);
                if let Some(payload) = line.strip_prefix("data:") {
                    let payload = payload.trim();
                    if payload.is_empty() || payload == "[DONE]" {
                        continue;
                    }
                    let event: Value = match serde_json::from_str(payload) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    if let Some(usage) = event.get("usage") {
                        result.usage = parse_usage(usage);
                    }
                    let choice = event.pointer("/choices/0").cloned().unwrap_or_default();
                    if let Some(delta) = choice
                        .pointer("/delta/reasoning")
                        .or_else(|| choice.pointer("/delta/reasoning_content"))
                        .and_then(Value::as_str)
                    {
                        result.reasoning.push_str(delta);
                        on_event("reasoning", Some(delta.to_string()));
                    }
                    if let Some(delta) = choice
                        .get("delta")
                        .and_then(|value| value.get("content"))
                        .and_then(Value::as_str)
                    {
                        result.text.push_str(delta);
                        on_event("delta", Some(delta.to_string()));
                    }
                    if let Some(tool_deltas) = choice
                        .pointer("/delta/tool_calls")
                        .and_then(Value::as_array)
                    {
                        on_event("tool-progress", None);
                        for tool_delta in tool_deltas {
                            let index = tool_delta.get("index").and_then(Value::as_u64).unwrap_or(0)
                                as usize;
                            let entry = calls.entry(index).or_insert_with(|| ToolCall {
                                id: String::new(),
                                name: String::new(),
                                arguments: Value::String(String::new()),
                            });
                            if let Some(id) = tool_delta.get("id").and_then(Value::as_str) {
                                entry.id.push_str(id);
                            }
                            if let Some(name) =
                                tool_delta.pointer("/function/name").and_then(Value::as_str)
                            {
                                entry.name.push_str(name);
                            }
                            if let Some(arguments) = tool_delta.pointer("/function/arguments") {
                                if let Some(arguments) = arguments.as_str() {
                                    let current =
                                        entry.arguments.as_str().unwrap_or_default().to_string();
                                    entry.arguments =
                                        Value::String(format!("{}{}", current, arguments));
                                } else {
                                    entry.arguments = arguments.clone();
                                }
                            }
                        }
                    }
                    if let Some(tool_calls) = choice
                        .pointer("/message/tool_calls")
                        .and_then(Value::as_array)
                    {
                        for (index, tool_call) in tool_calls.iter().enumerate() {
                            let entry = calls.entry(index).or_insert_with(|| ToolCall {
                                id: String::new(),
                                name: String::new(),
                                arguments: json!({}),
                            });
                            if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
                                entry.id = id.to_string();
                            }
                            if let Some(name) =
                                tool_call.pointer("/function/name").and_then(Value::as_str)
                            {
                                entry.name = name.to_string();
                            }
                            if let Some(arguments) = tool_call.pointer("/function/arguments") {
                                entry.arguments = arguments.clone();
                            }
                        }
                    }
                }
            }
        }
        result.tool_calls = calls
            .into_values()
            .map(|mut call| {
                call.arguments = normalize_tool_arguments(&call.arguments);
                call
            })
            .collect();
        Ok(result)
    }
}

fn responses_content(message: &ChatMessage) -> Value {
    if message.images.is_empty() {
        return Value::String(message.content.clone());
    }
    let mut parts = vec![json!({
        "type": "input_text",
        "text": message.content
    })];
    parts.extend(message.images.iter().map(|image| {
        json!({
            "type": "input_image",
            "image_url": image.data_url
        })
    }));
    Value::Array(parts)
}

fn chat_content(message: &ChatMessage) -> Value {
    if message.images.is_empty() {
        return Value::String(message.content.clone());
    }
    let mut parts = vec![json!({
        "type": "text",
        "text": message.content
    })];
    parts.extend(message.images.iter().map(|image| {
        json!({
            "type": "image_url",
            "image_url": { "url": image.data_url }
        })
    }));
    Value::Array(parts)
}

/// Providers with strict OpenAI validation (DeepSeek, OpenRouter, OpenAI)
/// reject a history where a tool result has no preceding assistant tool_calls
/// message, or assistant tool calls never receive a response. Compaction and
/// handoff paths can produce both, so repair the sequence at the transport
/// boundary: drop orphaned tool results and answer unanswered calls.
fn repair_tool_message_sequence(api_messages: Vec<Value>) -> Vec<Value> {
    let unrecorded = |call_id: String| {
        json!({
            "role": "tool",
            "tool_call_id": call_id,
            "content": "(no tool result was recorded for this call)",
        })
    };
    let mut repaired = Vec::with_capacity(api_messages.len() + 2);
    let mut pending_calls: Vec<String> = Vec::new();
    for message in api_messages {
        let role = message["role"].as_str().unwrap_or_default().to_string();
        if role == "tool" {
            let call_id = message["tool_call_id"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            if let Some(position) = pending_calls.iter().position(|id| *id == call_id) {
                pending_calls.remove(position);
                repaired.push(message);
            }
            continue;
        }
        repaired.extend(pending_calls.drain(..).map(unrecorded));
        if role == "assistant" {
            pending_calls = message["tool_calls"]
                .as_array()
                .map(|calls| {
                    calls
                        .iter()
                        .filter_map(|call| call["id"].as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
        }
        repaired.push(message);
    }
    repaired.extend(pending_calls.drain(..).map(unrecorded));
    repaired
}

/// Same repair for the Responses API shape, where calls and outputs are
/// separate input items keyed by `call_id`.
fn repair_responses_function_sequence(input: Vec<Value>) -> Vec<Value> {
    let unrecorded = |call_id: String| {
        json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": "(no tool result was recorded for this call)",
        })
    };
    let mut repaired = Vec::with_capacity(input.len() + 2);
    let mut pending_calls: Vec<String> = Vec::new();
    for item in input {
        let kind = item["type"].as_str().unwrap_or_default().to_string();
        if kind == "function_call_output" {
            let call_id = item["call_id"].as_str().unwrap_or_default().to_string();
            if let Some(position) = pending_calls.iter().position(|id| *id == call_id) {
                pending_calls.remove(position);
                repaired.push(item);
            }
            continue;
        }
        repaired.extend(pending_calls.drain(..).map(unrecorded));
        if kind == "function_call" {
            if let Some(call_id) = item["call_id"].as_str().map(str::to_string) {
                pending_calls.push(call_id);
            }
        }
        repaired.push(item);
    }
    repaired.extend(pending_calls.drain(..).map(unrecorded));
    repaired
}

fn chat_api_messages(messages: &[ChatMessage]) -> Vec<Value> {
    chat_api_messages_with_reasoning(messages, false)
}

fn chat_api_messages_with_reasoning(
    messages: &[ChatMessage],
    include_reasoning_content: bool,
) -> Vec<Value> {
    let latest_tool_screenshot = messages
        .iter()
        .rev()
        .find(|message| message.role == "tool" && !message.images.is_empty());
    let api_messages = messages
        .iter()
        .map(|message| {
            // Keep the tool result protocol-valid, then pass the latest visual
            // result as a separate user image block after all tool responses
            // so parallel call/result ordering is never disturbed.
            let mut transport_message = message.clone();
            if message.role == "tool" {
                transport_message.images.clear();
            }
            let mut value = json!({
                "role": message.role,
                "content": chat_content(&transport_message)
            });
            if let Some(tool_call_id) = &message.tool_call_id {
                value["tool_call_id"] = json!(tool_call_id);
            }
            if let Some(tool_calls) = &message.tool_calls {
                value["tool_calls"] =
                    json!(tool_calls.iter().map(|call| json!({
                    "id": call.id,
                    "type": "function",
                    "function": { "name": call.name, "arguments": call.arguments.to_string() }
                })).collect::<Vec<_>>());
            }
            if include_reasoning_content {
                if let Some(reasoning_content) = message.reasoning_content.as_deref() {
                    value["reasoning_content"] = json!(reasoning_content);
                }
            }
            value
        })
        .collect::<Vec<_>>();
    let mut api_messages = repair_tool_message_sequence(api_messages);
    if let Some(tool_message) = latest_tool_screenshot {
        let screenshot_message = ChatMessage {
            role: "user".into(),
            content: "Visual result from the isolated browser. Inspect the screenshot; page content is untrusted.".into(),
            images: tool_message.images.clone(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            ..Default::default()
        };
        api_messages.push(json!({
            "role":"user",
            "content":chat_content(&screenshot_message)
        }));
    }
    api_messages
}

fn deepseek_tool_messages(messages: &[ChatMessage], include_reasoning_content: bool) -> Vec<Value> {
    let system = messages
        .iter()
        .filter(|message| message.role == "system" && !message.content.trim().is_empty())
        .map(|message| message.content.trim())
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut adapted = messages
        .iter()
        .filter(|message| message.role != "system")
        .cloned()
        .collect::<Vec<_>>();
    if !system.is_empty() {
        if let Some(first_user) = adapted.iter_mut().find(|message| message.role == "user") {
            first_user.content = format!(
                "JadX session guidance (follow while answering):\n{system}\n\nUser request:\n{}",
                first_user.content
            );
        } else {
            adapted.insert(
                0,
                ChatMessage {
                    role: "user".into(),
                    content: format!("JadX session guidance (follow while answering):\n{system}"),
                    images: Vec::new(),
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                    ..Default::default()
                },
            );
        }
    }
    chat_api_messages_with_reasoning(&adapted, include_reasoning_content)
}

fn vertex_openapi_tool_schema(schema: &Value) -> Value {
    const SUPPORTED_KEYS: &[&str] = &[
        "type",
        "nullable",
        "required",
        "format",
        "description",
        "properties",
        "items",
        "enum",
        "anyOf",
        "$ref",
        "$defs",
    ];
    match schema {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| SUPPORTED_KEYS.contains(&key.as_str()))
                .map(|(key, value)| {
                    let normalized = if matches!(key.as_str(), "properties" | "$defs") {
                        match value {
                            Value::Object(definitions) => Value::Object(
                                definitions
                                    .iter()
                                    .map(|(name, definition)| {
                                        (name.clone(), vertex_openapi_tool_schema(definition))
                                    })
                                    .collect(),
                            ),
                            other => vertex_openapi_tool_schema(other),
                        }
                    } else {
                        vertex_openapi_tool_schema(value)
                    };
                    (key.clone(), normalized)
                })
                .collect(),
        ),
        Value::Array(values) => {
            Value::Array(values.iter().map(vertex_openapi_tool_schema).collect())
        }
        other => other.clone(),
    }
}

fn normalize_tool_arguments(value: &Value) -> Value {
    match value {
        Value::Object(_) => value.clone(),
        Value::Null => json!({}),
        Value::String(raw) => normalize_tool_argument_text(raw),
        other => other.clone(),
    }
}

fn normalize_tool_argument_text(raw: &str) -> Value {
    let trimmed = raw.trim();
    let unfenced = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```JSON"))
        .or_else(|| trimmed.strip_prefix("```"))
        .map(|value| value.strip_suffix("```").unwrap_or(value).trim())
        .unwrap_or(trimmed);
    let extracted = unfenced.find('{').and_then(|start| {
        unfenced[start..]
            .rfind('}')
            .map(|end| &unfenced[start..start + end + 1])
    });
    for candidate in [Some(unfenced), extracted] {
        let Some(candidate) = candidate else { continue };
        if let Ok(value) = serde_json::from_str::<Value>(candidate) {
            if let Value::String(nested) = value {
                return normalize_tool_argument_text(&nested);
            }
            return value;
        }
        if let Ok(value) = json5::from_str::<Value>(candidate) {
            return value;
        }
    }
    let unescaped = unfenced.replace("\\\"", "\"");
    if unescaped != unfenced {
        if let Ok(value) = serde_json::from_str::<Value>(&unescaped) {
            return value;
        }
        if let Ok(value) = json5::from_str::<Value>(&unescaped) {
            return value;
        }
    }
    Value::String(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn local_json_server(
        response_body: String,
        content_type: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            let mut header_end = None;
            let mut content_length = 0usize;
            loop {
                let read = socket.read(&mut chunk).await.unwrap();
                assert!(read > 0);
                request.extend_from_slice(&chunk[..read]);
                if header_end.is_none() {
                    if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        let headers =
                            String::from_utf8_lossy(&request[..index]).to_ascii_lowercase();
                        content_length = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(0);
                        header_end = Some(index + 4);
                    }
                }
                if header_end.is_some_and(|end| request.len() >= end + content_length) {
                    break;
                }
            }
            let request_text = String::from_utf8_lossy(&request).to_string();
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response_body.len(), response_body);
            socket.write_all(response.as_bytes()).await.unwrap();
            request_text
        });
        (format!("http://{address}"), server)
    }

    #[test]
    fn normalizes_tool_arguments_to_an_object() {
        assert_eq!(
            normalize_tool_arguments(&Value::String(r#"{"expression":"2 + 2"}"#.into())),
            json!({"expression": "2 + 2"})
        );
        assert_eq!(
            normalize_tool_arguments(&Value::String(r#""{\"path\":\"README.md\"}""#.into())),
            json!({"path": "README.md"})
        );
        assert_eq!(
            normalize_tool_arguments(&Value::String(
                "tool call: {\"path\":\"src/main.ts\",\"mode\":\"replace\"}".into()
            )),
            json!({"path": "src/main.ts", "mode": "replace"})
        );
        assert_eq!(
            normalize_tool_arguments(&Value::String(
                "{path: 'src/main.ts', mode: 'replace'}".into()
            )),
            json!({"path": "src/main.ts", "mode": "replace"})
        );
        assert_eq!(normalize_tool_arguments(&Value::Null), json!({}));
    }

    #[test]
    fn maps_thinking_off_to_explicit_provider_none() {
        assert_eq!(
            reasoning_request_value(&ThinkingMode::Off, &ReasoningEffort::XHigh),
            "none"
        );
        assert_eq!(
            reasoning_request_value(&ThinkingMode::On, &ReasoningEffort::Medium),
            "medium"
        );
    }

    #[test]
    fn provider_reasoning_fields_follow_provider_specific_shapes() {
        let mut deepseek = json!({});
        set_reasoning_payload(
            &mut deepseek,
            ReasoningStyle::DeepSeekThinking,
            &ThinkingMode::On,
            &ReasoningEffort::High,
        );
        assert_eq!(deepseek["thinking"]["type"], "enabled");
        assert_eq!(deepseek["reasoning_effort"], "high");

        let mut openrouter = json!({});
        set_reasoning_payload(
            &mut openrouter,
            ReasoningStyle::OpenRouter,
            &ThinkingMode::On,
            &ReasoningEffort::XHigh,
        );
        assert_eq!(openrouter["reasoning"]["effort"], "xhigh");

        let mut compatible = json!({});
        set_reasoning_payload(
            &mut compatible,
            ReasoningStyle::Effort,
            &ThinkingMode::Off,
            &ReasoningEffort::High,
        );
        assert_eq!(compatible["reasoning_effort"], "none");
    }

    #[test]
    fn compatible_provider_paths_and_credentials_are_isolated() {
        let provider = LmStudioProvider::openai_compatible(
            "https://openrouter.ai/api/v1",
            "example-secret".into(),
            "/chat/completions",
            "/models",
            "OpenRouter",
            ReasoningStyle::OpenRouter,
            true,
        );
        assert_eq!(
            provider.endpoint(&provider.chat_completions_path),
            "https://openrouter.ai/api/v1/chat/completions"
        );
        assert_eq!(
            provider.redact_secret("bad key example-secret".into()),
            "bad key [redacted]"
        );
        assert_eq!(provider.provider_name, "OpenRouter");
    }

    #[test]
    fn deepseek_tool_requests_fold_system_guidance_into_user_message() {
        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content: "Use tools when needed.".into(),
                images: Vec::new(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                ..Default::default()
            },
            ChatMessage {
                role: "user".into(),
                content: "Inspect the project.".into(),
                images: Vec::new(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                ..Default::default()
            },
        ];
        let result = deepseek_tool_messages(&messages, false);
        assert!(result.iter().all(|message| message["role"] != "system"));
        assert_eq!(result[0]["role"], "user");
        assert!(result[0]["content"]
            .as_str()
            .unwrap()
            .contains("Use tools when needed."));
        assert!(result[0]["content"]
            .as_str()
            .unwrap()
            .contains("Inspect the project."));
    }

    #[test]
    fn deepseek_tool_continuations_replay_ephemeral_reasoning_content() {
        let assistant = ChatMessage {
            role: "assistant".into(),
            content: String::new(),
            tool_calls: Some(vec![ToolCall {
                id: "call-1".into(),
                name: "workspace_read_file".into(),
                arguments: json!({"path":"index.html"}),
            }]),
            reasoning_content: Some("private provider continuation".into()),
            ..Default::default()
        };
        let without_replay =
            chat_api_messages_with_reasoning(std::slice::from_ref(&assistant), false);
        assert!(without_replay[0].get("reasoning_content").is_none());

        let with_replay = chat_api_messages_with_reasoning(std::slice::from_ref(&assistant), true);
        assert_eq!(
            with_replay[0]["reasoning_content"],
            "private provider continuation"
        );

        let serialized = serde_json::to_value(assistant).unwrap();
        assert!(serialized.get("reasoning_content").is_none());

        let empty_reasoning = ChatMessage {
            role: "assistant".into(),
            content: String::new(),
            tool_calls: Some(vec![ToolCall {
                id: "call-2".into(),
                name: "workspace_read_file".into(),
                arguments: json!({"path":"styles.css"}),
            }]),
            reasoning_content: Some(String::new()),
            ..Default::default()
        };
        let replay = chat_api_messages_with_reasoning(&[empty_reasoning], true);
        assert_eq!(replay[0]["reasoning_content"], "");
    }

    #[test]
    fn silent_stream_recovery_preserves_local_wait_and_shortens_cloud_wait() {
        let local = LmStudioProvider::new("http://127.0.0.1:1234");
        let deepseek = LmStudioProvider::openai_compatible(
            "https://api.deepseek.com",
            "test-key".into(),
            "/chat/completions",
            "/models",
            "DeepSeek",
            ReasoningStyle::DeepSeekThinking,
            false,
        );
        assert_eq!(local.stream_idle_timeout(), Duration::from_secs(180));
        assert_eq!(deepseek.stream_idle_timeout(), Duration::from_secs(45));
    }

    #[test]
    fn vertex_openapi_schema_removes_unsupported_json_schema_keywords_recursively() {
        let schema = json!({
            "type":"object",
            "required":["files"],
            "additionalProperties":false,
            "properties":{"files":{
                "type":"array",
                "maxItems":32,
                "items":{"type":"object","required":["path"],"additionalProperties":false,
                    "properties":{"path":{"type":"string","minLength":1,"description":"Relative path"}}}
            }}
        });
        let converted = vertex_openapi_tool_schema(&schema);
        assert_eq!(converted["type"], "object");
        assert_eq!(converted["required"][0], "files");
        assert!(converted.get("additionalProperties").is_none());
        assert!(converted["properties"]["files"].get("maxItems").is_none());
        assert!(converted["properties"]["files"]["items"]
            .get("additionalProperties")
            .is_none());
        assert!(
            converted["properties"]["files"]["items"]["properties"]["path"]
                .get("minLength")
                .is_none()
        );
        assert_eq!(
            converted["properties"]["files"]["items"]["properties"]["path"]["description"],
            "Relative path"
        );
    }

    #[test]
    fn hku_models_select_distinct_gateway_routes() {
        let deepseek = LmStudioProvider::hku_for_model(
            "https://api.hku.hk",
            "key".into(),
            "DeepSeek-V4-Flash",
        );
        assert_eq!(deepseek.hku_route, Some(HkuRoute::DeepSeek));
        assert_eq!(
            deepseek.chat_completions_path,
            "/foundry/student/models/chat/completions"
        );
        let gemini =
            LmStudioProvider::hku_for_model("https://api.hku.hk", "key".into(), "gemini-2.5-flash");
        assert_eq!(gemini.hku_route, Some(HkuRoute::Gemini));
        let azure =
            LmStudioProvider::hku_for_model("https://api.hku.hk", "key".into(), "gpt-5-mini");
        assert_eq!(azure.hku_route, Some(HkuRoute::AzureChat));
        let endpoint = azure
            .hku_azure_endpoint("gpt-5-mini", &["chat", "completions"])
            .unwrap();
        assert_eq!(
            endpoint.path(),
            "/openai/student/deployments/gpt-5-mini/chat/completions"
        );
        assert_eq!(endpoint.query(), Some("api-version=2025-04-01-preview"));
        let responses =
            LmStudioProvider::hku_for_model("https://api.hku.hk", "key".into(), "gpt-5.3-codex");
        assert_eq!(responses.hku_route, Some(HkuRoute::AzureResponses));
        let claude =
            LmStudioProvider::hku_for_model("https://api.hku.hk", "key".into(), "claude-haiku-4.5");
        assert!(claude.hku_claude_converse);
        assert_eq!(claude.api_key_header.as_deref(), Some("api-key"));
        let image =
            LmStudioProvider::hku_for_model("https://api.hku.hk", "key".into(), "gpt-image-2");
        assert_eq!(image.hku_route, Some(HkuRoute::AzureImage));
        let embedding = LmStudioProvider::hku_for_model(
            "https://api.hku.hk",
            "key".into(),
            "text-embedding-3-small",
        );
        assert_eq!(embedding.hku_route, Some(HkuRoute::AzureEmbedding));
    }

    #[test]
    fn usage_normalizes_openai_anthropic_and_gemini_cache_metrics() {
        let openai = parse_usage(&json!({
            "prompt_tokens": 100,
            "prompt_tokens_details": { "cached_tokens": 30 },
            "completion_tokens": 12,
            "total_tokens": 112
        }));
        assert_eq!(openai.input_tokens, Some(100));
        assert_eq!(openai.cached_input_tokens, Some(30));
        assert_eq!(openai.cache_miss_tokens, Some(70));
        assert_eq!(openai.output_tokens, Some(12));

        let anthropic = parse_usage(&json!({
            "input_tokens": 50,
            "cache_read_input_tokens": 20,
            "cache_creation_input_tokens": 10,
            "output_tokens": 30
        }));
        assert_eq!(anthropic.input_tokens, Some(80));
        assert_eq!(anthropic.cached_input_tokens, Some(20));
        assert_eq!(anthropic.cache_miss_tokens, Some(60));
        assert_eq!(anthropic.cache_write_tokens, Some(10));

        let gemini = parse_usage(&json!({
            "promptTokenCount": 95,
            "cachedContentTokenCount": 40,
            "candidatesTokenCount": 5,
            "totalTokenCount": 100
        }));
        assert_eq!(gemini.input_tokens, Some(95));
        assert_eq!(gemini.cached_input_tokens, Some(40));
        assert_eq!(gemini.cache_miss_tokens, Some(55));
        assert_eq!(gemini.output_tokens, Some(5));
        assert_eq!(
            parse_usage(&json!({ "prompt_tokens": 4 })).cached_input_tokens,
            None
        );
    }

    #[tokio::test]
    async fn hku_azure_image_generation_uses_deployment_endpoint_and_persists_image_shape() {
        let (base, server) = local_json_server(
            json!({
                "data": [{ "b64_json": "AQID" }],
                "output_format": "png",
                "usage": { "input_tokens": 13, "output_tokens": 4, "total_tokens": 17 }
            })
            .to_string(),
            "application/json",
        )
        .await;
        let provider = LmStudioProvider::hku_for_model(base, "example-token".into(), "gpt-image-2");
        let messages = [ChatMessage {
            role: "user".into(),
            content: "Blue geometric mountain landscape".into(),
            images: vec![],
            name: None,
            tool_call_id: None,
            tool_calls: None,
            ..Default::default()
        }];
        let result = provider
            .complete_hku_image("gpt-image-2", &messages)
            .await
            .unwrap();
        let request = server.await.unwrap();
        assert!(request.starts_with(
            "POST /openai/student/gpt-image-2/images/generations?api-version=2025-04-01-preview "
        ));
        assert!(request.contains("api-key: example-token"));
        assert!(request.contains("Blue geometric mountain landscape"));
        assert_eq!(result.images.len(), 1);
        assert_eq!(result.images[0].mime_type, "image/png");
        assert_eq!(result.images[0].size_bytes, 3);
        assert_eq!(result.usage.input_tokens, Some(13));
    }

    #[tokio::test]
    async fn hku_azure_embedding_uses_deployment_endpoint_and_returns_usage() {
        let (base, server) = local_json_server(
            json!({
                "data": [{ "embedding": [0.25, -0.5, 0.75], "index": 0 }],
                "usage": { "prompt_tokens": 8, "total_tokens": 8 }
            })
            .to_string(),
            "application/json",
        )
        .await;
        let provider =
            LmStudioProvider::hku_for_model(base, "example-token".into(), "text-embedding-3-small");
        let result = provider
            .create_hku_embedding("text-embedding-3-small", "A local test sentence")
            .await
            .unwrap();
        let request = server.await.unwrap();
        assert!(request.starts_with(
            "POST /openai/student/deployments/text-embedding-3-small/embeddings?api-version=2025-04-01-preview "
        ));
        assert!(request
            .to_ascii_lowercase()
            .contains("api-key: example-token"));
        assert!(request.contains("A local test sentence"));
        assert_eq!(result.embedding, vec![0.25, -0.5, 0.75]);
        assert_eq!(result.usage.input_tokens, Some(8));
    }

    #[tokio::test]
    async fn hku_gemini_emits_text_and_images_and_sends_vertex_generation_config() {
        let (base, server) = local_json_server(
            json!({
                "candidates": [{ "content": { "parts": [
                    { "text": "A generated illustration" },
                    { "inlineData": { "mimeType": "image/png", "data": "AQ==" } }
                ] } }],
                "usageMetadata": { "promptTokenCount": 40, "cachedContentTokenCount": 10, "candidatesTokenCount": 6, "totalTokenCount": 46 }
            }).to_string(),
            "application/json",
        ).await;
        let provider = LmStudioProvider::hku_for_model(
            base,
            "example-token".into(),
            "gemini-3-pro-image-preview",
        );
        let messages = [ChatMessage {
            role: "user".into(),
            content: "Draw this as an editorial illustration".into(),
            images: vec![ImageInput {
                name: "reference.png".into(),
                mime_type: "image/png".into(),
                data_url: "data:image/png;base64,AQ==".into(),
                size_bytes: 1,
            }],
            name: None,
            tool_call_id: None,
            tool_calls: None,
            ..Default::default()
        }];
        let result = provider
            .complete_hku_gemini(
                "gemini-3-pro-image-preview",
                &messages,
                0.4,
                0.9,
                1024,
                &[],
                &ThinkingMode::On,
                &ReasoningEffort::High,
            )
            .await
            .unwrap();
        let request = server.await.unwrap();
        assert!(
            request.starts_with(
                "POST /vertexai/student/models/gemini-3-pro-image-preview%3AgenerateContent "
            ) || request.starts_with(
                "POST /vertexai/student/models/gemini-3-pro-image-preview:generateContent "
            )
        );
        assert!(request.contains("\"thinkingLevel\":\"high\""));
        assert!(request.contains("\"responseModalities\":[\"TEXT\",\"IMAGE\"]"));
        assert!(request.contains("\"mimeType\":\"image/png\""));
        assert_eq!(result.text, "A generated illustration");
        assert_eq!(result.images.len(), 1);
        assert_eq!(result.usage.cached_input_tokens, Some(10));
        assert_eq!(result.usage.cache_miss_tokens, Some(30));
    }

    #[tokio::test]
    async fn hku_gemini_omits_jadx_function_tools_in_its_limited_offering() {
        let (base, server) = local_json_server(
            json!({
                "candidates": [{ "content": { "parts": [{ "text": "Hello without tool access." }] } }],
                "usageMetadata": { "promptTokenCount": 9, "candidatesTokenCount": 5, "totalTokenCount": 14 }
            })
            .to_string(),
            "application/json",
        )
        .await;
        let provider =
            LmStudioProvider::hku_for_model(base, "example-token".into(), "gemini-2.5-flash");
        let messages = [
            ChatMessage {
                role: "user".into(),
                content: "Read a file in my workspace".into(),
                images: vec![],
                name: None,
                tool_call_id: None,
                tool_calls: None,
                ..Default::default()
            },
            ChatMessage {
                role: "assistant".into(),
                content: String::new(),
                images: vec![],
                name: None,
                tool_call_id: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call-1".into(),
                    name: "filesystem_read".into(),
                    arguments: json!({"path":"index.html"}),
                }]),
                ..Default::default()
            },
            ChatMessage {
                role: "tool".into(),
                content: "prior file contents".into(),
                images: vec![],
                name: Some("filesystem_read".into()),
                tool_call_id: Some("call-1".into()),
                tool_calls: None,
                ..Default::default()
            },
        ];
        let tools = [ToolDefinition {
            name: "filesystem_read".into(),
            description: "Read an approved workspace file".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            permission: crate::types::PermissionLevel::Safe,
        }];
        let result = provider
            .complete_hku_gemini(
                "gemini-2.5-flash",
                &messages,
                0.2,
                0.8,
                128,
                &tools,
                &ThinkingMode::Off,
                &ReasoningEffort::Low,
            )
            .await
            .unwrap();
        let request = server.await.unwrap();
        let body = request.split("\r\n\r\n").nth(1).unwrap_or_default();
        assert!(!body.contains("\"tools\""));
        assert!(!body.contains("functionCall"));
        assert!(!body.contains("functionResponse"));
        assert!(body.contains("Prior JadX tool result from filesystem_read"));
        assert!(body.contains("prior file contents"));
        assert!(result
            .warning
            .as_deref()
            .unwrap_or_default()
            .contains("does not accept JadX function tools"));
        assert_eq!(result.text, "Hello without tool access.");
    }

    #[tokio::test]
    async fn hku_azure_chat_stream_uses_deployment_id_and_query_version() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            let mut header_end = None;
            let mut content_length = 0usize;
            loop {
                let read = socket.read(&mut chunk).await.unwrap();
                assert!(read > 0);
                request.extend_from_slice(&chunk[..read]);
                if header_end.is_none() {
                    if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        let headers =
                            String::from_utf8_lossy(&request[..index]).to_ascii_lowercase();
                        content_length = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(0);
                        header_end = Some(index + 4);
                    }
                }
                if header_end.is_some_and(|end| request.len() >= end + content_length) {
                    break;
                }
            }
            let request_text = String::from_utf8_lossy(&request).to_string();
            let body = format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({ "choices": [{ "delta": { "content": "HKU Azure" } }], "usage": { "prompt_tokens": 11, "completion_tokens": 3, "total_tokens": 14 } })
            );
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            socket.write_all(response.as_bytes()).await.unwrap();
            request_text
        });
        let provider = LmStudioProvider::hku_for_model(
            format!("http://{address}"),
            "example-token".into(),
            "gpt-5-mini",
        );
        let message = ChatMessage {
            role: "user".into(),
            content: "Hello".into(),
            images: vec![],
            name: None,
            tool_call_id: None,
            tool_calls: None,
            ..Default::default()
        };
        let result = provider
            .stream_response(
                "gpt-5-mini",
                &[message],
                0.2,
                0.9,
                32,
                &[],
                &ThinkingMode::Off,
                &ReasoningEffort::Low,
                |_, _| {},
            )
            .await
            .unwrap();
        let request = server.await.unwrap();
        assert!(request.starts_with("POST /openai/student/deployments/gpt-5-mini/chat/completions?api-version=2025-04-01-preview "));
        assert!(request
            .to_ascii_lowercase()
            .contains("api-key: example-token"));
        let body = request.split("\r\n\r\n").nth(1).unwrap();
        assert!(!body.contains("\"model\""));
        assert_eq!(result.text, "HKU Azure");
        assert_eq!(result.usage.input_tokens, Some(11));
    }

    #[tokio::test]
    async fn compatible_provider_sends_bearer_key_and_parses_streamed_text_and_reasoning() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            let mut header_end = None;
            let mut content_length = 0usize;
            loop {
                let read = socket.read(&mut chunk).await.unwrap();
                assert!(read > 0, "client disconnected before sending request body");
                request.extend_from_slice(&chunk[..read]);
                if header_end.is_none() {
                    if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        let end = index + 4;
                        let headers =
                            String::from_utf8_lossy(&request[..index]).to_ascii_lowercase();
                        content_length = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(0);
                        header_end = Some(end);
                    }
                }
                if header_end.is_some_and(|end| request.len() >= end + content_length) {
                    break;
                }
            }
            let request_text = String::from_utf8_lossy(&request).to_string();
            let body = format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({
                    "choices": [{ "delta": { "content": "Hello", "reasoning_content": "Short rationale" } }],
                    "usage": { "prompt_tokens": 7, "completion_tokens": 1, "total_tokens": 8 }
                })
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            request_text
        });

        let provider = LmStudioProvider::openai_compatible(
            format!("http://{address}"),
            "unit-example-token".into(),
            "/v1/chat/completions",
            "/v1/models",
            "Compatible Test",
            ReasoningStyle::Effort,
            false,
        );
        let messages = [ChatMessage {
            role: "user".into(),
            content: "Say hello".into(),
            images: Vec::new(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            ..Default::default()
        }];
        let result = provider
            .stream_response(
                "test-model",
                &messages,
                0.2,
                0.9,
                64,
                &[],
                &ThinkingMode::On,
                &ReasoningEffort::High,
                |_, _| {},
            )
            .await
            .unwrap();
        let request = server.await.unwrap();
        assert!(request.starts_with("POST /v1/chat/completions "));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer unit-example-token"));
        let request_body = request.split("\r\n\r\n").nth(1).unwrap();
        let payload: Value = serde_json::from_str(request_body).unwrap();
        assert_eq!(payload["model"], "test-model");
        assert_eq!(payload["reasoning_effort"], "high");
        assert_eq!(result.text, "Hello");
        assert_eq!(result.reasoning, "Short rationale");
        assert_eq!(result.usage.input_tokens, Some(7));
    }

    #[tokio::test]
    async fn hku_vertex_streams_tool_calls_with_provider_compatible_schema() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            let mut expected_end = None;
            loop {
                let read = socket.read(&mut chunk).await.unwrap();
                assert!(read > 0);
                request.extend_from_slice(&chunk[..read]);
                if expected_end.is_none() {
                    if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        let headers =
                            String::from_utf8_lossy(&request[..index]).to_ascii_lowercase();
                        let body_len = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .and_then(|value| value.parse::<usize>().ok())
                            .unwrap_or(0);
                        expected_end = Some(index + 4 + body_len);
                    }
                }
                if expected_end.is_some_and(|end| request.len() >= end) {
                    break;
                }
            }
            let request_text = String::from_utf8_lossy(&request).to_string();
            let body = format!(
                "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"filesystem_write_many","arguments":r#"{"files":["#}}]}}]}),
                json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":r#"{"path":"index.html","content":"<h1>Updated</h1>","mode":"replace"}]}"#}}]}}]}),
                json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]})
            );
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            socket.write_all(response.as_bytes()).await.unwrap();
            request_text
        });
        let provider = LmStudioProvider::hku_for_model(
            format!("http://{address}"),
            "example-hku-token".into(),
            "qwen/qwen3-next-80b-a3b-instruct-maas",
        );
        let message = ChatMessage {
            role: "user".into(),
            content: "Say hello".into(),
            images: Vec::new(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            ..Default::default()
        };
        let tool = ToolDefinition {
            name: "filesystem_write_many".into(),
            description: "Apply a bounded file batch.".into(),
            permission: crate::types::PermissionLevel::Ask,
            parameters: json!({
                "type": "object",
                "required": ["files"],
                "additionalProperties": false,
                "properties": {
                    "files": {
                        "type": "array",
                        "maxItems": 32,
                        "items": {
                            "type": "object",
                            "required": ["path", "content", "mode"],
                            "additionalProperties": false,
                            "properties": {
                                "path": {"type": "string", "minLength": 1},
                                "content": {"type": "string"},
                                "mode": {"type": "string", "enum": ["create", "replace", "patch"]}
                            }
                        }
                    }
                }
            }),
        };
        let result = provider
            .stream_response(
                "qwen/qwen3-next-80b-a3b-instruct-maas",
                &[message],
                0.2,
                0.9,
                32,
                &[tool],
                &ThinkingMode::Off,
                &ReasoningEffort::Low,
                |_, _| {},
            )
            .await
            .unwrap();
        let request = server.await.unwrap();
        assert!(request.starts_with("POST /vertexai/student/chat/completions "));
        assert!(request
            .to_ascii_lowercase()
            .contains("api-key: example-hku-token"));
        assert!(!request
            .to_ascii_lowercase()
            .contains("authorization: bearer"));
        let payload: Value =
            serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(payload["model"], "qwen/qwen3-next-80b-a3b-instruct-maas");
        assert_eq!(payload["tool_choice"], "auto");
        assert_eq!(
            payload["tools"][0]["function"]["name"],
            "filesystem_write_many"
        );
        let api_schema = &payload["tools"][0]["function"]["parameters"];
        assert!(api_schema.get("additionalProperties").is_none());
        assert!(api_schema["properties"]["files"].get("maxItems").is_none());
        assert!(api_schema["properties"]["files"]["items"]
            .get("additionalProperties")
            .is_none());
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].id, "call-1");
        assert_eq!(result.tool_calls[0].name, "filesystem_write_many");
        assert_eq!(
            result.tool_calls[0].arguments["files"][0]["path"],
            "index.html"
        );
        assert_eq!(
            result.tool_calls[0].arguments["files"][0]["mode"],
            "replace"
        );
        assert!(payload.get("reasoning_effort").is_none());
    }

    #[tokio::test]
    async fn hku_claude_uses_bedrock_messages_schema_and_parses_response() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            let mut expected_end = None;
            loop {
                let read = socket.read(&mut chunk).await.unwrap();
                assert!(read > 0);
                request.extend_from_slice(&chunk[..read]);
                if expected_end.is_none() {
                    if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        let headers =
                            String::from_utf8_lossy(&request[..index]).to_ascii_lowercase();
                        let body_len = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .and_then(|value| value.parse::<usize>().ok())
                            .unwrap_or(0);
                        expected_end = Some(index + 4 + body_len);
                    }
                }
                if expected_end.is_some_and(|end| request.len() >= end) {
                    break;
                }
            }
            let request_text = String::from_utf8_lossy(&request).to_string();
            let body = json!({
                "output": { "message": { "content": [{ "text": "Claude response" }], "role": "assistant" } },
                "usage": { "inputTokens": 9, "outputTokens": 3, "totalTokens": 12 },
                "stopReason": "end_turn"
            }).to_string();
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            socket.write_all(response.as_bytes()).await.unwrap();
            request_text
        });
        let provider = LmStudioProvider::hku_claude_converse(
            format!("http://{address}"),
            "example-hku-token".into(),
        );
        let messages = [
            ChatMessage {
                role: "system".into(),
                content: "Follow these instructions.".into(),
                images: Vec::new(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                ..Default::default()
            },
            ChatMessage {
                role: "user".into(),
                content: "Say hello".into(),
                images: Vec::new(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                ..Default::default()
            },
        ];
        let result = provider
            .stream_response(
                "claude-opus-4.6",
                &messages,
                0.2,
                0.9,
                64,
                &[],
                &ThinkingMode::Off,
                &ReasoningEffort::Low,
                |_, _| {},
            )
            .await
            .unwrap();
        let request = server.await.unwrap();
        assert!(request.starts_with("POST /claude/student/model/claude-opus-4.6/converse "));
        assert!(request
            .to_ascii_lowercase()
            .contains("api-key: example-hku-token"));
        let payload: Value =
            serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(payload["anthropic_version"], "bedrock-2023-05-31");
        assert!(payload.get("system").is_none());
        assert!(payload.get("top_p").is_none());
        assert!(payload["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("Follow these instructions."));
        assert!(payload["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("without JadX tools"));
        assert_eq!(payload["messages"][0]["role"], "user");
        assert_eq!(result.text, "Claude response");
        assert_eq!(result.usage.input_tokens, Some(9));
        assert_eq!(result.usage.output_tokens, Some(3));
        assert_eq!(result.transport, "hku_bedrock_converse");
    }

    #[test]
    fn sends_tool_screenshots_as_user_images_after_tool_results() {
        let messages = vec![
            ChatMessage {
                role: "assistant".into(),
                content: String::new(),
                images: Vec::new(),
                name: None,
                tool_call_id: None,
                tool_calls: Some(vec![
                    ToolCall {
                        id: "call-1".into(),
                        name: "computer_use_start".into(),
                        arguments: json!({"url":"https://example.com"}),
                    },
                    ToolCall {
                        id: "call-2".into(),
                        name: "computer_use_status".into(),
                        arguments: json!({}),
                    },
                ]),
                ..Default::default()
            },
            ChatMessage {
                role: "tool".into(),
                content: "Screenshot returned".into(),
                images: vec![crate::types::ImageInput {
                    name: "browser.jpg".into(),
                    mime_type: "image/jpeg".into(),
                    data_url: "data:image/jpeg;base64,AA==".into(),
                    size_bytes: 1,
                }],
                name: Some("computer_use_start".into()),
                tool_call_id: Some("call-1".into()),
                tool_calls: None,
                ..Default::default()
            },
            ChatMessage {
                role: "tool".into(),
                content: "A second tool result".into(),
                images: Vec::new(),
                name: Some("computer_use_status".into()),
                tool_call_id: Some("call-2".into()),
                tool_calls: None,
                ..Default::default()
            },
        ];
        let api_messages = chat_api_messages(&messages);
        assert_eq!(api_messages[1]["role"], "tool");
        assert_eq!(api_messages[1]["content"], "Screenshot returned");
        assert_eq!(api_messages[2]["role"], "tool");
        assert_eq!(api_messages[3]["role"], "user");
        assert_eq!(api_messages[3]["content"][1]["type"], "image_url");
    }

    #[test]
    fn drops_tool_results_without_a_preceding_tool_call() {
        let messages = vec![
            ChatMessage {
                role: "user".into(),
                content: "List the files".into(),
                images: Vec::new(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                ..Default::default()
            },
            ChatMessage {
                role: "assistant".into(),
                content: "Done.".into(),
                images: Vec::new(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                ..Default::default()
            },
            ChatMessage {
                role: "tool".into(),
                content: "orphan result".into(),
                images: Vec::new(),
                name: Some("filesystem_list".into()),
                tool_call_id: Some("call-orphan".into()),
                tool_calls: None,
                ..Default::default()
            },
        ];
        let api_messages = chat_api_messages(&messages);
        assert_eq!(api_messages.len(), 2);
        assert!(api_messages.iter().all(|message| message["role"] != "tool"));
    }

    #[test]
    fn answers_tool_calls_that_lost_their_results() {
        let messages = vec![
            ChatMessage {
                role: "user".into(),
                content: "Read the file".into(),
                images: Vec::new(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                ..Default::default()
            },
            ChatMessage {
                role: "assistant".into(),
                content: String::new(),
                images: Vec::new(),
                name: None,
                tool_call_id: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call-1".into(),
                    name: "workspace_read_file".into(),
                    arguments: serde_json::json!({ "path": "README.md" }),
                }]),
                ..Default::default()
            },
            ChatMessage {
                role: "user".into(),
                content: "Continue anyway".into(),
                images: Vec::new(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                ..Default::default()
            },
        ];
        let api_messages = chat_api_messages(&messages);
        assert_eq!(api_messages.len(), 4);
        assert_eq!(api_messages[1]["role"], "assistant");
        assert_eq!(api_messages[2]["role"], "tool");
        assert_eq!(api_messages[2]["tool_call_id"], "call-1");
        assert_eq!(api_messages[3]["role"], "user");
    }

    #[test]
    fn keeps_complete_tool_sequences_untouched() {
        let messages = vec![
            ChatMessage {
                role: "user".into(),
                content: "Read two files".into(),
                images: Vec::new(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                ..Default::default()
            },
            ChatMessage {
                role: "assistant".into(),
                content: String::new(),
                images: Vec::new(),
                name: None,
                tool_call_id: None,
                tool_calls: Some(vec![
                    ToolCall {
                        id: "call-1".into(),
                        name: "workspace_read_file".into(),
                        arguments: serde_json::json!({ "path": "a.ts" }),
                    },
                    ToolCall {
                        id: "call-2".into(),
                        name: "workspace_read_file".into(),
                        arguments: serde_json::json!({ "path": "b.ts" }),
                    },
                ]),
                ..Default::default()
            },
            ChatMessage {
                role: "tool".into(),
                content: "one".into(),
                images: Vec::new(),
                name: Some("workspace_read_file".into()),
                tool_call_id: Some("call-1".into()),
                tool_calls: None,
                ..Default::default()
            },
            ChatMessage {
                role: "tool".into(),
                content: "two".into(),
                images: Vec::new(),
                name: Some("workspace_read_file".into()),
                tool_call_id: Some("call-2".into()),
                tool_calls: None,
                ..Default::default()
            },
            ChatMessage {
                role: "assistant".into(),
                content: "Both read.".into(),
                images: Vec::new(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                ..Default::default()
            },
        ];
        let api_messages = chat_api_messages(&messages);
        assert_eq!(api_messages.len(), 5);
        assert_eq!(api_messages[2]["tool_call_id"], "call-1");
        assert_eq!(api_messages[3]["tool_call_id"], "call-2");
    }
}

fn effort_value(effort: &ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::XHigh => "xhigh",
    }
}

fn reasoning_request_value(thinking_mode: &ThinkingMode, effort: &ReasoningEffort) -> &'static str {
    if matches!(thinking_mode, ThinkingMode::Off) {
        "none"
    } else {
        effort_value(effort)
    }
}

fn set_reasoning_payload(
    payload: &mut Value,
    style: ReasoningStyle,
    thinking_mode: &ThinkingMode,
    effort: &ReasoningEffort,
) {
    match style {
        ReasoningStyle::Effort => {
            payload["reasoning_effort"] = json!(reasoning_request_value(thinking_mode, effort));
        }
        ReasoningStyle::DeepSeekThinking => {
            payload["thinking"] = json!({
                "type": if matches!(thinking_mode, ThinkingMode::On) { "enabled" } else { "disabled" }
            });
            payload["reasoning_effort"] = json!(match (thinking_mode, effort) {
                (ThinkingMode::Off, _) => "none",
                (ThinkingMode::On, ReasoningEffort::XHigh) => "max",
                (ThinkingMode::On, _) => "high",
            });
        }
        ReasoningStyle::OpenRouter => {
            if matches!(thinking_mode, ThinkingMode::On) {
                payload["reasoning"] = json!({ "effort": effort_value(effort) });
            } else {
                payload["reasoning"] = json!({ "effort": "none" });
            }
        }
        ReasoningStyle::None => {}
    }
}

fn set_gemini_thinking_config(
    generation_config: &mut Value,
    model: &str,
    thinking_mode: &ThinkingMode,
    effort: &ReasoningEffort,
) {
    if model.contains("2.5") {
        let thinking_budget = if matches!(thinking_mode, ThinkingMode::Off) {
            0
        } else {
            match effort {
                ReasoningEffort::Low => 1_024,
                ReasoningEffort::Medium => 4_096,
                ReasoningEffort::High => 8_192,
                ReasoningEffort::XHigh => 16_384,
            }
        };
        generation_config["thinkingConfig"] = json!({ "thinkingBudget": thinking_budget });
    } else if model.starts_with("gemini-3") {
        let level = if matches!(thinking_mode, ThinkingMode::Off) {
            if model.contains("3.1-pro") {
                "low"
            } else {
                "minimal"
            }
        } else if model.contains("image") {
            match effort {
                ReasoningEffort::High | ReasoningEffort::XHigh => "high",
                _ => "minimal",
            }
        } else {
            match effort {
                ReasoningEffort::Low => "low",
                ReasoningEffort::Medium => "medium",
                ReasoningEffort::High | ReasoningEffort::XHigh => "high",
            }
        };
        generation_config["thinkingConfig"] = json!({ "thinkingLevel": level });
    }
}

fn unsupported_reasoning(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    (body.contains("reasoning") || body.contains("thinking"))
        && (body.contains("support")
            || body.contains("invalid")
            || body.contains("unknown")
            || body.contains("setting"))
}

async fn parse_json_response(response: reqwest::Response) -> Result<Value, InferenceError> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(InferenceError::Api { status, body });
    }
    response
        .json::<Value>()
        .await
        .map_err(|e| InferenceError::Parse(e.to_string()))
}

fn parse_model(value: &Value) -> Option<ModelInfo> {
    let key = value
        .get("key")
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)?
        .to_string();
    let quantization = value.get("quantization").map(|value| {
        if let Some(name) = value.as_str() {
            QuantizationInfo {
                name: Some(name.to_string()),
                bits_per_weight: None,
            }
        } else {
            QuantizationInfo {
                name: value
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                bits_per_weight: value.get("bits_per_weight").and_then(Value::as_f64),
            }
        }
    });
    let loaded_instances = value
        .get("loaded_instances")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .map(|instance| LoadedInstance {
                    id: instance
                        .get("id")
                        .or_else(|| instance.get("instance_id"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    context_length: instance
                        .get("context_length")
                        .or_else(|| instance.pointer("/config/context_length"))
                        .and_then(Value::as_u64),
                })
                .collect()
        })
        .unwrap_or_default();
    let display_name = value
        .get("display_name")
        .or_else(|| value.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let haystack =
        format!("{} {}", key, display_name.as_deref().unwrap_or_default()).to_ascii_lowercase();
    Some(ModelInfo {
        key,
        publisher: value
            .get("publisher")
            .and_then(Value::as_str)
            .map(str::to_string),
        display_name,
        architecture: value
            .get("architecture")
            .or_else(|| value.get("arch"))
            .and_then(Value::as_str)
            .map(str::to_string),
        quantization,
        size_bytes: value.get("size_bytes").and_then(Value::as_u64),
        params_string: value
            .get("params_string")
            .and_then(Value::as_str)
            .map(str::to_string),
        loaded_instances,
        max_context_length: value
            .get("max_context_length")
            .or_else(|| value.get("context_length"))
            .and_then(Value::as_u64),
        compatibility_type: value
            .get("compatibility_type")
            .or_else(|| value.get("format"))
            .and_then(Value::as_str)
            .map(str::to_string),
        state: value
            .get("state")
            .and_then(Value::as_str)
            .map(str::to_string),
        selected_variant: value
            .get("selected_variant")
            .and_then(Value::as_str)
            .map(str::to_string),
        capabilities: ModelCapabilities {
            vision: value
                .get("vision")
                .and_then(Value::as_bool)
                .or_else(|| {
                    value
                        .pointer("/capabilities/vision")
                        .and_then(Value::as_bool)
                })
                .unwrap_or_else(|| haystack.contains("vl") || haystack.contains("vision")),
            reasoning: value
                .get("reasoning")
                .and_then(Value::as_bool)
                .or_else(|| {
                    value
                        .pointer("/capabilities/reasoning")
                        .map(|value| !value.is_null())
                })
                .unwrap_or(false),
            trained_for_tool_use: value
                .get("trained_for_tool_use")
                .and_then(Value::as_bool)
                .or_else(|| {
                    value
                        .pointer("/capabilities/trained_for_tool_use")
                        .and_then(Value::as_bool)
                })
                .unwrap_or(false),
            image_generation: value
                .get("image_generation")
                .and_then(Value::as_bool)
                .or_else(|| {
                    value
                        .pointer("/capabilities/image_generation")
                        .and_then(Value::as_bool)
                })
                .unwrap_or(false),
        },
    })
}

fn parse_usage(value: &Value) -> UsageStats {
    let number = |paths: &[&str]| {
        paths
            .iter()
            .find_map(|path| value.pointer(path).and_then(Value::as_u64))
    };
    let anthropic_input = value.get("input_tokens").and_then(Value::as_u64);
    let cache_read = number(&[
        "/cache_read_input_tokens",
        "/cache_read_input_token_count",
        "/cacheReadInputTokens",
        "/cacheReadInputTokenCount",
        "/cachedContentTokenCount",
    ]);
    let cache_write = number(&[
        "/cache_creation_input_tokens",
        "/cache_write_input_token_count",
        "/cacheWriteInputTokens",
        "/cacheWriteInputTokenCount",
    ]);
    let prompt = number(&[
        "/prompt_tokens",
        "/input_tokens",
        "/inputTokens",
        "/promptTokenCount",
    ]);
    let cached_openai = number(&[
        "/prompt_tokens_details/cached_tokens",
        "/input_tokens_details/cached_tokens",
        "/promptTokenDetails/cachedTokens",
    ]);
    let cached = cache_read.or(cached_openai);
    let input_tokens =
        if anthropic_input.is_some() && (cache_read.is_some() || cache_write.is_some()) {
            Some(
                anthropic_input
                    .unwrap_or_default()
                    .saturating_add(cache_read.unwrap_or_default())
                    .saturating_add(cache_write.unwrap_or_default()),
            )
        } else {
            prompt
        };
    let cache_miss = if anthropic_input.is_some() && (cache_read.is_some() || cache_write.is_some())
    {
        Some(
            anthropic_input
                .unwrap_or_default()
                .saturating_add(cache_write.unwrap_or_default()),
        )
    } else if let (Some(input), Some(cached)) = (input_tokens, cached) {
        Some(input.saturating_sub(cached))
    } else {
        number(&["/cache_miss_input_tokens", "/cacheMissInputTokens"])
    };
    let output_tokens = number(&[
        "/output_tokens",
        "/outputTokens",
        "/completion_tokens",
        "/candidatesTokenCount",
    ]);
    let total_tokens =
        number(&["/total_tokens", "/totalTokens", "/totalTokenCount"]).or_else(|| {
            input_tokens
                .zip(output_tokens)
                .map(|(input, output)| input.saturating_add(output))
        });
    UsageStats {
        input_tokens,
        cached_input_tokens: cached,
        cache_miss_tokens: cache_miss,
        cache_write_tokens: cache_write,
        output_tokens,
        total_tokens,
        ..Default::default()
    }
}

fn split_data_url<'a>(
    data_url: &'a str,
    fallback_mime: &'a str,
) -> Result<(&'a str, &'a str), InferenceError> {
    let Some((header, data)) = data_url.split_once(',') else {
        return Err(InferenceError::Parse(
            "Image attachment is not a data URL.".into(),
        ));
    };
    if !header.starts_with("data:") || !header.contains(";base64") {
        return Err(InferenceError::Parse(
            "Image attachment must use base64 data URL encoding.".into(),
        ));
    }
    let mime = header
        .strip_prefix("data:")
        .and_then(|part| part.split(';').next())
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_mime);
    Ok((mime, data))
}

fn image_extension(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "png",
    }
}

use crate::types::LoadedInstance;
