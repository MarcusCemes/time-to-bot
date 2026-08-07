//! Output executor: drains the per-channel send queue, applying code-computed
//! delays, RAII typing indicators, burst cap, and bot-history append.
//!
//! Handles both messages (with typing) and reactions (with a short delay) —
//! Kimi #16: all actions go through the executor so nothing fires instantly.

use serenity::all::{ChannelId, CreateAllowedMentions, CreateMessage};
use serenity::http::Typing;
use tracing::log::*;

use crate::llm::channel_manager::ChannelManager;

/// RAII guard that stops the typing indicator on drop (GLM MAJOR #6 — no leak
/// on send failure or panic).
pub struct TypingGuard {
    typing: Option<Typing>,
}

impl TypingGuard {
    pub fn new(typing: Typing) -> Self {
        Self { typing: Some(typing) }
    }
}

impl Drop for TypingGuard {
    fn drop(&mut self) {
        if let Some(t) = self.typing.take() {
            t.stop();
        }
    }
}

/// Execute the queued output for a channel: for each item, show typing for the
/// computed duration, then send (or react), then append the bot's reply to
/// history. One failed send/reaction logs + continues (Kimi #13) instead of
/// aborting the burst.
pub async fn execute_output(
    http: &std::sync::Arc<serenity::http::Http>,
    channel: ChannelId,
    manager: &ChannelManager,
    generation: u64,
) {
    // Drain the queue first (single-flight: executor owns the send).
    let queue = {
        let state_arc = manager.get_or_create(channel).await;
        let mut state = state_arc.lock().await;
        state.drain_queue()
    };

    for item in queue {
        // Handle reactions first (no content, short delay).
        if let Some((message_id, reaction_type)) = item.reaction {
            tokio::time::sleep(std::time::Duration::from_millis(item.delay_ms)).await;
            // Re-check generation AFTER the delay (Kimi round-4 #5): a /reset
            // during the sleep must abort the stale reaction.
            if !generation_current(manager, channel, generation).await {
                debug!("generation changed, aborting stale burst for {channel}");
                break;
            }
            if let Err(e) = channel
                .create_reaction(http, message_id, reaction_type)
                .await
            {
                warn!("react failed: {e}");
            }
            continue;
        }

        // Don't abort the burst if typing fails (403/missing perms): log + continue (GLM #6).
        let _typing_guard = TypingGuard::new(channel.start_typing(http));
        tokio::time::sleep(std::time::Duration::from_millis(item.delay_ms)).await;

        // Re-check generation AFTER the typing delay (Kimi round-4 #5): a /reset
        // during the sleep must abort the stale send.
        if !generation_current(manager, channel, generation).await {
            debug!("generation changed, aborting stale burst for {channel}");
            break;
        }

        match channel
            .send_message(
                http,
                CreateMessage::new()
                    .content(&item.content)
                    .allowed_mentions(CreateAllowedMentions::new()),
            )
            .await
        {
            Ok(message) => {
                // Append the bot's own reply to history with the REAL message id
                // (Kimi #17 — reactions to bot lines resolve).
                let state_arc = manager.get_or_create(channel).await;
                let mut state = state_arc.lock().await;
                state.push_bot_message(&item.content, message.id);
            }
            Err(e) => warn!("send failed: {e}"),
        }
    }
}

/// Whether the channel's generation epoch still matches the turn's. If a
/// `/reset` bumped it, this turn's output is stale.
async fn generation_current(
    manager: &ChannelManager,
    channel: ChannelId,
    generation: u64,
) -> bool {
    let state_arc = manager.get_or_create(channel).await;
    let state = state_arc.lock().await;
    state.generation == generation
}
