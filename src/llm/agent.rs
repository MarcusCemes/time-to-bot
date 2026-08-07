//! Rig agent: builds the OpenRouter client + agent with tools, runs the chat
//! loop with history, and returns the actions the tools enqueued.
//!
//! rig 0.41 split: `Agent` (config) + `AgentRunner` (execution). The runner is
//! where `tool_context` (injected tool state) and `run()` live. Verified against
//! rig-agent 0.41 source.

use anyhow::Context as _;
use rig::client::AgentClientExt;
use rig::providers::openrouter;

use crate::config::Config;
use crate::llm::prompt::SYSTEM_PROMPT;
use crate::llm::tools::{ActionSender, React, SendMessage, SeqSnapshot};

/// The LLM agent for one channel's conversation.
#[derive(Clone)]
pub struct LlmAgent {
    client: openrouter::Client,
    model: String,
}

impl serenity::prelude::TypeMapKey for LlmAgent {
    type Value = LlmAgent;
}

impl LlmAgent {
    pub fn new(config: &Config) -> Self {
        let client = if let Some(base_url) = &config.openrouter_base_url {
            openrouter::Client::builder()
                .base_url(base_url)
                .api_key(&config.openrouter_api_key)
                .build()
                .expect("Failed to build OpenRouter client")
        } else {
            openrouter::Client::new(&config.openrouter_api_key)
                .expect("Failed to build OpenRouter client")
        };
        Self {
            client,
            model: config.model.clone(),
        }
    }

    /// Run one LLM turn for a channel with the given transcript prompt, tools
    /// wired to `queue_tx` (actions the model decides) and `seq_snapshot`
    /// (seq→MessageId). The tools push `Action`s into `queue_tx` during the
    /// loop; the caller drains the receiver side (executor owns the queue).
    ///
    /// Retries transient failures (429/5xx) with exponential backoff (Kimi #24).
    /// On each retry the caller drains any actions the failed attempt already
    /// enqueued, so a retry can't duplicate tool effects (Kimi round-4 #8).
    pub async fn turn(
        &self,
        prompt: &str,
        queue_tx: ActionSender,
        seq_snapshot: SeqSnapshot,
    ) -> Result<(), anyhow::Error> {
        let agent = self
            .client
            .agent(&self.model)
            .preamble(SYSTEM_PROMPT)
            .tool(SendMessage)
            .tool(React)
            .build();

        // Runner holds the injected tool context + runs the loop.
        let mut context = rig::tool::ToolContext::new();
        context.insert(queue_tx.clone());
        context.insert(seq_snapshot.clone());

        // Exponential backoff on transient errors: 200ms, 1s, 3s (Kimi #24).
        // `AgentRunner::run` consumes the runner, so rebuild it per attempt —
        // it's just config assembly around the shared `&agent`.
        let delays_ms = [200, 1000, 3000];
        let mut last_err = None;
        for (i, delay) in delays_ms.iter().enumerate() {
            let runner = rig::agent::AgentRunner::from_agent(&agent, prompt)
                // MaxTurnsError (default max_turns=1) killed the smoke test:
                // the model's FIRST call is a tool call, so the loop needs
                // turns for tool round-trips + final text (Kimi #24).
                .max_turns(4)
                .tool_context(context.clone());
            match runner.run().await {
                Ok(_response) => return Ok(()),
                Err(e) => {
                    // Empty-response ("Response contained no message or tool
                    // call") is the model saying "nothing to add" — NOT a
                    // transient error. Retry once (model hiccup), then treat
                    // as a graceful quiet, not a failure (smoke-test log).
                    if is_empty_response(&e) {
                        if i == 0 {
                            last_err = Some(e);
                            continue; // one retry
                        }
                        // Second occurrence: quiet, no error.
                        return Ok(());
                    }
                    last_err = Some(e);
                    // Don't sleep after the FINAL attempt (Kimi round-4 #8).
                    if i + 1 < delays_ms.len() {
                        tokio::time::sleep(std::time::Duration::from_millis(*delay)).await;
                    }
                }
            }
        }
        Err(anyhow::Error::from(last_err.expect("delays non-empty")))
            .context("LLM chat failed after retries")
    }
}

/// True if the error is the "empty response" class (model returned neither
/// text nor tool call). rig 0.41 surfaces it as `PromptError::CompletionError(
/// CompletionError::ResponseError("Response contained no message or tool call"))`
/// (verified in rig-agent completion.rs + rig-core providers). The model
/// effectively said "nothing to add."
fn is_empty_response(err: &rig::completion::PromptError) -> bool {
    matches!(
        err,
        rig::completion::PromptError::CompletionError(rig::completion::CompletionError::ResponseError(msg))
            if msg.contains("Response contained no message or tool call")
    )
}
