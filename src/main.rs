
use std::env;

use serenity::all::GatewayIntents;
use serenity::client::Client;
use tracing::log::*;

use crate::config::Config;
use crate::llm::agent::LlmAgent;
use crate::llm::channel_manager::ChannelManager;

mod config;
mod events;
mod llm;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    init();

    info!("Connecting to gateway");

    let token = env::var("DISCORD_TOKEN").expect("Must provide the Discord token");
    // GUILDS intent: needed for mentions_me to resolve role/group mentions from
    // the guild cache (Kimi #12).
    let intents = GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::GUILDS;

    let config = Config::from_env();
    let channel_manager = ChannelManager::new(config.interest_timeout);
    let llm_agent = LlmAgent::new(&config);

    let mut client = Client::builder(&token, intents)
        .event_handler(events::Handler)
        .type_map_insert::<ChannelManager>(channel_manager)
        .type_map_insert::<LlmAgent>(llm_agent)
        .await
        .expect("Error while creating client");

    if let Err(why) = client.start().await {
        error!("An error occurred while running the client: {:?}", why);
    }
}

fn init() {
    color_eyre::install().expect("Failed to setup panic handling");
    tracing_subscriber::fmt::init();
}
