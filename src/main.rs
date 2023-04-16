use edit_distance::edit_distance;
use rand::seq::SliceRandom;
use songbird::SerenityInit;
use sqlx::sqlite::SqliteQueryResult;
use sqlx::{Pool, Sqlite};
use std::{
    collections::{HashMap, HashSet},
    env,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};

use rspotify::{
    model::{PlayableItem, PlaylistId},
    prelude::*,
    ClientCredsSpotify, ClientError, Credentials,
};
use serenity::{
    async_trait,
    client::{Client, Context, EventHandler},
    collector::{EventCollectorBuilder, MessageCollectorBuilder},
    framework::{
        standard::{
            macros::{command, group},
            Args, CommandResult,
        },
        StandardFramework,
    },
    futures::stream::StreamExt,
    model::{
        application::{component::ActionRowComponent::*, interaction::InteractionResponseType},
        channel::Message,
        gateway::Ready,
        prelude::component::InputTextStyle,
        prelude::*,
    },
    prelude::*,
    utils::MessageBuilder,
    Result as SerenityResult,
};

extern crate dotenv;
use dotenv::dotenv;

extern crate edit_distance;

mod structs;
use structs::*;

use sqlx::types::chrono::NaiveDateTime;
struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
}

#[group]
#[commands(join, leave, quiz, skip)]
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

fn levenstein_distance(s1: &str, s2: &str) -> usize {
    edit_distance(s1, s2)
}

const PLAYLIST_ID_REGEX: &str = r"/playlist/(.+)\?";

fn get_id_from_url(url: &str) -> String {
    regex::Regex::new(PLAYLIST_ID_REGEX)
        .unwrap()
        .captures(url)
        .unwrap()[1]
        .to_string()
}

async fn get_playlist_data(spotify: &Arc<ClientCredsSpotify>, url: String) -> Playlist {
    let id_from_url = get_id_from_url(&url);
    spotify.auto_reauth().await.unwrap();

    let playlist_id = PlaylistId::from_id(id_from_url).unwrap();

    let playlist = spotify.playlist(playlist_id, None, None).await.unwrap();

    Playlist::new(
        99999,
        playlist.name,
        url,
        playlist.tracks.total as i64,
        "".to_string(),
        NaiveDateTime::MIN,
        NaiveDateTime::MIN,
    )
}

async fn add_playlist_to_db(
    db: &Pool<Sqlite>,
    playlist: Playlist,
    user: u64,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let user_string = user.to_string();
    sqlx::query!("INSERT INTO playlists (playlist_url, playlist_name, added_by, amount_songs) VALUES (?,?,?,?)",
     playlist.playlist_name,
     playlist.playlist_url,
     user_string,
     playlist.amount_songs)
     .execute(db)
     .await
}

// async fn add_scores_to_db(db: &Pool<Sqlite>,scores: &HashMap<u64, u16>) -> Result<SqliteQueryResult, sqlx::Error> {
//     sqlx::query!("")
// }

async fn get_tracks(
    spotify: &Arc<ClientCredsSpotify>,
    url: String,
) -> Result<Vec<Song>, ClientError> {
    spotify.auto_reauth().await.unwrap();
    let mut tracks = Vec::new();
    let id_from_url = get_id_from_url(&url);
    let playlist_id = PlaylistId::from_id(id_from_url).unwrap();
    let mut offset = 0;
    let limit = 100;
    loop {
        let pl = spotify
            .playlist_items_manual(playlist_id.clone(), None, None, Some(limit), Some(offset))
            .await?;
        for track in pl.items {
            let full_track = match track.track {
                Some(track) => track,
                None => continue,
            };
            let full_track = match full_track {
                PlayableItem::Track(track) => track,
                PlayableItem::Episode(_) => continue,
            };
            match full_track.preview_url {
                Some(_) => (),
                None => continue,
            }
            let artits_string = full_track
                .artists
                .iter()
                .map(|artist| artist.name.to_string())
                .collect::<Vec<String>>()
                .join(", ");
            tracks.push(Song::new(
                full_track.preview_url.unwrap(),
                full_track.name,
                artits_string,
                full_track.album.name,
                full_track.external_urls.get("spotify").unwrap().to_string(),
            ));
            // TODO: Make this safe
        }
        offset += limit;
        if pl.next.is_none() {
            break;
        }
    }
    Ok(tracks)
}

fn validate_guess(guess: &str, track: &Song) -> bool {
    // TODO: Validate that guessing player is part of the game
    let mut guess = guess.to_lowercase();
    guess = guess.replace(" ", "");
    let mut track_name = track.song_name.to_lowercase();
    track_name = track_name.replace(" ", "");
    let mut artist_name = track.artist_name.to_lowercase();
    artist_name = artist_name.replace(" ", "");
    if guess == track_name || guess == artist_name {
        return true;
    }
    if levenstein_distance(&guess, &track_name) < 3 || levenstein_distance(&guess, &artist_name) < 3
    {
        return true;
    }
    return false;
}

// #[command]
// async fn test(ctx: &Context, msg: &Message, _: Args) -> CommandResult {
//     let data_read = ctx.data.read().await;
//     let spotify = data_read.get::<BotSpotCred>().unwrap().clone();
//     spotify.request_token().await.unwrap();
//     let playlist = get_playlist_data(
//         spotify,
//         "https://open.spotify.com/playlist/76XwYVzaGfkpkLJyZnfxK4?si=0e7ed76099954571".to_string(),
//     )
//     .await;
//     println!("{}", playlist.playlist_name);
//     let database = data_read.get::<BotDatabase>().unwrap().clone();
//     let id = msg.author.id.0.to_string();
//     sqlx::query!("INSERT INTO playlists (playlist_url, playlist_name, added_by, amount_songs) VALUES (?,?,?,?)", playlist.playlist_name, playlist.playlist_url, id, playlist.amount_songs).execute(&database).await.unwrap();
//     Ok(())
// }

async fn send_playlist_message(
    ctx: &Context,
    channel: ChannelId,
    playlists: Vec<Playlist>,
) -> Result<Message, serenity::Error> {
    let playlist_message = channel
        .send_message(&ctx, |m| {
            m.content("Please select a playlist or add a new one")
                .components(|c| {
                    c.create_action_row(|row| {
                        row.create_select_menu(|menu| {
                            menu.custom_id("playlist_select");
                            menu.placeholder("Select a playlist");
                            menu.options(|f| {
                                f.create_option(|o| o.label("Add new").value("Add new"));
                                for playlist in playlists {
                                    f.create_option(|o| {
                                        o.label(playlist.playlist_name).value(playlist.playlist_url)
                                    });
                                }
                                f
                            })
                        })
                    })
                })
        })
        .await;
    playlist_message
}

async fn read_playlists_from_db(db: Pool<Sqlite>) -> Result<Vec<Playlist>, sqlx::Error> {
    let playlists = sqlx::query_as!(Playlist, "SELECT * FROM playlists")
        .fetch_all(&db)
        .await;
    playlists
}
const PLAYLIST_VALID_REGEX: &str =
    r"^(https?://)?(www\.)?(open\.)?spotify\.com/playlist/[a-zA-Z0-9]+(\?.*)*$";
fn validate_url(url: &String) -> bool {
    regex::Regex::new(PLAYLIST_VALID_REGEX)
        .unwrap()
        .is_match(&url)
}

#[command]
async fn quiz(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    // TODO: Check if Bot is in message authors voice channel and if not join it
    let quiz_length = match args.parse::<u32>() {
        Ok(quiz_length) => quiz_length,
        Err(_) => {
            msg.channel_id
                .say(&ctx, "Please provide a valid number")
                .await?;
            return Ok(());
        }
    };
    let channel = msg.channel_id;
    let author_nick = ctx.http.get_user(msg.author.id.0).await.unwrap();
    let database = { ctx.data.read().await.get::<BotDatabase>().unwrap().clone() };
    let spotify = { ctx.data.read().await.get::<BotSpotCred>().unwrap().clone() };
    let playlists = match read_playlists_from_db(database.clone()).await {
        Ok(result) => result,
        _ => {
            channel
                .say(ctx, "Reading Playlists from Database failed!")
                .await?;
            return Ok(());
        }
    };

    let playlist_message = match send_playlist_message(ctx, channel, playlists).await {
        Ok(message) => message,
        _ => {
            channel.say(ctx, "Something went wrong here!").await?;
            return Ok(());
        }
    };

    let interaction = match playlist_message
        .await_component_interaction(&ctx)
        .timeout(Duration::from_secs(60 * 3))
        .await
    {
        Some(x) => x,
        None => {
            playlist_message.reply(&ctx, "Timed out").await.unwrap();
            return Ok(());
        }
    };

    let interaction_result = &interaction.data.values[0];
    let selected_playlist = if interaction_result == "Add new" {
        interaction
            .create_interaction_response(&ctx, |r| {
                r.kind(InteractionResponseType::Modal)
                    .interaction_response_data(|d| {
                        d.title("Add a new Playlist");
                        d.custom_id("playlist_modal");
                        d.content("Please enter a Spotify-Playlist URL")
                            .components(|c| {
                                c.create_action_row(|row| {
                                    row.create_input_text(|f| {
                                        f.custom_id("playlist_url");
                                        f.placeholder("Enter a Spotify-Playlist URL");
                                        f.style(InputTextStyle::Short);
                                        f.min_length(10);
                                        f.label("Playlist URL")
                                    })
                                })
                            })
                    })
            })
            .await
            .unwrap();
        let modal_interaction = match playlist_message
            .await_modal_interaction(&ctx)
            .timeout(Duration::from_secs(60 * 3))
            .await
        {
            Some(x) => x,
            None => {
                playlist_message
                    .reply(&ctx, "You took too long to select a playlist")
                    .await
                    .unwrap();
                return Ok(());
            }
        };
        let modal_result = match &modal_interaction.data.components[0].components[0] {
            InputText(t) => t.value.clone(),
            _ => String::new(),
        };
        println!("Modal playlist: {:?}", modal_result);
        if !validate_url(&modal_result) {
            modal_interaction
                .create_interaction_response(&ctx, |r| {
                    r.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|f| {
                            f.content("Please provide a valid Spotify-Playlist-Url")
                        })
                })
                .await
                .unwrap();
            return Ok(());
        }
        let modal_playlist = get_playlist_data(&spotify, modal_result.clone()).await;
        if add_playlist_to_db(&database, modal_playlist, msg.author.id.0)
            .await
            .is_err()
        {
            modal_interaction
                .create_interaction_response(&ctx, |r| {
                    r.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|f| f.content("Failed to add Playlist to DB!"))
                })
                .await
                .unwrap();
            return Ok(());
        }
        modal_interaction
            .create_interaction_response(&ctx, |r| {
                r.kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|f| {
                        f.content(format!("You added {:?} ", modal_result.clone()))
                    })
            })
            .await
            .unwrap();
        modal_result
    } else {
        let result = &interaction.data.values[0];
        println!("Selected playlist: {}", result);
        interaction
            .create_interaction_response(&ctx, |r| {
                r.kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|f| f.content(format!("You selected {:?} ", result)))
            })
            .await
            .unwrap();
        result.to_string()
    };

    println!("Selected playlist: {}", selected_playlist);

    let join_message = MessageBuilder::new()
        .push_bold_line(format!("{} started a Quiz", author_nick.name))
        .push("React to this Message to join the Quiz")
        .build();

    let join_msg = channel.say(&ctx, join_message).await?;

    let mut reaction_collector = EventCollectorBuilder::new(ctx)
        .add_message_id(join_msg.id)
        .add_event_type(EventType::ReactionAdd)
        .timeout(Duration::from_secs(10))
        .build()?;

    let mut participants = HashMap::<u64, u16>::new();

    while let Some(event) = reaction_collector.next().await {
        match event.as_ref() {
            Event::ReactionAdd(event) => {
                let participant_id = event.reaction.user_id.unwrap().0;
                let nick = ctx.http.get_user(participant_id).await?.name;
                participants.insert(participant_id, 0);
                channel
                    .say(&ctx.http, &format!("{} joined the quiz", nick))
                    .await?;
            }
            _ => {}
        }
    }
    // Store number of participants for skip command
    let participant_lock = {
        let data_read = ctx.data.read().await;
        data_read
            .get::<BotParticipantCount>()
            .expect("Expected BotParticipantCount")
            .clone()
    };
    participant_lock.store(participants.len() as u8, Ordering::SeqCst);

    let mut tracks = match get_tracks(&spotify, selected_playlist).await {
        Ok(t) => t,
        _ => {
            msg.channel_id
                .say(&ctx, "Failed to fetch Songs from Spotify!")
                .await?;
            return Ok(());
        }
    };

    tracks.shuffle(&mut rand::thread_rng());

    let mut round_counter: u32 = 1;
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let me = ctx.http.get_current_user().await.unwrap();

    for track in tracks.into_iter().take(quiz_length as usize) {
        // Reset skip counter
        let counter_lock = {
            let data_read = ctx.data.read().await;
            data_read
                .get::<BotSkipVotes>()
                .expect("Expected SkipVotes")
                .clone()
        };
        {
            let mut counter = counter_lock.write().await;
            counter.clear();
        }

        let filter_track = track.clone();
        channel
            .say(&ctx.http, format!("Round {}", round_counter))
            .await
            .unwrap();
        if let Some(handler_lock) = manager.get(guild_id) {
            let mut handler = handler_lock.lock().await;

            let source = match songbird::ytdl(&track.preview_url).await {
                Ok(source) => source,
                Err(why) => {
                    println!("Err starting source: {:?}", why);

                    check_msg(msg.channel_id.say(&ctx.http, "Error sourcing ffmpeg").await);

                    return Ok(());
                }
            };

            handler.play_source(source);
            println!("Playing: {} by {}", track.song_name, track.artist_name);

            let player_ids: Vec<u64> = participants.keys().map(|k| *k).collect();

            let collector = MessageCollectorBuilder::new(ctx)
                .channel_id(msg.channel_id)
                .collect_limit(1u32)
                .timeout(Duration::from_secs(30))
                .filter(move |m| {
                    // Check if Message is from Bot and includes 'skipping' to end early
                    if m.author.id.0 == me.id.0 && m.content == "Skipping!" {
                        return true;
                    }
                    if !player_ids.contains(&m.author.id.0) {
                        return false;
                    }
                    validate_guess(&m.content, &filter_track)
                })
                .build();

            let collected: Vec<_> = collector.then(|msg| async move { msg }).collect().await;
            handler.stop();
            if collected.len() > 0 {
                let winning_msg = collected.get(0).unwrap();
                if !winning_msg.author.id.0 == me.id.0 {
                    let author = &winning_msg.author.name;
                    let _ = winning_msg
                        .reply(ctx, &format!("{} guessed it!", author))
                        .await;
                    participants.insert(
                        winning_msg.author.id.0,
                        participants.get(&winning_msg.author.id.0).unwrap() + 1,
                    );
                } else {
                    channel
                        .say(
                            &ctx.http,
                            format!("Better luck next time!, the Song was: {} ", track.url),
                        )
                        .await?;
                }
            } else {
                channel
                    .say(
                        &ctx.http,
                        format!("Better luck next time!, the Song was: {} ", track.url),
                    )
                    .await?;
            }
        }
        round_counter += 1;
    }
    let mut score_message = MessageBuilder::new();
    score_message.push_bold_line("The Quiz is over! Here are the results:");
    let mut participants_vec: Vec<_> = participants.iter().collect();
    participants_vec.sort_by(|a, b| b.1.cmp(a.1));
    for (user_id, score) in participants_vec {
        let user = ctx.http.get_user(*user_id).await?;
        score_message.push_bold_line(&format!("{}: {}", user.name, score));
    }
    let message_string = score_message.build();
    channel.say(&ctx.http, &message_string).await?;
    // TODO: Add Score to DB
    // TODO: Add Skip Functionality
    // TODO: Nicer Messages
    Ok(())
}

#[command]
async fn skip(ctx: &Context, msg: &Message) -> CommandResult {
    // TODO: validate that message author is part of the game
    // TODO: validate that a game is currently in progress
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
        counter.insert(msg.author.id.0);
        counter.len()
    };
    println!("Skip Count: {}", count);
    let participants = participant_lock.load(Ordering::Relaxed);
    if count as f32 / participants as f32 >= 0.32 {
        msg.channel_id.say(ctx, "Skipping!").await.unwrap();
    }
    return Ok(());
}

#[command]
#[only_in(guilds)]
async fn join(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let channel_id = guild
        .voice_states
        .get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id);

    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        }
    };

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let _handler = manager.join(guild_id, connect_to).await;

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn leave(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();
    let has_handler = manager.get(guild_id).is_some();

    if has_handler {
        if let Err(e) = manager.remove(guild_id).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Left voice channel").await);
    } else {
        check_msg(msg.reply(ctx, "Not in a voice channel").await);
    }

    Ok(())
}

/// Checks that a message successfully sent; if not, then logs why to stdout.
fn check_msg(result: SerenityResult<Message>) {
    if let Err(why) = result {
        println!("Error sending message: {:?}", why);
    }
}
