//! In-memory per-channel state: the rolling message history and the awake/asleep session.
//!
//! The history is recorded even while the bot sleeps (it is free), so that a
//! mention wakes the bot with the preceding conversation as context.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub struct Entry {
    pub author: String,
    pub content: String,
    /// True for the bot's own replies.
    pub is_bot: bool,
}

/// Exists only while the bot is awake in a channel.
pub struct Session {
    /// Persona snapshot taken at wake-up.
    pub persona: String,
    pub turns: u32,
    last_active: Instant,
}

pub struct Channel {
    history: VecDeque<Entry>,
    capacity: usize,
    session: Option<Session>,
}

impl Channel {
    pub fn new(capacity: usize) -> Self {
        Self {
            history: VecDeque::with_capacity(capacity),
            capacity,
            session: None,
        }
    }

    pub fn push(&mut self, entry: Entry) {
        if self.history.len() == self.capacity {
            self.history.pop_front();
        }
        self.history.push_back(entry);
    }

    pub fn history(&self) -> impl DoubleEndedIterator<Item = &Entry> {
        self.history.iter()
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    pub fn wake(&mut self, persona: String) {
        self.session = Some(Session {
            persona,
            turns: 0,
            last_active: Instant::now(),
        });
    }

    pub fn sleep(&mut self) {
        self.session = None;
    }

    /// Records that the bot took part (replied), resetting the idle timer.
    pub fn record_turn(&mut self) {
        if let Some(session) = &mut self.session {
            session.turns += 1;
            session.last_active = Instant::now();
        }
    }

    /// Puts the bot to sleep if it has not taken part for `timeout`.
    /// Checked lazily on each message, so no timer task is needed.
    pub fn sleep_if_idle(&mut self, timeout: Duration) -> bool {
        let idle = self
            .session
            .as_ref()
            .is_some_and(|s| s.last_active.elapsed() > timeout);
        if idle {
            self.session = None;
        }
        idle
    }
}
