use serenity::all::{Context, CreateCommand, Ready};

#[cfg(debug_assertions)]
use serenity::all::GuildId;
use tracing::log::*;

#[cfg(debug_assertions)]
const DEV_GUILD_ID: u64 = 689081431208886323;

#[inline]
pub async fn ready_handler(ctx: Context, ready: Ready) {
    info!("{} is connected!", ready.user.name);
    register_commands(&ctx).await;
}

#[cfg(debug_assertions)]
async fn register_commands(ctx: &Context) {
    GuildId::new(DEV_GUILD_ID)
        .set_commands(ctx, vec![gather_command(), ping_command()])
        .await
        .expect("Unable to register dev guild commands");

    info!("Development guild commands registered");
}

#[cfg(not(debug_assertions))]
async fn register_commands(ctx: &Context) {
    serenity::all::Command::set_global_commands(ctx, vec![gather_command(), ping_command()])
        .await
        .expect("Unable to register global commands");

    info!("Global commands registered");
}

fn gather_command() -> CreateCommand {
    CreateCommand::new("gather")
        .description("A call to gather all server members for some games")
}

fn ping_command() -> CreateCommand {
    CreateCommand::new("ping").description("A ping command")
}
