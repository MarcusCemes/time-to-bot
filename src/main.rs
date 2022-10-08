mod events;

use std::{collections::HashSet, env};

use serenity::{
    constants::GATEWAY_VERSION, framework::StandardFramework, http::Http, model::prelude::*,
    prelude::*,
};
use tracing::log::*;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    init();

    info!("Connecting to gateway v{}", GATEWAY_VERSION);

    let config = load_config();
    let bot_info = get_bot_info(&config.token).await;
    let framework = create_framework(bot_info);
    let mut client = create_client(&config, framework).await;

    if let Err(why) = client.start().await {
        error!("An error occurred while running the client: {:?}", why);
    }
}

fn init() {
    color_eyre::install().expect("Failed to setup panic handling");
    tracing_subscriber::fmt::init();
}

struct Config {
    application_id: u64,
    token: String,
}

fn load_config() -> Config {
    let application_id = env::var("APPLICATION_ID")
        .expect("Must provide the Discord application ID")
        .parse()
        .expect("Application ID is not a valid");

    let token = env::var("DISCORD_TOKEN").expect("Must provide the Discord token");

    Config {
        application_id,
        token,
    }
}

struct BotInfo {
    bot_id: UserId,
    owners: HashSet<UserId>,
}

async fn get_bot_info(token: &str) -> BotInfo {
    let http = Http::new(token);

    match http.get_current_application_info().await {
        Ok(info) => {
            let mut owners = HashSet::new();

            match info.team {
                Some(team) => owners.insert(team.owner_user_id),
                None => owners.insert(info.owner.id),
            };

            match http.get_current_user().await {
                Ok(CurrentUser { id, .. }) => BotInfo { owners, bot_id: id },
                Err(why) => panic!("Could not access the bot id: {:?}", why),
            }
        }
        Err(why) => panic!("Could not access application info: {:?}", why),
    }
}

fn create_framework(bot_info: BotInfo) -> StandardFramework {
    StandardFramework::new().configure(|c| {
        c.on_mention(Some(bot_info.bot_id))
            .owners(bot_info.owners)
            .with_whitespace(true)
            .prefix("!")
    })
}

async fn create_client(config: &Config, framework: StandardFramework) -> Client {
    Client::builder(&config.token, GatewayIntents::empty())
        .application_id(config.application_id)
        .event_handler(events::Handler)
        .framework(framework)
        .await
        .expect("Error while creating client")
}
