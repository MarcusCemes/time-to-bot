use serenity::{builder::CreateApplicationCommands, model::prelude::*, prelude::*};
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
