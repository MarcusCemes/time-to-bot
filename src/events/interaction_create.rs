use std::{fmt::Display, sync::Arc, time::Duration};

use serenity::{
    http::Http,
    model::{interactions::application_command::ApplicationCommandInteraction, prelude::*},
    prelude::*,
};
use tokio::time::sleep;
use tracing::log::*;

/// Time the bot should take to type one character, used for typing indicators.
const MS_PER_CHAR: u64 = 100;

#[inline]
pub async fn handle_interaction_create(ctx: Context, interaction: Interaction) {
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
    AddReaction(char),
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
            Say("Why is @everyone asleep!?!?"),
            Sleep(500),
            AddReaction('😴'),
            Sleep(2000),
            Say("Come on come one! Wake up!"),
            Sleep(500),
            Say("It's Time to Game!"),
        ];

        let mut last_message = None;

        for action in sequence {
            match action {
                Sleep(millis) => Self::sleep_millis(millis).await,
                Say(msg) => {
                    last_message = Some(self.say(msg).await?);
                }
                AddReaction(emoji) => {
                    if let Some(message) = &last_message {
                        message
                            .react(&self.http, emoji)
                            .await
                            .map_err(|_| "Failed to react to message")?;
                    }
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
    content: impl Display,
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
