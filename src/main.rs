use std::{collections::HashSet, env};

use async_trait::async_trait;
use serenity::{model::prelude::*, prelude::*};
use tracing::log::*;

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
        use interactions::application_command::{ApplicationCommand, ApplicationCommandOptionType};

        info!("{} is connected!", ready.user.name);

        ApplicationCommand::set_global_application_commands(&ctx.http, |commands| {
            commands
                .create_application_command(|command| {
                    command.name("ping").description("A ping command")
                })
                .create_application_command(|command| {
                    command
                        .name("id")
                        .description("Get a user id")
                        .create_option(|option| {
                            option
                                .name("id")
                                .description("The user to lookup")
                                .kind(ApplicationCommandOptionType::User)
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
                                .kind(ApplicationCommandOptionType::User)
                                .required(true)
                        })
                        .create_option(|option| {
                            option
                                .name("message")
                                .description("The message to send")
                                .kind(ApplicationCommandOptionType::String)
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
        })
        .await
        .expect("Unable to register global commands");

        info!("Global commands registered");
    }
}

#[tokio::main]
async fn main() {
    use serenity::framework::StandardFramework;

    init();

    let application_id: u64 = env::var("APPLICATION_ID")
        .expect("Must provide the Discord application ID")
        .parse()
        .expect("Application ID is not a valid");

    let token = env::var("DISCORD_TOKEN").expect("Must provide the Discord token");
    let (owners, bot_id) = get_bot_id(&token).await;

    let framework = StandardFramework::new().configure(|c| {
        c.on_mention(Some(bot_id))
            .owners(owners)
            .with_whitespace(true)
            .prefix("!")
    });

    let mut client = Client::builder(&token)
        .application_id(application_id)
        .event_handler(Handler)
        .framework(framework)
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

async fn get_bot_id(token: &str) -> (HashSet<UserId>, UserId) {
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
                Ok(bot_id) => (owners, bot_id.id),
                Err(why) => panic!("Could not access the bot id: {:?}", why),
            }
        }
        Err(why) => panic!("Could not access application info: {:?}", why),
    }
}
