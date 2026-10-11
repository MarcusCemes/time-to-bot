//! Generic client for OpenRouter's decision-model endpoint (Jev).
//!
//! A decision model takes a `state` plus typed questions and returns, per
//! question, a probability or choice drawn from the options you supplied. It
//! never generates text. What to ask lives in `gate.rs`; this file is only the
//! wire format, so it is the one place to fix if the alpha API changes.
//! Field names follow https://docs.typesafe.ai/api.md; the endpoint path is
//! OpenRouter's alpha route (`alpha.decisions.create` in the SDK), documented at
//! https://openrouter.ai/docs/api/api-reference/alphadecisions/submit-a-decisions-request

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub struct Decisions {
    http: reqwest::Client,
    /// Full URL of the decisions endpoint (`DECISION_ENDPOINT`).
    url: String,
    api_key: String,
    model: String,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    /// Yes/no, answered as a probability from 0 (no) to 1 (yes).
    Noul {
        instructions: String,
        criteria: NoulCriteria,
    },
    /// One of the named options.
    Choice {
        instructions: String,
        criteria: BTreeMap<String, String>,
    },
    /// A position on an ordered scale.
    #[allow(dead_code)] // not used by the gate yet
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
}

#[derive(Serialize)]
pub struct NoulCriteria {
    #[serde(rename = "true")]
    pub yes: String,
    #[serde(rename = "false")]
    pub no: String,
}

impl Question {
    pub fn noul(instructions: &str, yes: &str, no: &str) -> Self {
        Self::Noul {
            instructions: instructions.into(),
            criteria: NoulCriteria {
                yes: yes.into(),
                no: no.into(),
            },
        }
    }

    pub fn choice(instructions: &str, options: &[(&str, &str)]) -> Self {
        Self::Choice {
            instructions: instructions.into(),
            criteria: options
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    session_id: &'a str,
    state: &'a str,
    questions: &'a BTreeMap<&'static str, Question>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Answer {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
    },
    /// Probability-weighted level, so it can fall between integer levels.
    Score {
        score: f64,
    },
}

#[derive(Deserialize)]
struct Response {
    answers: BTreeMap<String, Answer>,
}

/// Answers keyed by question name, with typed accessors.
pub struct Answers(BTreeMap<String, Answer>);

impl Answers {
    pub fn noul(&self, key: &str) -> Option<f64> {
        match self.0.get(key)? {
            Answer::Noul { noul } => Some(*noul),
            _ => None,
        }
    }

    pub fn choice(&self, key: &str) -> Option<&str> {
        match self.0.get(key)? {
            Answer::Choice { choice } => Some(choice),
            _ => None,
        }
    }

    #[allow(dead_code)] // not used by the gate yet
    pub fn score(&self, key: &str) -> Option<f64> {
        match self.0.get(key)? {
            Answer::Score { score } => Some(*score),
            _ => None,
        }
    }
}

impl Decisions {
    pub fn new(http: reqwest::Client, url: String, api_key: String, model: String) -> Self {
        Self {
            http,
            url,
            api_key,
            model,
        }
    }

    /// `session_id` groups related requests so OpenRouter can route them to
    /// the same server (max 256 characters).
    pub async fn decide(
        &self,
        session_id: &str,
        state: &str,
        questions: &BTreeMap<&'static str, Question>,
    ) -> Result<Answers> {
        let response = self
            .http
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .json(&Request {
                model: &self.model,
                session_id,
                state,
                questions,
            })
            .send()
            .await
            .context("sending decision request")?;

        let status = response.status();
        let body = response.text().await.context("reading decision response")?;
        if !status.is_success() {
            bail!("decision request failed ({status}): {body}");
        }

        let parsed: Response = serde_json::from_str(&body)
            .with_context(|| format!("unexpected decision response: {body}"))?;
        Ok(Answers(parsed.answers))
    }
}
