use async_trait::async_trait;
use serenity::{
    model::{application::interaction::Interaction, prelude::*},
    prelude::*,
};

pub mod interaction_create;
pub mod ready;

pub struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        interaction_create::handle_interaction_create(ctx, interaction).await;
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        ready::ready_handler(ctx, ready).await;
    }
}
