use async_trait::async_trait;
use serenity::all::{
    CommandInteraction, Context, EventHandler, Interaction, Message, Ready,
};

pub mod interaction_create;
pub mod message_create;
pub mod ready;

pub struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        interaction_create::handle_interaction_create(ctx, interaction).await;
    }

    async fn message(&self, ctx: Context, new_message: Message) {
        message_create::handle_message_create(ctx, new_message).await;
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        ready::ready_handler(ctx, ready).await;
    }
}

#[allow(unused)]
fn _type_hint(_: CommandInteraction) {}
