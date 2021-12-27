use std::env;

use async_trait::async_trait;
use serenity::{
    client::{Context, EventHandler},
    framework::{
        standard::{macros::command, CommandResult},
        StandardFramework,
    },
    model::channel::Message,
    Client,
};

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    // async fn message(&self, ctx: Context, msg: Message) {
    //     if msg.content == "!ping" {
    //         // Sending a message can fail, due to a network error, an
    //         // authentication error, or lack of permissions to post in the
    //         // channel, so log to stdout when some error happens, with a
    //         // description of it.
    //         if let Err(why) = msg.channel_id.say(&ctx.http, "Pong!").await {
    //             println!("Error sending message: {:?}", why);
    //         }
    //     }
    // }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
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

#[command]
async fn ping(ctx: &Context, msg: &Message) -> CommandResult {
    msg.reply(ctx, "Pong!").await?;

    Ok(())
}