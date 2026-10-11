mod bot;
mod config;
mod decision;
mod gate;
mod llm;
mod persona;
mod state;

use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serenity::{Client, model::gateway::GatewayIntents};
use tracing_subscriber::EnvFilter;

use crate::{bot::Bot, config::Config, decision::Decisions, llm::Llm};

// One thread is plenty for a chat bot; all the work is waiting on the network.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = Config::from_env()?;

    // Fail at startup, not on the first mention, if the persona file is missing.
    persona::load(&config.persona_path)?;

    let http = reqwest::Client::builder()
        .default_headers(attribution_headers())
        .build()
        .context("building HTTP client")?;
    let decisions = Decisions::new(
        http.clone(),
        config.decision_endpoint.clone(),
        config.openrouter_api_key.clone(),
        config.decision_model.clone(),
    );
    let llm = Llm::new(
        http,
        config.llm_endpoint.clone(),
        config.openrouter_api_key.clone(),
        config.llm_model.clone(),
        config.llm_reasoning_effort.clone(),
    );

    // MESSAGE_CONTENT is a privileged intent: enable it in the developer portal.
    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;
    let token = config.discord_token.clone();
    let mut client = Client::builder(token, intents)
        .event_handler(Bot::new(config, decisions, llm))
        .await
        .context("creating Discord client")?;

    client.start().await.context("running Discord client")
}

/// OpenRouter app attribution, sent with every request so usage shows up under
/// a named app in the dashboard. `hidden` keeps the app out of public rankings
/// and the marketplace; OpenRouter applies it only when the app is first
/// created, and changing it later needs support.
/// See https://openrouter.ai/docs/app-attribution
fn attribution_headers() -> HeaderMap {
    HeaderMap::from_iter([
        (
            HeaderName::from_static("http-referer"),
            HeaderValue::from_static("https://mkus.dev"),
        ),
        (
            HeaderName::from_static("x-openrouter-title"),
            HeaderValue::from_static("time-to-bot"),
        ),
        (
            HeaderName::from_static("x-openrouter-app-visibility"),
            HeaderValue::from_static("hidden"),
        ),
    ])
}
