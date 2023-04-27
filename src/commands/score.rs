use std::sync::Arc;

use serenity::{
    builder::{CreateApplicationCommand, CreateEmbed},
    model::{
        prelude::{
            component::ButtonStyle,
            interaction::{
                application_command::ApplicationCommandInteraction,
                message_component::MessageComponentInteraction, InteractionResponseType,
            },
        },
        user::User,
    },
    prelude::Context,
};
use sqlx::types::chrono;

use crate::{database::game::read_leaderboard, BotDatabase};

pub fn register_score(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
    command
        .name("score")
        .description("Display the Trivia-Scoreboard")
}

pub async fn run_score(ctx: &Context, interaction: &ApplicationCommandInteraction) {
    let db = {
        let data = ctx.data.read().await;
        data.get::<BotDatabase>().unwrap().clone()
    };
    let scores = read_leaderboard(&db).await.unwrap();
    let mut user_scores = Vec::<(User, i32)>::new();
    for (player_id, score) in scores {
        let user = ctx.http.get_user(player_id).await.unwrap();
        user_scores.push((user, score));
    }
    interaction
        .create_interaction_response(ctx, |f| {
            f.kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|d| {
                    d.title("Trivia-Scoreboard");
                    d.add_embed({
                        let mut embed = CreateEmbed::default();
                        embed.title("Trivia-Scoreboard");
                        embed.description("The current Trivia-Scoreboard");
                        let mut i = 1;
                        for (user, score) in user_scores {
                            embed.field("", format!("**#{} - {} : {}**", i, user, score), false);
                            i += 1;
                        }
                        embed
                    });
                    d.components(|c| {
                        c.create_action_row(|a| {
                            a.create_button(|b| {
                                b.label("🔄");
                                b.style(ButtonStyle::Primary);
                                b.custom_id("refresh");
                                b
                            })
                        })
                    })
                })
        })
        .await
        .unwrap();
}

pub async fn refresh(ctx: &Context, interaction: Arc<MessageComponentInteraction>) {
    let db = {
        let data = ctx.data.read().await;
        data.get::<BotDatabase>().unwrap().clone()
    };
    let scores = read_leaderboard(&db).await.unwrap();
    let mut user_scores = Vec::<(User, i32)>::new();
    for (player_id, score) in scores {
        let user = ctx.http.get_user(player_id).await.unwrap();
        user_scores.push((user, score));
    }
    let current_time = chrono::Utc::now().with_timezone(&chrono::Local);
    let formatted_time = current_time.format("%d.%m.%Y %H:%M:%S").to_string();
    interaction
        .create_interaction_response(&ctx, |f| {
            f.kind(InteractionResponseType::UpdateMessage);
            f.interaction_response_data(|d| {
                d.set_embed({
                    let mut embed = CreateEmbed::default();
                    embed.title("Trivia-Scoreboard");
                    embed.description(format!("**{}**", formatted_time));
                    let mut i = 1;
                    for (user, score) in user_scores {
                        embed.field("", format!("**#{} - {} : {}**", i, user, score), false);
                        i += 1;
                    }
                    embed
                });
                d.components(|c| {
                    c.create_action_row(|a| {
                        a.create_button(|b| {
                            b.label("🔄");
                            b.style(ButtonStyle::Primary);
                            b.custom_id("refresh");
                            b
                        })
                    })
                })
            })
        })
        .await
        .unwrap();
}
