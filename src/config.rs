//! Environment configuration for the LLM layer.

/// Configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// OpenRouter API key (`OPENROUTER_API_KEY`).
    pub openrouter_api_key: String,
    /// Optional OpenRouter base URL override (`OPENROUTER_BASE_URL`).
    pub openrouter_base_url: Option<String>,
    /// Model to use (`MODEL`), default `google/gemma-4-31b-it`.
    pub model: String,
    /// Interest window duration in minutes (`INTEREST_TIMEOUT_MINUTES`), default 15.
    pub interest_timeout: std::time::Duration,
}

impl Config {
    /// Build config from environment variables, applying defaults.
    pub fn from_env() -> Self {
        let openrouter_api_key =
            std::env::var("OPENROUTER_API_KEY").expect("Must provide OPENROUTER_API_KEY");
        let model = std::env::var("MODEL").unwrap_or_else(|_| "google/gemma-4-31b-it".to_string());
        let interest_minutes: u64 = std::env::var("INTEREST_TIMEOUT_MINUTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(15);
        Self {
            openrouter_api_key,
            openrouter_base_url: std::env::var("OPENROUTER_BASE_URL").ok(),
            model,
            interest_timeout: std::time::Duration::from_secs(interest_minutes.saturating_mul(60)),
        }
    }
}
