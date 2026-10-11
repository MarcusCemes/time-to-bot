//! Runtime configuration, read from environment variables (`.env` is loaded in `main`).

use std::{env, path::PathBuf, str::FromStr, time::Duration};

use anyhow::{Context, Result};

pub struct Config {
    pub discord_token: String,
    pub openrouter_api_key: String,
    /// Chat completions endpoint for replies (e.g. an EU host for data residency).
    pub llm_endpoint: String,
    /// Model that writes replies: any OpenRouter model slug, or `@preset/<slug>`.
    pub llm_model: String,
    /// `reasoning.effort` sent with reply requests.
    pub llm_reasoning_effort: String,
    /// Decisions endpoint for the gate. Configured separately from the chat
    /// endpoint: decision models aren't necessarily served from the same hosts.
    pub decision_endpoint: String,
    /// Decision model used by the gate (see `decision.rs`).
    pub decision_model: String,
    pub persona_path: PathBuf,
    /// Messages kept per channel, and shown to the models.
    pub history_len: usize,
    /// The bot goes back to sleep if it has not taken part for this long.
    pub idle_timeout: Duration,
    /// The bot goes back to sleep after this many replies in one session.
    pub max_turns: u32,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            discord_token: required("DISCORD_TOKEN")?,
            openrouter_api_key: required("OPENROUTER_API_KEY")?,
            llm_endpoint: optional(
                "LLM_ENDPOINT",
                "https://openrouter.ai/api/v1/chat/completions".to_string(),
            )?,
            llm_model: required("LLM_MODEL")?,
            llm_reasoning_effort: optional("LLM_REASONING_EFFORT", "low".to_string())?,
            decision_endpoint: optional(
                "DECISION_ENDPOINT",
                "https://openrouter.ai/api/alpha/decisions".to_string(),
            )?,
            decision_model: optional("DECISION_MODEL", "typesafe/jev-1.13".to_string())?,
            persona_path: optional("PERSONA_PATH", PathBuf::from("config/persona.md"))?,
            history_len: optional("HISTORY_LEN", 30)?,
            idle_timeout: Duration::from_secs(optional("IDLE_TIMEOUT_SECS", 300)?),
            max_turns: optional("MAX_TURNS", 20)?,
        })
    }
}

fn required(key: &str) -> Result<String> {
    env::var(key).with_context(|| format!("missing environment variable {key}"))
}

fn optional<T: FromStr>(key: &str, default: T) -> Result<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match env::var(key) {
        Ok(value) => value
            .parse()
            .with_context(|| format!("invalid value for {key}")),
        Err(_) => Ok(default),
    }
}
