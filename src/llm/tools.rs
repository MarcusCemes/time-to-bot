//! Rig tools the LLM can call: send a message, react to a message.
//!
//! Tools are unit structs implementing `rig_agent::tool::Tool`. Shared state
//! (the action queue sender, the seq→MessageId snapshot) is injected via
//! `ToolContext` — the canonical rig 0.41 pattern (verified against source).

use std::collections::HashMap;
use std::sync::Arc;

use rig::tool::{Tool, ToolContext, ToolExecutionError, ToolOutput};
use serde::Deserialize;
use serenity::all::{MessageId, ReactionType};
use tokio::sync::mpsc;

/// An action the LLM decided to take, produced by a tool call.
#[derive(Debug, Clone)]
pub enum Action {
    /// Send a message in the channel (executor applies delay/typing/burst cap).
    Send(String),
    /// Add a reaction to a message by resolved message id.
    React { message_id: MessageId, emoji: String },
}

/// A message queued by the LLM, ready for the executor.
#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub content: String,
    pub delay_ms: u64,
    /// Optional reaction to apply instead of sending content (Kimi #16).
    pub reaction: Option<(MessageId, ReactionType)>,
}

/// Sender used by tools to enqueue actions for the executor.
pub type ActionSender = mpsc::Sender<Action>;

/// Snapshot of the seq→MessageId map, owned by the tool execution (GLM #8:
/// the LLM can only react to messages it actually saw in its prompt).
pub type SeqSnapshot = Arc<HashMap<u64, MessageId>>;

/// Args for the `send_message` tool (LLM-facing).
#[derive(Deserialize)]
pub struct SendMessageArgs {
    pub content: String,
}

/// Tool: queue a message to be sent in this channel.
pub struct SendMessage;

impl Tool for SendMessage {
    const NAME: &'static str = "send_message";
    type Args = SendMessageArgs;
    type Output = ToolOutput;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Queue a message to be sent in this channel. Multiple calls queue a burst of messages.".into()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "content": { "type": "string", "description": "The message content to send" }
            },
            "required": ["content"]
        })
    }

    async fn call(
        &self,
        context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let tx = context
            .require::<ActionSender>()
            .map_err(|_| ToolExecutionError::other("send_message sender missing from context"))?;
        tx.send(Action::Send(args.content))
            .await
            .map_err(|e| ToolExecutionError::other(format!("send_message queue full: {e}")))?;
        Ok(ToolOutput::text("queued"))
    }
}

/// Args for the `react` tool (LLM-facing).
#[derive(Deserialize)]
pub struct ReactArgs {
    /// Sequence number `#N` of the message to react to (from the transcript).
    pub target: u64,
    /// Emoji to react with.
    pub emoji: String,
}

/// Tool: add a reaction to a message by its sequence number.
pub struct React;

impl Tool for React {
    const NAME: &'static str = "react";
    type Args = ReactArgs;
    type Output = ToolOutput;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Add a reaction to a message. target is the #N sequence number shown in the transcript.".into()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "target": { "type": "integer", "description": "The #N sequence number of the message" },
                "emoji": { "type": "string", "description": "The emoji to react with" }
            },
            "required": ["target", "emoji"]
        })
    }

    async fn call(
        &self,
        context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let seq_map = context
            .require::<SeqSnapshot>()
            .map_err(|_| ToolExecutionError::other("react seq map missing from context"))?;
        let message_id = seq_map
            .get(&args.target)
            .ok_or_else(|| ToolExecutionError::not_found(format!("no message #{} in snapshot", args.target)))?;
        let tx = context
            .require::<ActionSender>()
            .map_err(|_| ToolExecutionError::other("react sender missing from context"))?;
        tx.send(Action::React {
            message_id: *message_id,
            emoji: args.emoji,
        })
        .await
        .map_err(|e| ToolExecutionError::other(format!("react queue full: {e}")))?;
        Ok(ToolOutput::text("reacted"))
    }
}
