
use std::env;

use serenity::{all::GatewayIntents, client::Client};
use tracing::log::*;

mod events;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    init();

    info!("Connecting to gateway");

    let token = env::var("DISCORD_TOKEN").expect("Must provide the Discord token");
    let intents = GatewayIntents::MESSAGE_CONTENT | GatewayIntents::GUILD_MESSAGES;

    let mut client = Client::builder(&token, intents)
        .event_handler(events::Handler)
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
