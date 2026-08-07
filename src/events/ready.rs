use std::sync::atomic::{AtomicBool, Ordering};

use serenity::all::{Context, CreateCommand, Ready};

#[cfg(debug_assertions)]
use serenity::all::GuildId;
use tracing::log::*;

#[cfg(debug_assertions)]
const DEV_GUILD_ID: u64 = 689081431208886323;

/// Only register commands once per process, even across gateway resumes
/// (Kimi #28).
static COMMANDS_REGISTERED: AtomicBool = AtomicBool::new(false);

pub async fn ready_handler(ctx: Context, ready: Ready) {
    info!("{} is connected!", ready.user.name);
    register_commands(&ctx).await;
}

#[cfg(debug_assertions)]
async fn register_commands(ctx: &Context) {
    // Only register once per process (Kimi #28). The flag is set AFTER a
    // successful registration so a transient failure can retry on resume
    // (Kimi round-4 #14 — previously set before, panicking with no retry).
    if COMMANDS_REGISTERED.load(Ordering::SeqCst) {
        return; // already registered (resume)
    }
    let result = GuildId::new(DEV_GUILD_ID)
        .set_commands(
            &ctx.http,
            vec![
                gather_command(),
                ping_command(),
                summon_command(),
                reset_command(),
                status_command(),
            ],
        )
        .await;
    if let Err(e) = result {
        error!("Failed to register dev guild commands: {e}");
        return; // flag NOT set — a later resume retries
    }
    COMMANDS_REGISTERED.store(true, Ordering::SeqCst);
    info!("Development guild commands registered");
}

#[cfg(not(debug_assertions))]
async fn register_commands(ctx: &Context) {
    if COMMANDS_REGISTERED.load(Ordering::SeqCst) {
        return; // already registered (resume)
    }
    let result = serenity::all::Command::set_global_commands(
        &ctx.http,
        vec![
            gather_command(),
            ping_command(),
            summon_command(),
            reset_command(),
            status_command(),
        ],
    )
    .await;
    if let Err(e) = result {
        error!("Failed to register global commands: {e}");
        return; // flag NOT set — a later resume retries
    }
    COMMANDS_REGISTERED.store(true, Ordering::SeqCst);
    info!("Global commands registered");
}

fn gather_command() -> CreateCommand {
    CreateCommand::new("gather")
        .description("A call to gather all server members for some games")
}

fn ping_command() -> CreateCommand {
    CreateCommand::new("ping").description("A ping command")
}

fn summon_command() -> CreateCommand {
    CreateCommand::new("summon")
        .description("Summon the bot — it wakes up and joins the conversation")
}

fn reset_command() -> CreateCommand {
    CreateCommand::new("reset").description("Clear the bot's context in this channel")
}

fn status_command() -> CreateCommand {
    CreateCommand::new("status").description("Show the bot's state (interest, history, in-flight)")
}
