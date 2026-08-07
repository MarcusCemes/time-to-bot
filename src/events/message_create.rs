//! Message handler: the core trigger for the LLM layer.
//!
//! Every message is appended to the channel's history deque (the bot's memory),
//! whether or not it is interested. A mention OR active interest window enters
//! (or refreshes) the interest window and triggers a single-flight LLM turn
//! (Kimi #2 — the bot joins the conversation for a while, not just on mention).

use serenity::all::{Context, Message};
use tracing::log::*;

use crate::llm::channel_manager::{ChannelManager, HistoryEntry};
use crate::llm::turn;

pub async fn handle_message_create(ctx: Context, new_message: Message) {
    // Ignore the bot's own messages and system messages.
    if new_message.author.bot {
        return;
    }

    let manager = ctx
        .data
        .read()
        .await
        .get::<ChannelManager>()
        .cloned()
        .expect("ChannelManager missing from context");

    let channel_id = new_message.channel_id;
    let author = new_message.author.name.clone();
    let content = new_message.content.clone();
    let message_id = new_message.id;

    // 1. Always append to history (the deque is the bot's memory).
    //    Explicit mention check (Kimi #12): don't trust mentions_me's guild-cache
    //    dependency silently; log errors instead of swallowing them.
    let mentioned = match new_message.mentions_me(&ctx).await {
        Ok(m) => m,
        Err(e) => {
            warn!("mentions_me failed: {e}");
            false
        }
    };
    let interested = {
        let state_arc = manager.get_or_create(channel_id).await;
        let mut state = state_arc.lock().await;
        state.push_message(
            HistoryEntry {
                seq: 0, // assigned by push_message
                author,
                content: content.clone(),
                message_id,
            },
            false, // user-authored
        );
        // 2. Enter/refresh interest: a mention wakes the bot; any message while
        //    already interested extends the window (keeps it awake in an active convo).
        if mentioned || state.is_interested() {
            state.interest(manager.interest_timeout());
        }
        state.is_interested()
    };

    // 3. Trigger a turn if interested (single-flight handled inside turn.rs).
    if interested {
        turn::trigger_turn(&ctx, manager, channel_id).await;
    }
}
