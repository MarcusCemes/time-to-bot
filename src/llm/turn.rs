//! Shared LLM turn orchestration: single-flight trigger, panic-safe in-flight
//! flag, continuation after turn completion, and the turn itself.
//!
//! Consolidated here so both `message_create` (mentions) and `interaction_create`
//! (`/summon`) use the same trigger logic (Kimi #19).

use std::sync::Arc;

use serenity::all::{ChannelId, Context, Http, ReactionType};
use tokio::sync::mpsc;
use tracing::log::*;

use crate::llm::agent::LlmAgent;
use crate::llm::channel_manager::ChannelManager;
use crate::llm::executor;
use crate::llm::prompt::{format_history_line, SUMMON_SUFFIX};
use crate::llm::tools::Action;

/// RAII guard that clears the channel's `generation_in_flight` flag on drop,
/// even if the spawned turn task panics (Kimi #4).
pub struct InFlightGuard {
    manager: ChannelManager,
    channel_id: ChannelId,
    cleared: bool,
}

impl InFlightGuard {
    /// Mark the channel's turn as in-flight and return a guard that clears it.
    pub async fn new(manager: &ChannelManager, channel_id: ChannelId) -> Self {
        let state_arc = manager.get_or_create(channel_id).await;
        let mut state = state_arc.lock().await;
        state.generation_in_flight = true;
        Self {
            manager: manager.clone(),
            channel_id,
            cleared: false,
        }
    }

    /// Mark the flag as already-cleared (avoids double-clear after a successful turn).
    pub fn disarm(&mut self) {
        self.cleared = true;
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        if self.cleared {
            return;
        }
        let manager = self.manager.clone();
        let channel_id = self.channel_id;
        // Best-effort clear; this runs on panic paths too (the tokio runtime
        // is still up when the task is dropped).
        tokio::spawn(async move {
            let state_arc = manager.get_or_create(channel_id).await;
            let mut state = state_arc.lock().await;
            state.generation_in_flight = false;
        });
    }
}

/// Public entry from event handlers: trigger an LLM turn for a channel
/// (single-flight: at most one in-flight). If a turn is already running, this
/// returns without spawning — the running turn's continuation logic picks up
/// new messages (Kimi #2/#3).
pub async fn trigger_turn(ctx: &Context, manager: ChannelManager, channel_id: ChannelId) {
    let http = ctx.http.clone();
    spawn_turn(http, manager, channel_id).await;
}

/// Core single-flight trigger. Shared so the continuation path (inside
/// `llm_turn`) can re-trigger without needing a full `Context` (whose
/// constructor is private in serenity 0.12).
async fn spawn_turn(http: Arc<Http>, manager: ChannelManager, channel_id: ChannelId) {
    // Single-flight check: skip if a turn is already in flight.
    {
        let state_arc = manager.get_or_create(channel_id).await;
        let state = state_arc.lock().await;
        if state.generation_in_flight {
            debug!("turn already in flight for {channel_id}");
            return;
        }
    }

    // Plain tokio::spawn — the future is Send (probe-verified; the earlier
    // non-Send was a recursive opaque-type artifact, see spawn_continuation).
    let turn_future = run_turn(http, manager, channel_id);
    tokio::spawn(turn_future);
}

/// The actual turn body: guard + llm_turn + continuation. Spawned directly via
/// `tokio::spawn` (the future is Send); the continuation is spawned via the
/// separate `spawn_continuation` fn to keep opaque types finite.
async fn run_turn(http: Arc<Http>, manager: ChannelManager, channel_id: ChannelId) {
    // InFlightGuard clears the flag on drop (panic-safe) and marks in-flight
    // for the duration of the turn.
    let mut guard = InFlightGuard::new(&manager, channel_id).await;

    // Snapshot the seq BEFORE the turn so continuation can detect growth.
    let snapshot_seq = {
        let state_arc = manager.get_or_create(channel_id).await;
        let state = state_arc.lock().await;
        state.next_seq
    };

    let result = llm_turn(&http, &manager, channel_id).await;
    if let Err(e) = result {
        error!("LLM turn failed for {channel_id}: {e}");
    }

    guard.disarm();

    // Continuation (Kimi #3): if still interested AND history grew during the
    // turn, run another turn to respond to messages that arrived while we were
    // busy. Spawn a fresh task (not await) — spawn_turn's future is not Send.
    let history_grew = {
        let state_arc = manager.get_or_create(channel_id).await;
        let state = state_arc.lock().await;
        state.is_interested() && state.next_seq > snapshot_seq
    };
    if history_grew {
        spawn_continuation(http.clone(), manager.clone(), channel_id);
    }
}

/// Fire-and-forget a continuation turn. Separate async fn so its opaque type
/// doesn't recurse into `run_turn` (which would make the type infinite).
fn spawn_continuation(http: Arc<Http>, manager: ChannelManager, channel_id: ChannelId) {
    tokio::task::spawn(async move {
        run_turn(http, manager, channel_id).await;
    });
}

/// Run one full LLM turn: build history, call the agent, drain actions to the
/// executor. After the turn, if still interested AND history grew during the
/// turn, trigger a continuation turn (Kimi #3 — "history diff will kick the
/// next turn").
pub async fn llm_turn(
    http: &Arc<Http>,
    manager: &ChannelManager,
    channel_id: ChannelId,
) -> Result<(), anyhow::Error> {
    let config = crate::config::Config::from_env();
    let agent = LlmAgent::new(&config);

    // Build the LLM prompt (formatted transcript) from the deque.
    let (prompt, seq_snapshot) = {
        let state_arc = manager.get_or_create(channel_id).await;
        let mut state = state_arc.lock().await;

        // Format the transcript as `#N author: content` lines (last 40 — Kimi #21).
        let lines: Vec<String> = state
            .history
            .iter()
            .rev()
            .take(40)
            .rev()
            .map(|e| format_history_line(e.seq, &e.author, &e.content))
            .collect();
        let mut prompt = lines.join("\n");
        // Empty-transcript guard (Kimi #30): never send an empty prompt.
        if prompt.is_empty() {
            prompt.push_str("(no prior messages)");
        }
        // If summoned, append the amusing entrance (consume the flag ONLY on
        // successful use — Kimi #14).
        if state.summoned {
            prompt.push_str(SUMMON_SUFFIX);
            state.summoned = false;
        }
        let seq_snapshot = std::sync::Arc::new(state.seq_snapshot());
        (prompt, seq_snapshot)
    };

    // Channel for tool actions.
    let (tx, mut rx) = mpsc::channel::<Action>(32);

    // Run the agent turn (with retry/backoff — Kimi #24).
    agent
        .turn(&prompt, tx.clone(), seq_snapshot.clone())
        .await?;

    // Drain the actions the tools enqueued.
    let mut actions: Vec<Action> = Vec::new();
    while let Ok(action) = rx.try_recv() {
        actions.push(action);
    }

    // Execute the actions: route sends to the executor queue, reactions through
    // the executor too (Kimi #16 — delay + cap, no instant spam).
    for action in actions {
        match action {
            Action::Send(content) => {
                manager
                    .get_or_create(channel_id)
                    .await
                    .lock()
                    .await
                    .enqueue(content);
            }
            Action::React { message_id, emoji } => {
                // Parse the emoji defensively (Kimi #18 — trust nothing from the LLM).
                match emoji.parse::<ReactionType>() {
                    Ok(reaction_type) => {
                        manager
                            .get_or_create(channel_id)
                            .await
                            .lock()
                            .await
                            .enqueue_reaction(message_id, reaction_type);
                    }
                    Err(e) => warn!("invalid reaction emoji {emoji:?}: {e}"),
                }
            }
        }
    }

    // Execute the send queue (executor drains + sends with typing). Pass the
    // generation epoch so a mid-turn `/reset` aborts the stale burst (Kimi #15).
    let generation = {
        let state_arc = manager.get_or_create(channel_id).await;
        let state = state_arc.lock().await;
        state.generation
    };
    executor::execute_output(http, channel_id, manager, generation).await?;

    Ok(())
}
