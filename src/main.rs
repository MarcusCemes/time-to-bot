use std::env;

use async_trait::async_trait;
use serenity::{client::EventHandler, framework::StandardFramework, Client};

struct Handler;

#[async_trait]
impl EventHandler for Handler {}

#[tokio::main]
async fn main() {
    init();

    let framework = StandardFramework::new().configure(|c| c.with_whitespace(true).prefix("!"));
    let token = env::var("DISCORD_TOKEN").expect("Must provide the Discord token");

    let mut client = Client::builder(&token)
        .event_handler(Handler)
        .framework(framework)
        .await
        .expect("Error while creating client");

    if let Err(why) = client.start().await {
        println!("An error occurred while running the client: {:?}", why);
    }
}

fn init() {
    color_eyre::install().expect("Failed to setup panic handling");
    tracing_subscriber::fmt::init();
}
