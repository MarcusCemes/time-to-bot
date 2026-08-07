use serenity::all::{
    CommandInteraction, Context, CreateInteractionResponse, CreateInteractionResponseMessage,
    GetMessages, GuildChannel, Interaction,
};
use tracing::log::*;

use crate::llm::channel_manager::{ChannelManager, HistoryEntry};
use crate::llm::turn;

/// Time the bot should take to type one character, used for typing indicators.
const MS_PER_CHAR: u64 = 100;

pub async fn handle_interaction_create(ctx: Context, interaction: Interaction) {
    if let Interaction::Command(command) = interaction {
        match command.data.name.as_str() {
            "gather" => start_gathering(&ctx, &command).await,
            "ping" => respond_to_interaction(&ctx, &command, "Hey, I'm alive!").await,
            "summon" => summon(&ctx, &command).await,
            "reset" => reset(&ctx, &command).await,
            "status" => status(&ctx, &command).await,
            cmd => warn!("The command \"{}\" is not implemented", cmd),
        };
    }
}

struct GatherSequence<'a> {
    channel: &'a GuildChannel,
    ctx: &'a Context,
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
                            .react(&self.ctx.http, emoji)
                            .await
                            .map_err(|_| "Failed to react to message")?;
                    }
                }
            }
        }

        Ok(())
    }

    async fn say(&self, content: &str) -> Result<serenity::all::Message, &'static str> {
        let delay = std::time::Duration::from_millis(content.chars().count() as u64 * MS_PER_CHAR);
        say_with_typing(self.ctx, self.channel, delay, content).await
    }

    async fn sleep_millis(millis: u64) {
        tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
    }
}

async fn start_gathering(ctx: &Context, command: &CommandInteraction) {
    let try_channel = get_guild_channel(ctx, command).await;

    match try_channel {
        Ok(channel) => {
            let interaction_response =
                format!("Okay {}, let's get this party started!", command.user.name);

            respond_to_interaction(ctx, command, interaction_response).await;

            let try_run = GatherSequence { channel: &channel, ctx }.run().await;

            if let Err(error) = try_run {
                channel
                    .say(&ctx.http, format!("Encountered an error: {}", error))
                    .await
                    .ok();
            }
        }

        Err(msg) => respond_to_interaction(ctx, command, msg).await,
    }
}

async fn get_guild_channel(
    ctx: &Context,
    command: &CommandInteraction,
) -> Result<GuildChannel, &'static str> {
    if command.guild_id.is_none() {
        return Err("This command can only be used in a server channel.");
    }

    if let Some(guild_id) = command.guild_id {
        if let Some(guild) = ctx.cache.guild(guild_id) {
            if let Some(channel) = guild.channels.get(&command.channel_id) {
                return Ok(channel.clone());
            }
        }
    }

    warn!("Unable to find channel in cache for interaction!");
    Err("Internal error: could not find the originating channel.")
}

async fn say_with_typing(
    ctx: &Context,
    channel: &GuildChannel,
    delay: std::time::Duration,
    content: impl ToString,
) -> Result<serenity::all::Message, &'static str> {
    let typing = channel.start_typing(&ctx.http);
    tokio::time::sleep(delay).await;

    let message = channel
        .say(&ctx.http, content.to_string())
        .await;

    typing.stop();

    message.map_err(|_| "Unable to send message in channel")
}

async fn respond_to_interaction<C>(
    ctx: &Context,
    command: &CommandInteraction,
    content: C,
) where
    C: ToString,
{
    let try_respond = command
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content(content.to_string()),
            ),
        )
        .await;

    if let Err(why) = try_respond {
        error!("Cannot respond to slash command: {}", why);
    }
}

/// `/summon` — enter interest with an amusing entrance; the bot acts like it woke up.
async fn summon(ctx: &Context, command: &CommandInteraction) {
    let manager = ctx
        .data
        .read()
        .await
        .get::<ChannelManager>()
        .cloned()
        .expect("ChannelManager missing from context");

    let channel_id = command.channel_id;

    // Backfill transcript from Discord if history is empty (Kimi #8 — mention
    // and summon are symmetric: both get the last 15 min of context).
    {
        let state_arc = manager.get_or_create(channel_id).await;
        let mut state = state_arc.lock().await;
        if state.history.is_empty() {
            // Fetch last 50 messages (15 min at typical pace) and push them
            // oldest→newest so seq order matches.
            if let Ok(messages) = channel_id
                .messages(&ctx.http, GetMessages::new().limit(50))
                .await
            {
                for message in messages.iter().rev() {
                    if message.author.bot {
                        continue;
                    }
                    state.push_message(HistoryEntry {
                        seq: 0, // assigned by push_message
                        author: message.author.name.clone(),
                        content: message.content.clone(),
                        message_id: message.id,
                    });
                }
            } else {
                warn!("failed to backfill transcript for {channel_id}");
            }
        }
        // Set interest + summoned flag.
        state.interest(manager.interest_timeout());
        state.summoned = true;
    }

    respond_to_interaction(ctx, command, "You rang?").await;

    // Trigger the LLM turn (single-flight) so the bot speaks.
    turn::trigger_turn(ctx, manager, channel_id).await;
}

/// `/reset` — clear the channel's context now (ephemeral confirmation — Kimi #22).
async fn reset(ctx: &Context, command: &CommandInteraction) {
    let manager = ctx
        .data
        .read()
        .await
        .get::<ChannelManager>()
        .cloned()
        .expect("ChannelManager missing from context");

    let channel_id = command.channel_id;
    {
        let state_arc = manager.get_or_create(channel_id).await;
        let mut state = state_arc.lock().await;
        state.clear_context();
    }
    respond_to_interaction_ephemeral(ctx, command, "Context cleared.").await;
}

/// `/status` — debugging: show interest window + history length (ephemeral — Kimi #22).
async fn status(ctx: &Context, command: &CommandInteraction) {
    let manager = ctx
        .data
        .read()
        .await
        .get::<ChannelManager>()
        .cloned()
        .expect("ChannelManager missing from context");

    let channel_id = command.channel_id;
    let (interested, history_len, in_flight, queue_len, generation) = {
        let state_arc = manager.get_or_create(channel_id).await;
        let state = state_arc.lock().await;
        (
            state.is_interested(),
            state.history.len(),
            state.generation_in_flight,
            state.send_queue.len(),
            state.generation,
        )
    };
    let msg = format!(
        "interested: {interested}, history: {history_len} entries, in-flight: {in_flight}, queued: {queue_len}, generation: {generation}"
    );
    respond_to_interaction_ephemeral(ctx, command, msg).await;
}

/// Respond to an interaction with an ephemeral message (only the invoker sees it).
async fn respond_to_interaction_ephemeral<C>(
    ctx: &Context,
    command: &CommandInteraction,
    content: C,
) where
    C: ToString,
{
    let try_respond = command
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(content.to_string())
                    .ephemeral(true),
            ),
        )
        .await;

    if let Err(why) = try_respond {
        error!("Cannot respond to slash command: {}", why);
    }
}
