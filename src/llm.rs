//! Chat completions through OpenRouter. `model` can be any model slug or a
//! preset (`@preset/<slug>`), so the model can be swapped from the dashboard
//! or the environment.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub struct Llm {
    http: reqwest::Client,
    /// Full URL of the chat completions endpoint (`LLM_ENDPOINT`).
    url: String,
    api_key: String,
    model: String,
    /// `reasoning.effort`, e.g. "low".
    reasoning_effort: String,
}

#[derive(Serialize)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
}

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    /// Routes requests of one conversation to the same server, for warm caches.
    session_id: &'a str,
    /// Anthropic models need caching requested explicitly; other providers
    /// cache automatically and ignore this.
    cache_control: CacheControl,
    reasoning: Reasoning<'a>,
}

#[derive(Serialize)]
struct CacheControl {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Serialize)]
struct Reasoning<'a> {
    effort: &'a str,
}

#[derive(Deserialize)]
struct Response {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Reply,
}

#[derive(Deserialize)]
struct Reply {
    content: Option<String>,
}

impl Llm {
    pub fn new(
        http: reqwest::Client,
        url: String,
        api_key: String,
        model: String,
        reasoning_effort: String,
    ) -> Self {
        Self {
            http,
            url,
            api_key,
            model,
            reasoning_effort,
        }
    }

    pub async fn complete(&self, session_id: &str, messages: &[ChatMessage]) -> Result<String> {
        let response = self
            .http
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .json(&Request {
                model: &self.model,
                messages,
                session_id,
                cache_control: CacheControl { kind: "ephemeral" },
                reasoning: Reasoning {
                    effort: &self.reasoning_effort,
                },
            })
            .send()
            .await
            .context("sending chat request")?;

        let status = response.status();
        let body = response.text().await.context("reading chat response")?;
        if !status.is_success() {
            bail!("chat request failed ({status}): {body}");
        }

        let parsed: Response = serde_json::from_str(&body)
            .with_context(|| format!("unexpected chat response: {body}"))?;
        parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .context("chat response had no content")
    }
}
