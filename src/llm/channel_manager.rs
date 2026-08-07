//! Per-channel state: interest window, history deque, seq→MessageId map,
//! single-flight flag. All shared state lives behind one `Arc<Mutex<HashMap>>`
//! (GLM BLOCKER #1: spawned tasks get the Arc, not a state clone).

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use serenity::all::{ChannelId, MessageId, ReactionType};
use serenity::prelude::TypeMapKey;
use tokio::sync::Mutex;

use crate::llm::tools::QueuedMessage;

/// Maximum history entries per channel before eviction (drop oldest 1–5).
pub const MAX_HISTORY: usize = 100;
/// Maximum burst of queued messages per turn.
pub const MAX_BURST: usize = 5;
/// Maximum length of a single queued message (Discord hard limit 2000; cap at
/// 1900 for safety margin — Kimi #11).
pub const MAX_MESSAGE_LEN: usize = 1900;

/// One transcript entry, LLM-visible with a stable sequential id.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub seq: u64,
    pub author: String,
    pub content: String,
    pub message_id: MessageId,
}

#[derive(Debug)]
pub struct ChannelState {
    pub interested_until: Option<Instant>,
    pub history: VecDeque<HistoryEntry>,
    pub next_seq: u64,
    /// Highest seq contributed by a USER message (not bot output). Continuation
    /// uses this to detect "history grew" without counting the bot's own
    /// replies (Kimi round-4 #2 — prevents self-sustaining reply loop).
    pub last_external_seq: u64,
    pub seq_to_msg: HashMap<u64, MessageId>,
    pub send_queue: VecDeque<QueuedMessage>,
    pub generation_in_flight: bool,
    /// Epoch counter bumped on every `/reset`. Captured at turn start; if it
    /// changes mid-turn, the executor bails (Kimi #15 — in-flight turn must
    /// not survive a reset).
    pub generation: u64,
    pub summoned: bool,
}

impl ChannelState {
    fn new() -> Self {
        Self {
            interested_until: None,
            history: VecDeque::new(),
            next_seq: 1,
            last_external_seq: 0,
            seq_to_msg: HashMap::new(),
            send_queue: VecDeque::new(),
            generation_in_flight: false,
            generation: 0,
            summoned: false,
        }
    }

    /// Enter (or refresh) the interest window.
    pub fn interest(&mut self, timeout: std::time::Duration) {
        self.interested_until = Some(Instant::now() + timeout);
    }

    /// Whether the interest window is currently active.
    pub fn is_interested(&self) -> bool {
        self.interested_until.is_some_and(|t| t > Instant::now())
    }

    /// Append a message to history (always — the deque is the bot's memory).
    /// `is_bot` marks the bot's own output so continuation can distinguish
    /// user-authored growth (Kimi round-4 #2).
    pub fn push_message(&mut self, entry: HistoryEntry, is_bot: bool) {
        let seq = self.next_seq;
        self.next_seq += 1;
        let mut entry = entry;
        entry.seq = seq;
        if !is_bot {
            self.last_external_seq = seq;
        }
        self.seq_to_msg.insert(seq, entry.message_id);
        self.history.push_back(entry);
        self.evict();
    }

    /// Append the bot's own sent reply to history (self-messages are ignored by
    /// serenity, so the LLM must see its own output — GLM BLOCKER #2). Takes the
    /// real message id so reactions to bot lines resolve (Kimi #17).
    pub fn push_bot_message(&mut self, content: &str, message_id: MessageId) {
        self.push_message(
            HistoryEntry {
                seq: 0, // overwritten by push_message
                author: "Bot".to_string(),
                content: content.to_string(),
                message_id,
            },
            true,
        );
    }

    /// Content of the most recent bot message in history, if any. Used to
    /// suppress the model repeating its own last line (smoke test 2026-08-07 —
    /// the bot sometimes re-sends the same content across turns).
    pub fn last_bot_message(&self) -> Option<String> {
        self.history
            .iter()
            .rev()
            .find(|e| e.author == "Bot")
            .map(|e| e.content.clone())
    }

    /// Drop oldest 1–5 entries when over the cap; prune the seq map (GLM MINOR #8).
    fn evict(&mut self) {
        while self.history.len() > MAX_HISTORY {
            if let Some(removed) = self.history.pop_front() {
                self.seq_to_msg.remove(&removed.seq);
            }
        }
    }

    /// Clear context (for `/reset`). Bumps the generation epoch so any in-flight
    /// turn aborts its send loop (Kimi #15); resets `summoned` (Kimi #14) and
    /// clears `generation_in_flight` so `/reset` can't wedge a channel (Kimi
    /// round-4 #1).
    pub fn clear_context(&mut self) {
        self.history.clear();
        self.seq_to_msg.clear();
        self.send_queue.clear();
        self.next_seq = 1;
        self.last_external_seq = 0;
        self.interested_until = None;
        self.generation += 1;
        self.generation_in_flight = false;
        self.summoned = false;
    }

    /// Queue a message for the executor, capped at MAX_BURST and truncated to
    /// MAX_MESSAGE_LEN (Kimi #11 — Discord 2000-char limit).
    pub fn enqueue(&mut self, content: String) {
        if self.send_queue.len() >= MAX_BURST {
            tracing::warn!("burst cap reached ({MAX_BURST}), dropping extra message");
            return;
        }
        let content: String = content.chars().take(MAX_MESSAGE_LEN).collect();
        let chars = content.chars().count() as u64;
        // Code-computed delay + typing: base 500ms + 30ms/char, capped at 3s
        // (Kimi #23 — cap is 3s total, not 3.5s).
        let delay_ms = (500 + 30 * chars).min(3000);
        self.send_queue.push_back(QueuedMessage {
            content,
            delay_ms,
            reaction: None,
        });
    }

    /// Queue a reaction for the executor (Kimi #16 — reactions go through the
    /// executor like sends, with delay + cap).
    pub fn enqueue_reaction(&mut self, message_id: MessageId, reaction_type: ReactionType) {
        if self.send_queue.len() >= MAX_BURST {
            tracing::warn!("burst cap reached ({MAX_BURST}), dropping extra reaction");
            return;
        }
        // Reactions get a short, fixed delay (200ms) — enough to feel natural.
        self.send_queue.push_back(QueuedMessage {
            content: String::new(), // reactions don't carry content
            delay_ms: 200,
            reaction: Some((message_id, reaction_type)),
        });
    }

    /// Drain the send queue.
    pub fn drain_queue(&mut self) -> Vec<QueuedMessage> {
        self.send_queue.drain(..).collect()
    }
}

/// The shared per-channel state manager.
#[derive(Clone)]
pub struct ChannelManager {
    inner: Arc<Mutex<HashMap<ChannelId, Arc<Mutex<ChannelState>>>>>,
    interest_timeout: std::time::Duration,
}

impl TypeMapKey for ChannelManager {
    type Value = ChannelManager;
}

impl ChannelManager {
    pub fn new(interest_timeout: std::time::Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            interest_timeout,
        }
    }

    pub fn interest_timeout(&self) -> std::time::Duration {
        self.interest_timeout
    }

    /// Get-or-create the per-channel state mutex.
    pub async fn get_or_create(&self, channel: ChannelId) -> Arc<Mutex<ChannelState>> {
        let mut map = self.inner.lock().await;
        map.entry(channel)
            .or_insert_with(|| Arc::new(Mutex::new(ChannelState::new())))
            .clone()
    }
}
