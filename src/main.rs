use std::{collections::HashSet, env};

use async_trait::async_trait;
use serenity::{
    builder::CreateApplicationCommands, client::bridge::gateway::GatewayIntents,
    constants::GATEWAY_VERSION, framework::StandardFramework, model::prelude::*, prelude::*,
};
use tracing::log::*;

const DEV_GUILD_ID: u64 = 689081431208886323;

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        use application_command::ApplicationCommandInteractionDataOptionValue::User;

        if let Interaction::ApplicationCommand(command) = interaction {
            let content = match command.data.name.as_str() {
                "ping" => "Hey, I'm alive!".to_string(),
                "id" => {
                    let options = command
                        .data
                        .options
                        .get(0)
                        .expect("Expected user option")
                        .resolved
                        .as_ref()
                        .expect("Expected user object");

                    match options {
                        User(user, _member) => format!("{}'s id is {}", user.tag(), user.id),
                        _ => "Please provide a valid user".to_string(),
                    }
                }
                _ => "not implemented :(".to_string(),
            };

            if let Err(why) = command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| message.content(content))
                })
                .await
            {
                error!("Cannot respond to slash command: {}", why);
            }
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);
        register_commands(&ctx).await;
    }
}

#[cfg(debug_assertions)]
async fn register_commands(ctx: &Context) {
    GuildId(DEV_GUILD_ID)
        .set_application_commands(&ctx.http, create_commands)
        .await
        .expect("Unable to register global commands");

    info!("Development guild commands registered");
}

#[cfg(not(debug_assertions))]
async fn register_commands(ctx: &Context) {
    use interactions::application_command::ApplicationCommand;

    ApplicationCommand::set_global_application_commands(&ctx.http, create_commands)
        .await
        .expect("Unable to register global commands");

    info!("Global commands registered");
}

fn create_commands(commands: &mut CreateApplicationCommands) -> &mut CreateApplicationCommands {
    use interactions::application_command::ApplicationCommandOptionType::{
        String as StringType, User as UserType,
    };

    commands
        .create_application_command(|command| command.name("ping").description("A ping command"))
        .create_application_command(|command| {
            command
                .name("id")
                .description("Get a user id")
                .create_option(|option| {
                    option
                        .name("id")
                        .description("The user to lookup")
                        .kind(UserType)
                        .required(true)
                })
        })
        .create_application_command(|command| {
            command
                .name("welcome")
                .description("Welcome a user")
                .create_option(|option| {
                    option
                        .name("user")
                        .description("The user to welcome")
                        .kind(UserType)
                        .required(true)
                })
                .create_option(|option| {
                    option
                        .name("message")
                        .description("The message to send")
                        .kind(StringType)
                        .required(true)
                        .add_string_choice(
                            "Welcome to our cool server! Ask me if you need help",
                            "pizza",
                        )
                        .add_string_choice("Hey, do you want a coffee?", "coffee")
                        .add_string_choice(
                            "Welcome to the club, you're now a good person. Well, I hope.",
                            "club",
                        )
                        .add_string_choice(
                            "I hope that you brought a controller to play together!",
                            "game",
                        )
                })
        })
}

#[tokio::main]
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
    use serenity::http::Http;

    let http = Http::new_with_token(token);

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
    Client::builder(&config.token)
        .application_id(config.application_id)
        .intents(gateway_intents())
        .event_handler(Handler)
        .framework(framework)
        .await
        .expect("Error while creating client")
}

const fn gateway_intents() -> GatewayIntents {
    GatewayIntents::empty()
}
