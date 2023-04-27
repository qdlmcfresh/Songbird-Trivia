use rspotify::{ClientCredsSpotify, Credentials};
use serenity::collector::ComponentInteractionCollectorBuilder;
use serenity::futures::StreamExt;
use serenity::{
    async_trait,
    client::{Client, Context, EventHandler},
    framework::{standard::macros::group, StandardFramework},
    model::{application::interaction::Interaction, gateway::Ready, prelude::*},
    prelude::*,
};
use songbird::SerenityInit;
use sqlx::{Pool, Sqlite};
use std::{
    collections::HashSet,
    env,
    sync::{atomic::AtomicU8, Arc},
};
extern crate dotenv;
use dotenv::dotenv;

extern crate edit_distance;

mod commands;
mod database;
mod spotify;
mod structs;
pub mod util;
struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        let ctx1 = Arc::new(ctx.clone());
        println!("{} is connected!", ready.user.name);
        let guild_id = GuildId(
            env::var("DISCORD_GUILD_ID")
                .expect("Expected a DISCORD_GUILD_ID in environment")
                .parse()
                .expect("DISCORD_GUILD_ID must be an INTERGER"),
        );
        let _commands = GuildId::set_application_commands(&guild_id, &ctx.http, |commands| {
            commands.create_application_command(|command| commands::quiz::register_quiz(command));
            commands.create_application_command(|command| commands::skip::register_skip(command));
            commands.create_application_command(|command| commands::score::register_score(command))
        })
        .await;
        // Thread to wait for refresh button interactions
        tokio::spawn(async move {
            let mut comp_int = ComponentInteractionCollectorBuilder::new(&*ctx1)
                .filter(move |i| i.data.custom_id == "refresh")
                .build();
            while let Some(event) = comp_int.next().await {
                println!("refreshing");
                commands::score::refresh(&ctx1, event).await;
            }
        });
    }
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let command = match interaction {
            Interaction::ApplicationCommand(command) => command,
            _ => return,
        };
        match command.data.name.as_str() {
            "quiz" => commands::quiz::run_quiz(&ctx, &command).await,
            "skip" => commands::skip::run_skip(&ctx, &command).await,
            "score" => commands::score::run_score(&ctx, &command).await,
            _ => return,
        };
    }
}

#[group]
struct General;

struct BotSpotCred;
impl TypeMapKey for BotSpotCred {
    type Value = Arc<ClientCredsSpotify>;
}

struct BotDatabase;
impl TypeMapKey for BotDatabase {
    type Value = Pool<Sqlite>;
}

struct BotSkipVotes;
impl TypeMapKey for BotSkipVotes {
    type Value = Arc<RwLock<HashSet<u64>>>;
}

struct BotParticipantCount;
impl TypeMapKey for BotParticipantCount {
    type Value = Arc<AtomicU8>;
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Configure the client with your Discord bot token in the environment.
    dotenv().ok();

    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");
    let dbfile = env::var("DATABASE_URL").expect("Expected a DB file in the environment");
    let dbfile_split = dbfile.split(":").collect::<Vec<&str>>()[1];

    let client_id = env::var("SPOTIFY_CLIENT_ID").expect("Expected a token in the environment");
    let client_secret =
        env::var("SPOTIFY_CLIENT_SECRET").expect("Expected a token in the environment");
    let credentials = Credentials::new(&client_id, &client_secret);
    let spotify = ClientCredsSpotify::new(credentials);
    spotify.request_token().await.unwrap();
    let database = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(dbfile_split)
                .create_if_missing(true),
        )
        .await
        .expect("Failed to connect to database");

    let framework = StandardFramework::new()
        .configure(|c| c.prefix("~"))
        .group(&GENERAL_GROUP);
    let intents = GatewayIntents::non_privileged()
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILD_MESSAGE_REACTIONS;

    let mut client = Client::builder(&token, intents)
        .event_handler(Handler)
        .framework(framework)
        .register_songbird()
        .await
        .expect("Err creating client");

    {
        let mut data = client.data.write().await;
        data.insert::<BotSpotCred>(Arc::new(spotify));
        data.insert::<BotDatabase>(database);
        data.insert::<BotSkipVotes>(Arc::new(RwLock::new(HashSet::new())));
        data.insert::<BotParticipantCount>(Arc::new(AtomicU8::new(0)));
    }

    tokio::spawn(async move {
        let _ = client
            .start()
            .await
            .map_err(|why| println!("Client ended: {:?}", why));
    });

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install CTRL+C signal handler");
    println!("Received Ctrl-C, shutting down.");
}
