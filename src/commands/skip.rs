use std::sync::atomic::Ordering;

use serenity::{
    builder::CreateApplicationCommand,
    model::prelude::interaction::{
        application_command::ApplicationCommandInteraction, InteractionResponseType,
    },
    prelude::Context,
};

use crate::{BotParticipantCount, BotSkipVotes};

pub fn register_skip(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
    command.name("skip").description("Skip the current song")
}

pub async fn run_skip(ctx: &Context, interaction: &ApplicationCommandInteraction) {
    let counter_lock = {
        let data_read = ctx.data.read().await;
        data_read
            .get::<BotSkipVotes>()
            .expect("Expected SkipVotes")
            .clone()
    };

    let participant_lock = {
        let data_read = ctx.data.read().await;
        data_read
            .get::<BotParticipantCount>()
            .expect("Expected SkipVotes")
            .clone()
    };
    let count = {
        let mut counter = counter_lock.write().await;
        counter.insert(interaction.user.id.0);
        counter.len()
    };
    println!("Skip Count: {}", count);
    interaction
        .create_interaction_response(ctx, |f| {
            f.kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|m| m.content(format!("Skip count: {}", count)))
        })
        .await
        .unwrap();
    let participants = participant_lock.load(Ordering::Relaxed);
    if count as f32 / participants as f32 >= 0.32 {
        interaction.channel_id.say(ctx, "Skipping!").await.unwrap();
    }
}
