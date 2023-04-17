use edit_distance::edit_distance;
use rand::seq::SliceRandom;
use serenity::builder::{
    CreateApplicationCommand, CreateButton, CreateEmbed, CreateInteractionResponse,
};
use serenity::model::prelude::component::ButtonStyle;
use serenity::model::prelude::interaction::application_command::CommandDataOptionValue;
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
    collector::MessageCollectorBuilder,
    framework::{
        standard::{
            macros::{command, group},
            CommandResult,
        },
        StandardFramework,
    },
    futures::stream::StreamExt,
    model::{
        application::{
            component::ActionRowComponent::*,
            interaction::{
                application_command::ApplicationCommandInteraction, Interaction,
                InteractionResponseType,
            },
        },
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
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
        let guild_id = GuildId(
            env::var("DISCORD_GUILD_ID")
                .expect("Expected a DISCORD_GUILD_ID in environment")
                .parse()
                .expect("DISCORD_GUILD_ID must be an INTERGER"),
        );
        let commands = GuildId::set_application_commands(&guild_id, &ctx.http, |commands| {
            commands.create_application_command(|command| register_quiz(command));
            commands.create_application_command(|command| register_skip(command))
        })
        .await;
        println!(
            "I now have the following guild slash commands: {:#?}",
            commands
        );
    }
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let command = match interaction {
            Interaction::ApplicationCommand(command) => command,
            _ => return,
        };
        match command.data.name.as_str() {
            "quiz" => run_quiz(&ctx, &command).await,
            "skip" => run_skip(&ctx, &command).await,
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

async fn join_channel(
    ctx: &Context,
    interaction: &ApplicationCommandInteraction,
) -> Result<(), ()> {
    let author_id = interaction.user.id;
    let guild = match interaction.guild_id.unwrap().to_guild_cached(&ctx.cache) {
        Some(it) => it,
        None => return Err(()),
    };
    let channel_id = guild
        .voice_states
        .get(&author_id)
        .and_then(|voice_state| voice_state.channel_id);

    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            interaction
                .create_interaction_response(ctx, |f| {
                    f.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|m| {
                            m.content("You must be in a voice channel to use this command")
                        })
                })
                .await
                .unwrap();
            return Err(());
        }
    };

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let _handler = manager.join(guild.id, connect_to).await;
    return Ok(());
}

async fn leave_channel(
    ctx: &Context,
    interaction: &ApplicationCommandInteraction,
) -> Result<(), ()> {
    let guild = match interaction.guild_id.unwrap().to_guild_cached(&ctx.cache) {
        Some(it) => it,
        None => return Err(()),
    };
    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();
    let has_handler = manager.get(guild.id).is_some();

    if has_handler {
        if let Err(e) = manager.remove(guild.id).await {
            check_msg(
                interaction
                    .channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }
    } else {
        check_msg(
            interaction
                .create_followup_message(ctx, |f| f.content("Not in a voice channel"))
                .await,
        );
    }
    Ok(())
}

fn create_join_response(
    response: &mut CreateInteractionResponse,
    interaction_type: InteractionResponseType,
    players: &HashSet<User>,
) {
    response
        .kind(interaction_type)
        .interaction_response_data(|r| {
            r.content("Click the green button to join the quiz!")
                .components(|c| {
                    c.create_action_row(|row| {
                        row.add_button({
                            let mut b = CreateButton::default();
                            b.custom_id("join_button");
                            b.label("✅ Join");
                            b.style(ButtonStyle::Success);
                            b
                        });

                        row.add_button({
                            let mut b = CreateButton::default();
                            b.custom_id("leave_button");
                            b.label("❌ Leave");
                            b.style(ButtonStyle::Danger);
                            b
                        })
                    })
                })
                .add_embed({
                    let mut e = CreateEmbed::default();
                    e.color(0xff7c1e);
                    e.title("Join the quiz!");
                    let player_string = players
                        .iter()
                        .map(|x| x.to_string().clone())
                        .collect::<Vec<String>>()
                        .join("\n");
                    e.field("Participants", player_string, false);
                    e
                })
        });
}

async fn join_timer(
    ctx: Context,
    interaction: ApplicationCommandInteraction,
    countdown_time: i8,
    player_lock: Arc<RwLock<HashSet<User>>>,
) {
    let mut timer = tokio::time::interval(Duration::from_secs(1));
    let mut count = countdown_time;
    let mut message = interaction.get_interaction_response(&ctx).await.unwrap();
    while count >= 0 {
        timer.tick().await;
        let players = player_lock.read().await;
        message
            .edit(&ctx, |r| {
                r.content(format!("You have {} seconds to join!", count))
                    .components(|c| {
                        c.create_action_row(|row| {
                            row.add_button({
                                let mut b = CreateButton::default();
                                b.custom_id("join_button");
                                b.label("✅ Join");
                                b.style(ButtonStyle::Success);
                                b
                            });

                            row.add_button({
                                let mut b = CreateButton::default();
                                b.custom_id("leave_button");
                                b.label("❌ Leave");
                                b.style(ButtonStyle::Danger);
                                b
                            })
                        })
                    })
                    .set_embed({
                        let mut e = CreateEmbed::default();
                        e.color(0xff7c1e);
                        e.title("Join the quiz!");
                        let player_string = players
                            .iter()
                            .map(|x| x.to_string().clone())
                            .collect::<Vec<String>>()
                            .join("\n");
                        e.field("Participants", player_string, false);
                        e
                    })
            })
            .await
            .unwrap();
        count -= 1;
    }
}

fn register_skip(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
    command.name("skip").description("Skip the current song")
}

async fn run_skip(ctx: &Context, interaction: &ApplicationCommandInteraction) {
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

fn register_quiz(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
    command
        .name("quiz")
        .description("Hier entsteht großes")
        .create_option(|option| {
            option
                .name("quiz_length")
                .description("Quiz length")
                .description_localized("de", "Quizlaenge")
                .kind(command::CommandOptionType::Integer)
                .min_int_value(1)
                .required(true)
        })
}

async fn run_quiz(ctx: &Context, interaction: &ApplicationCommandInteraction) {
    let quiz_length_option = interaction
        .data
        .options
        .get(0)
        .expect("Expected an Option")
        .resolved
        .as_ref()
        .expect("Expected int Object");

    let quiz_length = match quiz_length_option {
        CommandDataOptionValue::Integer(x) => *x as u8,
        _ => {
            check_msg(
                interaction
                    .channel_id
                    .say(ctx, "Quiz length is not an integer!")
                    .await,
            );
            return;
        }
    };
    join_channel(&ctx, &interaction).await.unwrap();
    let channel = interaction.channel_id;
    let database = { ctx.data.read().await.get::<BotDatabase>().unwrap().clone() };
    let spotify = { ctx.data.read().await.get::<BotSpotCred>().unwrap().clone() };
    let playlists = match read_playlists_from_db(database.clone()).await {
        Ok(result) => result,
        _ => {
            check_msg(
                channel
                    .say(ctx, "Reading Playlists from Database failed!")
                    .await,
            );
            return;
        }
    };

    let players = Arc::new(RwLock::new(HashSet::<User>::new()));
    let countdown_time: i8 = 10;
    {
        let p = players.read().await;
        let _asd = interaction
            .create_interaction_response(&ctx.http, |f| {
                create_join_response(f, InteractionResponseType::ChannelMessageWithSource, &p);
                f
            })
            .await;
    }

    let resp = interaction.get_interaction_response(ctx).await;
    let message = match resp {
        Ok(resp) => resp,
        _ => {
            check_msg(channel.say(ctx, "Something went wrong here!").await);
            return;
        }
    };
    tokio::spawn(join_timer(
        ctx.clone(),
        interaction.clone(),
        countdown_time,
        Arc::clone(&players),
    ));

    let interactions = message.await_component_interactions(ctx);
    let mut response_collector = interactions
        .timeout(Duration::from_secs((countdown_time + 1) as u64))
        .build();
    while let Some(event) = response_collector.next().await {
        match event.data.custom_id.as_str() {
            "join_button" => {
                {
                    let mut p = players.write().await;
                    p.insert(event.user.clone());
                    println!("{:?}", p);
                }
                let _e = event
                    .create_interaction_response(ctx, |resp| {
                        resp.kind(InteractionResponseType::UpdateMessage)
                    })
                    .await;
            }
            "leave_button" => {
                {
                    let mut p = players.write().await;
                    p.remove(&event.user);
                    println!("{:?}", p);
                }
                let _e = event
                    .create_interaction_response(ctx, |resp| {
                        resp.kind(InteractionResponseType::UpdateMessage)
                    })
                    .await;
            }

            _ => {}
        }
    }
    let playlist_message = interaction
        .create_followup_message(ctx, |f| {
            f.content("Please select a playlist!");
            f.ephemeral(true).components(|c| {
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
        .await
        .unwrap();
    let playlist_interaction = match playlist_message
        .await_component_interaction(&ctx)
        .timeout(Duration::from_secs(60 * 3))
        .await
    {
        Some(x) => x,
        None => {
            playlist_message.reply(&ctx, "Timed out").await.unwrap();
            return;
        }
    };
    let interaction_result = &playlist_interaction.data.values[0];
    let selected_playlist = if interaction_result == "Add new" {
        playlist_interaction
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
                return;
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
            return;
        }
        let modal_playlist = get_playlist_data(&spotify, modal_result.clone()).await;
        if add_playlist_to_db(&database, modal_playlist, interaction.user.id.0)
            .await
            .is_err()
        {
            modal_interaction
                .create_interaction_response(&ctx, |r| {
                    r.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|f| {
                            f.ephemeral(true).content("Failed to add Playlist to DB!")
                        })
                })
                .await
                .unwrap();
            return;
        }
        modal_interaction
            .create_interaction_response(&ctx, |r| {
                r.kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|f| {
                        f.ephemeral(true)
                            .content(format!("You added {:?} ", modal_result.clone()))
                    })
            })
            .await
            .unwrap();
        modal_result
    } else {
        let result = &playlist_interaction.data.values[0];
        playlist_interaction
            .create_interaction_response(&ctx, |r| {
                r.kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|f| {
                        f.content(format!(
                            "{} chose:\n{}",
                            interaction.user.to_string(),
                            result
                        ))
                    })
            })
            .await
            .unwrap();
        result.to_string()
    };
    println!("Selected playlist: {}", selected_playlist);

    // Store number of participants for skip command
    let participant_lock = {
        let data_read = ctx.data.read().await;
        data_read
            .get::<BotParticipantCount>()
            .expect("Expected BotParticipantCount")
            .clone()
    };
    let player_count = {
        let p = players.read().await;
        p.len()
    };

    participant_lock.store(player_count as u8, Ordering::SeqCst);

    let mut tracks = match get_tracks(&spotify, selected_playlist).await {
        Ok(t) => t,
        _ => {
            check_msg(
                interaction
                    .channel_id
                    .say(&ctx, "Failed to fetch Songs from Spotify!")
                    .await,
            );
            return;
        }
    };

    tracks.shuffle(&mut rand::thread_rng());

    let mut round_counter: u32 = 1;

    let guild_id = interaction.guild_id.unwrap();

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let me = ctx.http.get_current_user().await.unwrap();

    let mut scores = HashMap::<User, u8>::new();
    {
        let players = players.read().await;
        for player in players.iter() {
            scores.insert(player.clone(), 0);
        }
    }
    let user_ids = Arc::new(
        scores
            .keys()
            .cloned()
            .map(|k| k.id.0)
            .collect::<HashSet<u64>>(),
    );

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

                    check_msg(
                        interaction
                            .channel_id
                            .say(&ctx.http, "Error sourcing ffmpeg")
                            .await,
                    );

                    return;
                }
            };
            handler.play_source(source).set_volume(0.5).unwrap();
            println!("Playing: {} by {}", track.song_name, track.artist_name);
            let user_arc = Arc::clone(&user_ids);
            let collector = MessageCollectorBuilder::new(ctx)
                .channel_id(interaction.channel_id)
                .collect_limit(1u32)
                .timeout(Duration::from_secs(30))
                .filter(move |m| {
                    // Check if Message is from Bot and includes 'skipping' to end early
                    if m.author.id.0 == me.id.0 && m.content == "Skipping!" {
                        return true;
                    }
                    if !user_arc.contains(&m.author.id.0) {
                        return false;
                    }
                    is_guess_correct(&m.content, &filter_track, 3)
                })
                .build();

            let collected: Vec<_> = collector.then(|msg| async move { msg }).collect().await;
            handler.stop();
            if collected.len() > 0 {
                let winning_msg = collected.get(0).unwrap();
                if !(winning_msg.author.id.0 == me.id.0) {
                    println!("{} guessed it!", winning_msg.author.name);
                    let _ = winning_msg
                        .reply(
                            ctx,
                            &format!(
                                "{} guessed it!\n{}",
                                winning_msg.author.to_string(),
                                track.url
                            ),
                        )
                        .await;
                    scores.insert(
                        winning_msg.author.clone(),
                        scores.get(&winning_msg.author).unwrap() + 1,
                    );
                } else {
                    check_msg(
                        channel
                            .say(
                                &ctx.http,
                                format!("Better luck next time!, the Song was: {} ", track.url),
                            )
                            .await,
                    );
                }
            } else {
                check_msg(
                    channel
                        .say(
                            &ctx.http,
                            format!("Better luck next time!, the Song was: {} ", track.url),
                        )
                        .await,
                );
            }
        }
        round_counter += 1;
    }
    let mut score_message = MessageBuilder::new();
    score_message.push_bold_line("The Quiz is over! Here are the results:");
    let mut participants_vec: Vec<_> = scores.iter().collect();
    participants_vec.sort_by(|a, b| b.1.cmp(a.1));
    for (user, score) in participants_vec {
        score_message.push_bold_line(&format!("{}: {}", user.to_string(), score));
    }
    let message_string = score_message.build();
    check_msg(channel.say(&ctx.http, &message_string).await);
    leave_channel(&ctx, &interaction).await.unwrap();
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

fn levenshtein_distance(s1: &str, s2: &str) -> usize {
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
        }
        offset += limit;
        if pl.next.is_none() {
            break;
        }
    }
    Ok(tracks)
}
fn is_guess_correct(guess: &str, track: &Song, threshold: usize) -> bool {
    let track_title = &track.song_name.to_lowercase();
    let invalid_chars = ['&', '#', '/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    let mut valid_title = track_title.as_str();
    let guess = guess.to_lowercase();

    for keyword in ["feat.", "ft.", "remix", "edit"].iter() {
        if let Some(idx) = track_title.find(keyword) {
            valid_title = &track_title[..idx];
            break;
        }
    }

    let binding = valid_title
        .chars()
        .filter(|c| !invalid_chars.contains(c))
        .collect::<String>();
    valid_title = binding.as_str();
    let guess = guess
        .chars()
        .filter(|c| !invalid_chars.contains(c))
        .collect::<String>();

    let dist = levenshtein_distance(&guess, valid_title);

    if dist <= threshold {
        true
    } else {
        false
    }
}

// fn validate_guess(guess: &str, track: &Song) -> bool {
//     // TODO: Validate that guessing player is part of the game
//     let mut guess = guess.to_lowercase();
//     guess = guess.replace(" ", "");
//     let mut track_name = track.song_name.to_lowercase();
//     track_name = track_name.replace(" ", "");
//     let mut artist_name = track.artist_name.to_lowercase();
//     artist_name = artist_name.replace(" ", "");
//     if guess == track_name || guess == artist_name {
//         return true;
//     }
//     if levenshtein_distance(&guess, &track_name) < 3
//         || levenshtein_distance(&guess, &artist_name) < 3
//     {
//         return true;
//     }
//     return false;
// }

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

/// Checks that a message successfully sent; if not, then logs why to stdout.
fn check_msg(result: SerenityResult<Message>) {
    if let Err(why) = result {
        println!("Error sending message: {:?}", why);
    }
}
