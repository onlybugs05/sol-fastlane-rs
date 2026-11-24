use thiserror::Error;

#[derive(Error, Debug)]
pub enum FastlaneError {
    #[error("RPC error on {endpoint}: {message}")]
    Rpc { endpoint: String, message: String },

    #[error("WebSocket error on {endpoint}: {message}")]
    WebSocket { endpoint: String, message: String },

    #[error("Request timed out after {duration_ms}ms on {endpoint}")]
    Timeout { endpoint: String, duration_ms: u64 },

    #[error("No healthy RPC endpoints available in pool")]
    NoHealthyEndpoints,

    #[error("DEX quote error ({dex}): {message}")]
    DexError { dex: String, message: String },

    #[error("Failed to parse response: {0}")]
    ParseError(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("URL parse error: {0}")]
    Url(#[from] url::ParseError),

    #[error("Channel communication error: {0}")]
    Channel(String),
}

pub type Result<T> = std::result::Result<T, FastlaneError>;
