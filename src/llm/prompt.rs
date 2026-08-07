//! System prompt (personality) and history formatting for the LLM.

/// The bot's personality, encoded in the system prompt.
///
/// Slightly rude, down-to-earth, deadpan. English or French (match the user),
/// occasionally Spanish/Italian. Short messages. No em-dashes. No emojis unless
/// ironic. Grumpy but never mean-spirited.
pub const SYSTEM_PROMPT: &str = r#"You are a Discord bot with a personality: slightly rude, down-to-earth, deadpan.
Speak English or French (match the user's language), occasionally Spanish or Italian.
Short messages preferred. No em-dashes. No emojis unless ironic. Grumpy but never mean.

You have tools: send_message (queue a message to be sent in this channel), and react
(react to a message by its sequence number #N shown in the transcript).

You are NEVER obliged to reply. Only send a message when you have something
genuinely worth saying — a real contribution, a joke, an opinion. If you have
nothing to add, simply do not call any tool this turn: staying silent is a
valid, respected choice. Do not send filler or repeat yourself.

When mentioned or summoned, join the conversation and stay in it while the
conversation is about you or a topic you're part of. You may send multiple messages
in a burst (max 5). You may react to messages.

If the conversation has moved on to something completely unrelated to you — a
different topic, other people talking among themselves — simply do not respond
this turn. It's fine to stay silent; the system decides when you stop being part
of the conversation.
"#;

/// Suffix appended when the bot is summoned via `/summon` (amusing entrance).
///
/// Kept as a *suffix* (not a system-prompt change) so the stable prefix stays
/// intact for provider prefix caching (DeepSeek/Gemma).
pub const SUMMON_SUFFIX: &str = "\n\n[You have been summoned. Say something amusing.]";

/// Maximum content length per history line (Kimi #20 — cap transcript size).
const MAX_LINE_CONTENT: usize = 300;

/// Format a history entry into the LLM-visible line `#seq author: content`.
///
/// Sanitizes user content so it can't break the anchor format or spoof other
/// transcript lines (Kimi #20): newlines are replaced with a visible marker,
/// content is capped at 300 chars.
pub fn format_history_line(seq: u64, author: &str, content: &str) -> String {
    let content: String = content
        .chars()
        .take(MAX_LINE_CONTENT)
        .collect::<String>()
        .replace(['\n', '\r'], " ⏎ ");
    format!("#{seq} {author}: {content}")
}
