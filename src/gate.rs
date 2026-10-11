//! The gate: the questions we put to the decision model while the bot is awake.
//!
//! One request per incoming message, answered in a single cheap call. To make
//! the bot decide something new (who to address, whether to roast someone,
//! how long to reply...), add a question in `assess` and a field on `Verdict`.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::{
    decision::{Decisions, Question},
    state::Entry,
};

/// Probability above which the latest message is treated as being for the bot.
const RESPOND_THRESHOLD: f64 = 0.6;
/// Probability above which the bot considers the conversation finished.
const SLEEP_THRESHOLD: f64 = 0.7;
/// Probability above which a joke is suggested to the reply model.
const JOKE_THRESHOLD: f64 = 0.6;

pub struct Verdict {
    pub directed_at_bot: f64,
    pub conversation_over: f64,
    pub joke: f64,
    pub tone: Option<String>,
}

impl Verdict {
    pub fn should_respond(&self) -> bool {
        self.directed_at_bot >= RESPOND_THRESHOLD
    }

    pub fn should_sleep(&self) -> bool {
        self.conversation_over >= SLEEP_THRESHOLD
    }

    /// Guidance for the reply model. It changes every turn, so it goes at the
    /// very end of the prompt to keep the cached prefix intact.
    pub fn hint(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(tone) = &self.tone {
            parts.push(format!("suggested tone: {tone}"));
        }
        if self.joke >= JOKE_THRESHOLD {
            parts.push("a joke would land well here".to_string());
        }
        (!parts.is_empty()).then(|| parts.join("; "))
    }
}

pub async fn assess<'a>(
    decisions: &Decisions,
    session_id: &str,
    bot_name: &str,
    history: impl Iterator<Item = &'a Entry>,
) -> Result<Verdict> {
    let mut state = format!(
        "A Discord channel conversation. A chatbot called \"{bot_name}\" is taking part.\n\
         Transcript, oldest first; the last line is the latest message:\n\n"
    );
    for entry in history {
        state.push_str(&format!("{}: {}\n", entry.author, entry.content));
    }

    let questions = BTreeMap::from([
        (
            "directed_at_bot",
            Question::noul(
                "Is the latest message addressed to the chatbot, or a natural continuation \
                 of the chatbot's current exchange?",
                "Addressed to the chatbot, replies to it, or expects it to answer",
                "People are talking among themselves, or it is not the chatbot's place to answer",
            ),
        ),
        (
            "conversation_over",
            Question::noul(
                "Has the conversation involving the chatbot run its course?",
                "Wrapped up, changed to an unrelated topic, or the chatbot is no longer wanted",
                "The chatbot is still part of an ongoing exchange",
            ),
        ),
        (
            "joke",
            Question::noul(
                "Would a witty remark fit the latest message?",
                "Casual or playful, a joke would be welcome",
                "Serious, sensitive or a plain question",
            ),
        ),
        (
            "tone",
            Question::choice(
                "Which tone suits a reply to the latest message?",
                &[
                    ("playful", "Casual banter, teasing, jokes"),
                    ("deadpan", "Dry, understated humour"),
                    ("helpful", "A straightforward, useful answer"),
                    ("supportive", "Someone is upset or needs encouragement"),
                ],
            ),
        ),
    ]);

    let answers = decisions.decide(session_id, &state, &questions).await?;

    Ok(Verdict {
        directed_at_bot: answers.noul("directed_at_bot").unwrap_or(0.0),
        conversation_over: answers.noul("conversation_over").unwrap_or(0.0),
        joke: answers.noul("joke").unwrap_or(0.0),
        tone: answers.choice("tone").map(str::to_owned),
    })
}
