//! The persona file: personality, rules and knowledge about the friends, as plain markdown.
//!
//! It is re-read from disk every time the bot wakes, so edits made over SSH
//! apply from the next conversation, without a restart.

use std::path::Path;

use anyhow::{Context, Result};

pub fn load(path: &Path) -> Result<String> {
    std::fs::read_to_string(path)
        .with_context(|| format!("reading persona file {}", path.display()))
}
