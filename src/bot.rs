//! The Discord event handler: ties together history, gate and reply model.
//!
//! Per message in a channel:
//!   1. record it in the channel's history (always, even asleep);
//!   2. asleep and not mentioned -> stop here, no model calls;
//!   3. mentioned while asleep -> wake (re-reading the persona file);
//!   4. awake -> ask the gate (Jev) whether to reply, and whether to sleep;
//!   5. reply via the OpenRouter preset when mentioned or the gate says so.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
};

use anyhow::Result;
use serenity::{
    async_trait,
    builder::{CreateAllowedMentions, CreateMessage},
    model::{channel::Message, gateway::Ready, id::ChannelId},
    prelude::*,
};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::{
    config::Config,
    decision::Decisions,
    gate::{self, Verdict},
    llm::{ChatMessage, Llm},
    persona,
    state::{Channel, Entry},
};

/// Discord rejects messages longer than this.
const DISCORD_MESSAGE_LIMIT: usize = 2000;

pub struct Bot {
    config: Config,
    decisions: Decisions,
    llm: Llm,
    // Messages in one channel are handled one at a time (the inner lock is
    // held across the gate and reply calls), so two quick messages can't
    // trigger two overlapping replies. Different channels run concurrently.
    channels: StdMutex<HashMap<ChannelId, Arc<Mutex<Channel>>>>,
}

impl Bot {
    pub fn new(config: Config, decisions: Decisions, llm: Llm) -> Self {
        Self {
            config,
            decisions,
            llm,
            channels: StdMutex::new(HashMap::new()),
        }
    }

    fn channel(&self, id: ChannelId) -> Arc<Mutex<Channel>> {
        let capacity = self.config.history_len;
        let mut channels = self.channels.lock().unwrap();
        channels
            .entry(id)
            .or_insert_with(|| Arc::new(Mutex::new(Channel::new(capacity))))
            .clone()
    }

    async fn handle(&self, ctx: &Context, msg: &Message) -> Result<()> {
        // Never react to bots (including ourselves); our own replies are
        // recorded directly when we send them.
        if msg.author.bot {
            return Ok(());
        }

        let (me, bot_name) = {
            let user = ctx.cache.current_user();
            (user.id, user.name.clone())
        };
        let mentioned = msg.mentions_user_id(me);

        let channel = self.channel(msg.channel_id);
        let mut channel = channel.lock().await;

        if channel.sleep_if_idle(self.config.idle_timeout) {
            debug!(channel = %msg.channel_id, "fell asleep (idle)");
        }
        channel.push(Entry {
            author: display_name(msg),
            content: msg.content_safe(&ctx.cache),
            is_bot: false,
        });

        if channel.session().is_none() {
            if !mentioned {
                return Ok(());
            }
            channel.wake(persona::load(&self.config.persona_path)?);
            info!(channel = %msg.channel_id, "woke up");
        }

        // Stable per channel, so each channel's requests hit the same warm cache.
        let session_id = format!("time-to-bot:{}", msg.channel_id);

        // A gate failure shouldn't silence a direct mention, so degrade to
        // "no verdict" rather than bailing out.
        let verdict =
            match gate::assess(&self.decisions, &session_id, &bot_name, channel.history()).await {
                Ok(verdict) => Some(verdict),
                Err(err) => {
                    warn!(error = %format!("{err:#}"), "gate failed");
                    None
                }
            };
        if let Some(v) = &verdict {
            debug!(
                directed = v.directed_at_bot,
                over = v.conversation_over,
                joke = v.joke,
                tone = ?v.tone,
                "gate"
            );
        }

        let respond = mentioned || verdict.as_ref().is_some_and(Verdict::should_respond);
        let over = verdict.as_ref().is_some_and(Verdict::should_sleep);

        if respond {
            let system = system_prompt(&channel.session().expect("awake").persona, &bot_name);
            let mut messages = vec![ChatMessage {
                role: "system",
                content: system,
            }];
            messages.extend(channel.history().map(|e| {
                if e.is_bot {
                    ChatMessage {
                        role: "assistant",
                        content: e.content.clone(),
                    }
                } else {
                    ChatMessage {
                        role: "user",
                        content: format!("{}: {}", e.author, e.content),
                    }
                }
            }));

            // The latest message is always the last one, a user message.
            if let (Some(hint), Some(last)) = (
                verdict.as_ref().and_then(Verdict::hint),
                messages.last_mut(),
            ) {
                last.content
                    .push_str(&format!("\n\n[Guidance for your reply: {hint}]"));
            }

            let _typing = msg.channel_id.start_typing(&ctx.http);
            let reply = self.llm.complete(&session_id, &messages).await?;
            let reply = reply.trim();

            if !reply.is_empty() {
                for chunk in split_message(reply) {
                    // Empty allowed_mentions: the model can't ping anyone.
                    let builder = CreateMessage::new()
                        .content(chunk)
                        .allowed_mentions(CreateAllowedMentions::new());
                    msg.channel_id.send_message(&ctx.http, builder).await?;
                }
                channel.push(Entry {
                    author: bot_name,
                    content: reply.to_string(),
                    is_bot: true,
                });
            }
            channel.record_turn();
        }

        let turns = channel.session().map_or(0, |s| s.turns);
        if over || turns >= self.config.max_turns {
            channel.sleep();
            info!(channel = %msg.channel_id, "went to sleep");
        }
        Ok(())
    }
}

#[async_trait]
impl EventHandler for Bot {
    async fn ready(&self, _ctx: Context, ready: Ready) {
        info!(user = %ready.user.name, "connected");
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if let Err(err) = self.handle(&ctx, &msg).await {
            error!(channel = %msg.channel_id, error = %format!("{err:#}"), "failed to handle message");
        }
    }
}

fn display_name(msg: &Message) -> String {
    msg.member
        .as_ref()
        .and_then(|m| m.nick.clone())
        .or_else(|| msg.author.global_name.clone())
        .unwrap_or_else(|| msg.author.name.clone())
}

/// The persona file is the personality; this adds only the mechanical framing.
///
/// Kept identical between turns of a session so the provider's prompt cache
/// can be reused; per-turn guidance is appended to the last message instead.
fn system_prompt(persona: &str, bot_name: &str) -> String {
    format!(
        "You are {bot_name}, a bot taking part in a Discord group chat.\n\
         Messages from people are prefixed with their name. Reply with only your own \
         message, without a name prefix. Keep it short, like a chat message.\n\n\
         {persona}\n"
    )
}

/// Splits on character boundaries into pieces Discord will accept.
fn split_message(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars
        .chunks(DISCORD_MESSAGE_LIMIT)
        .map(|c| c.iter().collect())
        .collect()
}
