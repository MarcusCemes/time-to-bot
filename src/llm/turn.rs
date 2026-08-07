//! Shared LLM turn orchestration: single-flight trigger, panic-safe in-flight
//! flag, continuation after turn completion, and the turn itself.
//!
//! Consolidated here so both `message_create` (mentions) and `interaction_create`
//! (`/summon`) use the same trigger logic (Kimi #19).

use std::collections::HashMap;
use std::sync::Arc;

use serenity::all::{ChannelId, Context, Http, ReactionType};
use tokio::sync::mpsc;
use tracing::log::*;

use crate::llm::agent::LlmAgent;
use crate::llm::channel_manager::{ChannelManager, HistoryEntry};
use crate::llm::executor;
use crate::llm::prompt::{format_history_line, SUMMON_SUFFIX};
use crate::llm::tools::Action;

/// RAII guard that clears the channel's `generation_in_flight` flag on drop,
/// even if the spawned turn task panics (Kimi #4). On the normal completion
/// path the flag is cleared atomically with the continuation decision (Kimi
/// round-4 #1) and the guard is disarmed; Drop is the panic-only path.
pub struct InFlightGuard {
    manager: ChannelManager,
    channel_id: ChannelId,
    cleared: bool,
}

impl InFlightGuard {
    /// Arm the channel's turn as in-flight. Assumes the flag was already set
    /// by `spawn_turn`'s single-flight critical section (Kimi round-4 #3 —
    /// TOCTOU fix: check-and-set happens in ONE locked section).
    pub fn new(manager: &ChannelManager, channel_id: ChannelId) -> Self {
        Self {
            manager: manager.clone(),
            channel_id,
            cleared: false,
        }
    }

    /// Mark the flag as already-cleared (the completion path did it).
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
        // Panic-only path: clear the flag so the channel isn't wedged. Runs
        // during runtime shutdown too, where tokio::spawn would panic — use
        // Handle::try_current to stay safe (Kimi round-4 #13).
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let state_arc = manager.get_or_create(channel_id).await;
                let mut state = state_arc.lock().await;
                state.generation_in_flight = false;
            });
        }
    }
}

/// Public entry from event handlers: trigger an LLM turn for a channel
/// (single-flight: at most one in-flight). If a turn is already running, this
/// returns without spawning — the running turn's continuation logic picks up
/// new messages (Kimi #2/#3).
pub async fn trigger_turn(ctx: &Context, manager: ChannelManager, channel_id: ChannelId) {
    let http = ctx.http.clone();
    let agent = ctx
        .data
        .read()
        .await
        .get::<LlmAgent>()
        .cloned()
        .expect("LlmAgent missing from context");
    spawn_turn(http, agent, manager, channel_id).await;
}

/// Core single-flight trigger. Shared so the continuation path (inside
/// `llm_turn`) can re-trigger without needing a full `Context` (whose
/// constructor is private in serenity 0.12).
async fn spawn_turn(
    http: Arc<Http>,
    agent: LlmAgent,
    manager: ChannelManager,
    channel_id: ChannelId,
) {
    // Single-flight check-and-set in ONE locked critical section (Kimi round-4
    // #3 — TOCTOU: check and set must not span tasks).
    {
        let state_arc = manager.get_or_create(channel_id).await;
        let mut state = state_arc.lock().await;
        if state.generation_in_flight {
            debug!("turn already in flight for {channel_id}");
            return;
        }
        state.generation_in_flight = true;
    }

    // Plain tokio::spawn — the future is Send (probe-verified; the earlier
    // non-Send was a recursive opaque-type artifact, see spawn_continuation).
    let turn_future = run_turn(http, agent, manager, channel_id);
    tokio::spawn(turn_future);
}

/// The actual turn body: guard + llm_turn + continuation. Spawned directly via
/// `tokio::spawn` (the future is Send); the continuation is spawned via the
/// separate `spawn_continuation` fn to keep opaque types finite.
///
/// Compile-time Send pin (Kimi round-4 #20): if this fails, a dependency
/// (e.g. a rig upgrade) re-broke Send-ness and we'd know at build time.
#[allow(dead_code)]
fn _assert_run_turn_send(
    http: Arc<Http>,
    agent: LlmAgent,
    manager: ChannelManager,
    channel_id: ChannelId,
) -> impl std::future::Future<Output = ()> + Send {
    run_turn(http, agent, manager, channel_id)
}
async fn run_turn(
    http: Arc<Http>,
    agent: LlmAgent,
    manager: ChannelManager,
    channel_id: ChannelId,
) {
    // The flag was armed by spawn_turn's critical section; the guard's Drop
    // is the panic-only path.
    let mut guard = InFlightGuard::new(&manager, channel_id);

    // Snapshot the seq BEFORE the turn so continuation can detect growth.
    let snapshot_seq = {
        let state_arc = manager.get_or_create(channel_id).await;
        let state = state_arc.lock().await;
        state.next_seq
    };
    // Capture the generation epoch at turn START (Kimi round-4 #4 — must be
    // before agent.turn, not after, or a mid-thinking /reset slips through).
    let generation = {
        let state_arc = manager.get_or_create(channel_id).await;
        let state = state_arc.lock().await;
        state.generation
    };

    let result = llm_turn(&http, &manager, channel_id, generation, &agent).await;
    if result.is_err() {
        error!("LLM turn failed for {channel_id}: {:?}", result.as_ref().err());
    }

    // Clear the flag atomically with the continuation decision (Kimi round-4
    // #1): if we continue, the flag stays armed and is handed to the next turn;
    // otherwise clear it here. Keep the Drop impl as the panic-only path.
    let continue_turn = {
        let state_arc = manager.get_or_create(channel_id).await;
        let mut state = state_arc.lock().await;
        // Only continue if the turn produced no error, the channel is still
        // interested, and USER-authored history grew while we were busy
        // (Kimi round-4 #2 — the bot's own output must NOT count as growth,
        // or we get a self-sustaining reply loop).
        let cont = result.is_ok()
            && state.is_interested()
            && (state.summoned || state.last_external_seq > snapshot_seq);
        if !cont {
            state.generation_in_flight = false;
        }
        cont
    };
    guard.disarm();

    if continue_turn {
        spawn_continuation(http.clone(), agent, manager.clone(), channel_id);
    }
}

/// Fire-and-forget a continuation turn. Separate async fn so its opaque type
/// doesn't recurse into `run_turn` (which would make the type infinite).
fn spawn_continuation(
    http: Arc<Http>,
    agent: LlmAgent,
    manager: ChannelManager,
    channel_id: ChannelId,
) {
    tokio::task::spawn(async move {
        run_turn(http, agent, manager, channel_id).await;
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
    generation: u64,
    agent: &LlmAgent,
) -> Result<(), anyhow::Error> {
    // Build the LLM prompt (formatted transcript) from the deque.
    // `was_summoned` must live past the block for error-restore (Kimi round-4 #10).
    let (prompt, seq_snapshot, was_summoned) = {
        let state_arc = manager.get_or_create(channel_id).await;
        let mut state = state_arc.lock().await;

        // Format the transcript as `#N author: content` lines (last 40 — Kimi #21).
        let window: Vec<&HistoryEntry> = state
            .history
            .iter()
            .rev()
            .take(40)
            .collect();
        let lines: Vec<String> = window
            .iter()
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
        let was_summoned = state.summoned;
        if was_summoned {
            prompt.push_str(SUMMON_SUFFIX);
        }
        // Snapshot ONLY the 40-entry window the model actually saw (Kimi
        // round-4 #9 — a wider snapshot lets it react to unseen messages).
        let seq_snapshot = std::sync::Arc::new(
            window
                .iter()
                .map(|e| (e.seq, e.message_id))
                .collect::<HashMap<_, _>>(),
        );
        drop(window); // release the immutable borrow before mutating state
        if was_summoned {
            state.summoned = false; // consumed only on successful use (restored on error)
        }
        (prompt, seq_snapshot, was_summoned)
    };

    // Channel for tool actions.
    let (tx, mut rx) = mpsc::channel::<Action>(32);

    // Run the agent turn (with retry/backoff — Kimi #24).
    let turn_result = agent
        .turn(&prompt, tx.clone(), seq_snapshot.clone())
        .await;

    // If the turn failed, restore the summoned flag so the entrance isn't lost
    // (Kimi round-4 #10 — consume only on successful use). Idempotent even if
    // a fresh /summon raced in.
    if turn_result.is_err() && was_summoned {
        let state_arc = manager.get_or_create(channel_id).await;
        let mut state = state_arc.lock().await;
        state.summoned = true;
    }
    turn_result?;

    // Drain the actions the tools enqueued. Drop our tx clone first so the
    // receiver sees EOF when rig's senders are gone (Kimi round-4 #19).
    drop(tx);
    let mut actions: Vec<Action> = Vec::new();
    while let Ok(action) = rx.try_recv() {
        actions.push(action);
    }

    // Execute the actions: route sends to the executor queue, reactions through
    // the executor too (Kimi #16 — delay + cap, no instant spam).
    //
    // Dedup (smoke test 2026-08-07): the model sometimes queues the same content
    // multiple times in one burst, or repeats its own previous line. Suppress
    // both — a seen-this-turn set, and the bot's last sent message.
    let mut seen_this_turn = std::collections::HashSet::new();
    let last_bot = {
        let state_arc = manager.get_or_create(channel_id).await;
        let state = state_arc.lock().await;
        state.last_bot_message()
    };
    for action in actions {
        match action {
            Action::Send(content) => {
                // Skip empty content (Kimi round-4 #19 — Discord 400).
                let trimmed = content.trim().to_string();
                if trimmed.is_empty() {
                    warn!("skipping empty LLM message");
                    continue;
                }
                // Dedup: skip if sent this turn or repeats the bot's last line.
                if !seen_this_turn.insert(trimmed.clone()) {
                    info!("dedup: skipping repeated content this turn");
                    continue;
                }
                if last_bot.as_deref() == Some(trimmed.as_str()) {
                    info!("dedup: skipping repeat of bot's last message");
                    continue;
                }
                info!("LLM queued a message ({} chars)", trimmed.chars().count());
                manager
                    .get_or_create(channel_id)
                    .await
                    .lock()
                    .await
                    .enqueue(trimmed);
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

    // Execute the send queue (executor drains + sends with typing). The
    // generation epoch was captured at turn start; the executor re-checks it
    // after each delay so a mid-sleep /reset aborts the stale burst
    // (Kimi #15, round-4 #5). Executor is infallible (Kimi round-4 #19).
    executor::execute_output(http, channel_id, manager, generation).await;

    Ok(())
}
