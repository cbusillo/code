use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::HashMap;
use ts_rs::TS;

/// Wire protocol that the provider speaks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum WireApi {
    /// The Responses API exposed by OpenAI at `/v1/responses`.
    Responses,

    /// Experimental: Responses API over WebSocket transport.
    #[serde(rename = "responses_websocket")]
    ResponsesWebsocket,

    /// Regular Chat Completions compatible with `/v1/chat/completions`.
    #[default]
    Chat,
}

/// Serializable representation of a provider definition.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, TS)]
pub struct ModelProviderInfo {
    /// Friendly display name.
    pub name: String,
    /// Base URL for the provider's OpenAI-compatible API.
    pub base_url: Option<String>,
    /// Environment variable that stores the user's API key for this provider.
    pub env_key: Option<String>,

    /// Optional instructions to help the user get a valid value for the
    /// variable and set it.
    pub env_key_instructions: Option<String>,

    /// Value to use with `Authorization: Bearer <token>` header.
    pub experimental_bearer_token: Option<String>,

    /// Which wire protocol this provider expects.
    #[serde(default)]
    pub wire_api: WireApi,

    /// Optional query parameters to append to the base URL.
    pub query_params: Option<HashMap<String, String>>,

    /// Additional HTTP headers to include in requests to this provider where
    /// the (key, value) pairs are the header name and value.
    pub http_headers: Option<HashMap<String, String>>,

    /// Optional HTTP headers to include in requests to this provider where the
    /// (key, value) pairs are the header name and _environment variable_ whose
    /// value should be used.
    pub env_http_headers: Option<HashMap<String, String>>,

    /// Maximum number of times to retry a failed HTTP request to this provider.
    pub request_max_retries: Option<u64>,

    /// Number of times to retry reconnecting a dropped streaming response before failing.
    pub stream_max_retries: Option<u64>,

    /// Idle timeout (in milliseconds) to wait for activity on a streaming response before treating
    /// the connection as lost.
    pub stream_idle_timeout_ms: Option<u64>,

    /// Whether this provider requires some form of standard authentication.
    #[serde(default)]
    pub requires_openai_auth: bool,

    /// Optional OpenRouter-specific configuration for routing preferences and metadata.
    #[serde(default)]
    pub openrouter: Option<OpenRouterConfig>,
}

/// OpenRouter-specific configuration, allowing users to control routing and pricing metadata.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, TS)]
#[serde(default)]
pub struct OpenRouterConfig {
    /// Provider-level routing preferences forwarded to OpenRouter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<OpenRouterProviderConfig>,

    /// Optional `route` payload forwarded as-is to OpenRouter for advanced routing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<Value>,

    /// Additional top-level fields that may be forwarded to OpenRouter as the API evolves.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Provider routing preferences supported by OpenRouter.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, TS)]
#[serde(default)]
pub struct OpenRouterProviderConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_fallbacks: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_parameters: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_collection: Option<OpenRouterDataCollectionPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zdr: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub only: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantizations: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<OpenRouterProviderSort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_price: Option<OpenRouterMaxPrice>,

    /// Catch-all for additional provider keys so new OpenRouter features do not break deserialization.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, TS)]
#[serde(rename_all = "lowercase")]
pub enum OpenRouterDataCollectionPolicy {
    Allow,
    Deny,
}

impl Default for OpenRouterDataCollectionPolicy {
    fn default() -> Self {
        OpenRouterDataCollectionPolicy::Allow
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, TS)]
#[serde(rename_all = "lowercase")]
pub enum OpenRouterProviderSort {
    Price,
    Throughput,
    Latency,
}

impl Default for OpenRouterProviderSort {
    fn default() -> Self {
        OpenRouterProviderSort::Price
    }
}

/// `max_price` envelope for OpenRouter provider routing controls.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, TS)]
#[serde(default)]
pub struct OpenRouterMaxPrice {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<f64>,

    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
