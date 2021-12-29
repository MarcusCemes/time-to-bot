use std::{collections::HashSet, env, fmt, sync::Arc, time::Duration};

use async_trait::async_trait;
use serenity::{
    builder::CreateApplicationCommands,
    client::bridge::gateway::GatewayIntents,
    constants::GATEWAY_VERSION,
    framework::StandardFramework,
    http::Http,
    model::{interactions::application_command::ApplicationCommandInteraction, prelude::*},
    prelude::*,
};
use tokio::time::sleep;
use tracing::log::*;

#[cfg(debug_assertions)]
const DEV_GUILD_ID: u64 = 689081431208886323;

const MS_PER_CHAR: u64 = 100;

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        handle_interaction_create(ctx, interaction).await;
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);
        register_commands(&ctx).await;
    }
}

async fn handle_interaction_create(ctx: Context, interaction: Interaction) {
    if let Interaction::ApplicationCommand(command) = interaction {
        match command.data.name.as_str() {
            "gather" => start_gathering(&ctx.http, &command).await,
            "ping" => respond_to_interaction(&ctx.http, &command, "Hey, I'm alive!").await,
            cmd => warn!("The command \"{}\" is not implemented", cmd),
        };
    }
}

struct GatherSequence<'a> {
    channel: &'a GuildChannel,
    http: &'a Arc<Http>,
}

enum GatherAction {
    Sleep(u64),
    Say(&'static str),
    WithEmoji(&'static str, char),
}

impl GatherSequence<'_> {
    async fn run(&self) -> Result<(), &'static str> {
        use GatherAction::*;

        let sequence = [
            Sleep(1000),
            Say("Hey!"),
            Sleep(1000),
            Say("What's going on here?"),
            Sleep(1000),
            WithEmoji("Why is @everyone asleep!?!?", '😴'),
            Sleep(2000),
            Say("Come on come one! Wake up!"),
            Sleep(500),
            Say("It's Time to Game!"),
        ];

        for action in sequence {
            match action {
                Sleep(millis) => Self::sleep_millis(millis).await,
                Say(msg) => {
                    self.say(msg).await?;
                }
                WithEmoji(msg, emoji) => {
                    let message = self.say(msg).await?;
                    Self::sleep_millis(500).await;
                    message
                        .react(&self.http, emoji)
                        .await
                        .map_err(|_| "Failed to react to message")?;
                }
            }
        }

        Ok(())
    }

    async fn say(&self, content: &str) -> Result<Message, &'static str> {
        let delay = Duration::from_millis(content.len() as u64 * MS_PER_CHAR);
        say_with_typing(self.http, self.channel, delay, content).await
    }

    async fn sleep_millis(millis: u64) {
        sleep(Duration::from_millis(millis)).await;
    }
}

async fn start_gathering(http: &Arc<Http>, command: &ApplicationCommandInteraction) {
    let try_channel = get_guild_channel(http, command.channel_id).await;

    match try_channel {
        Ok(channel) => {
            let interaction_response =
                format!("OK {}, let's get this party started!", command.user.name);

            respond_to_interaction(http, command, interaction_response).await;

            let try_run = GatherSequence {
                channel: &channel,
                http,
            }
            .run()
            .await;

            if let Err(error) = try_run {
                channel
                    .say(http, format!("Encountered an error: {}", error))
                    .await
                    .ok();
            }
        }

        Err(msg) => respond_to_interaction(http, command, msg).await,
    }
}

async fn get_guild_channel(
    http: &Arc<Http>,
    channel_id: ChannelId,
) -> Result<GuildChannel, &'static str> {
    channel_id
        .to_channel(http)
        .await
        .map_err(|why| {
            warn!("Unable to find channel from interaction! Error: {}", why);
            "Internal error: could not find the originating channel."
        })?
        .guild()
        .ok_or("This command can only be used in a server channel.")
}

async fn say_with_typing(
    http: &Arc<Http>,
    channel: &GuildChannel,
    delay: Duration,
    content: impl fmt::Display,
) -> Result<Message, &'static str> {
    let try_typing = channel.clone().start_typing(http);
    tokio::time::sleep(delay).await;
    let message = channel
        .say(http, content)
        .await
        .map_err(|_| "Unable to send message in channel")?;

    if let Ok(typing) = try_typing {
        typing.stop();
    }

    Ok(message)
}

async fn respond_to_interaction<C>(
    http: &Arc<Http>,
    command: &ApplicationCommandInteraction,
    content: C,
) where
    C: ToString,
{
    let try_respond = command
        .create_interaction_response(http, |response| {
            response
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|message| message.content(content))
        })
        .await;

    if let Err(why) = try_respond {
        error!("Cannot respond to slash command: {}", why);
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
    commands
        .create_application_command(|command| {
            command
                .name("gather")
                .description("A call to gather all server members for some games")
        })
        .create_application_command(|command| command.name("ping").description("A ping command"))
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
