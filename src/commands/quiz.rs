use rand::seq::SliceRandom;
use regex::Regex;
use serenity::{
    async_trait,
    builder::{CreateApplicationCommand, CreateButton, CreateEmbed, CreateInteractionResponse},
    collector::MessageCollectorBuilder,
    futures::StreamExt,
    model::{
        prelude::{
            command,
            component::{ActionRowComponent::InputText, ButtonStyle, InputTextStyle},
            interaction::{
                application_command::{ApplicationCommandInteraction, CommandDataOptionValue},
                InteractionResponseType,
            },
            ChannelId, Guild, ReactionType, UserId,
        },
        user::User,
    },
    prelude::{Context, RwLock},
    utils::MessageBuilder,
};
use songbird::{EventContext, EventHandler as VoiceEventHandler, TrackEvent};
use std::{
    collections::{HashMap, HashSet},
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tracing::info;

use crate::{
    structs::{CollectionResult, Song},
    util::util::{
        add_playlist_to_db, check_msg, get_playlist_data, get_tracks, read_playlists_from_db,
        validate_url,
    },
    BotDatabase, BotParticipantCount, BotSkipVotes, BotSpotCred,
};
use edit_distance::edit_distance;

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

async fn check_for_title(
    ctx: Context,
    channel_id: ChannelId,
    song: Song,
    bot_user: UserId,
    player_lock: Arc<RwLock<HashSet<User>>>,
) -> Result<(User, CollectionResult), ()> {
    let regex_parentheses: Regex = regex::Regex::new(r"\(.*\)").unwrap();
    let regex_dash: Regex = regex::Regex::new(r"-.*").unwrap();
    let regex_special_character: Regex = regex::Regex::new(r"[^0-9a-zA-Z\s]+").unwrap();
    let regex_whitespace: Regex = regex::Regex::new(r"\s*").unwrap();

    let mut song_title = regex_dash.replace_all(&song.song_name, "").to_string();
    song_title = regex_parentheses.replace_all(&song_title, "").to_string();
    song_title = regex_special_character
        .replace_all(&song_title, "")
        .to_string();
    song_title = regex_whitespace.replace_all(&song_title, "").to_string();
    song_title = song_title.to_lowercase();

    let players = {
        let players = player_lock.read().await;
        players.clone()
    };

    let message_collector = MessageCollectorBuilder::new(&ctx)
        .channel_id(channel_id)
        .filter(move |m| {
            if m.author.id == bot_user && m.content == "Skipping!" {
                return true;
            }
            if !players.contains(&m.author) {
                return false;
            }
            let mut guess = regex_dash.replace_all(&m.content, "").to_string();
            guess = regex_parentheses.replace_all(&guess, "").to_string();
            guess = regex_special_character.replace_all(&guess, "").to_string();
            guess = regex_whitespace.replace_all(&guess, "").to_string();
            guess = guess.to_lowercase();

            is_title_correct(&guess, &song_title, 3)
        })
        .collect_limit(1)
        .timeout(Duration::from_secs(29))
        .build();
    let collected: Vec<_> = message_collector
        .then(|msg| async move { msg })
        .collect()
        .await;
    if collected.is_empty() {
        Err(())
    } else {
        let msg = collected.get(0).unwrap();
        if msg.author.id == bot_user {
            return Ok((msg.author.clone(), CollectionResult::Skip));
        }
        msg.react(&ctx, ReactionType::Unicode("🎶".to_string()))
            .await
            .unwrap();
        Ok((msg.author.clone(), CollectionResult::Title))
    }
}

async fn check_for_author(
    ctx: Context,
    channel_id: ChannelId,
    song: Song,
    bot_user: UserId,
    player_lock: Arc<RwLock<HashSet<User>>>,
) -> Result<(User, CollectionResult), ()> {
    let players = {
        let players = player_lock.read().await;
        players.clone()
    };

    let message_collector = MessageCollectorBuilder::new(&ctx)
        .channel_id(channel_id)
        .filter(move |m| {
            if m.author.id == bot_user && m.content == "Skipping!" {
                return true;
            }
            if !players.contains(&m.author) {
                return false;
            }

            is_artist_correct(&m.content, &song, 3)
        })
        .collect_limit(1)
        .timeout(Duration::from_secs(29))
        .build();
    let collected: Vec<_> = message_collector
        .then(|msg| async move { msg })
        .collect()
        .await;
    if collected.is_empty() {
        Err(())
    } else {
        let msg = collected.get(0).unwrap();
        if msg.author.id == bot_user {
            return Ok((msg.author.clone(), CollectionResult::Skip));
        }
        msg.react(&ctx, ReactionType::Unicode("🎙️".to_string()))
            .await
            .unwrap();
        Ok((msg.author.clone(), CollectionResult::Artist))
    }
}

async fn get_winners(
    ctx: &Context,
    channel_id: ChannelId,
    song: Song,
    players: Arc<RwLock<HashSet<User>>>,
) -> (
    Result<(User, CollectionResult), ()>,
    Result<(User, CollectionResult), ()>,
) {
    let me = ctx.cache.current_user();
    let artist_handle = tokio::spawn(check_for_author(
        ctx.clone(),
        channel_id,
        song.clone(),
        me.id,
        players.clone(),
    ));
    let title_handle = tokio::spawn(check_for_title(
        ctx.clone(),
        channel_id,
        song.clone(),
        me.id,
        players,
    ));
    while !artist_handle.is_finished() && !title_handle.is_finished() {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let author = artist_handle.await.unwrap();
    let title = title_handle.await.unwrap();
    (author, title)
}

pub fn register_quiz(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
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

pub async fn run_quiz(ctx: &Context, interaction: &ApplicationCommandInteraction) {
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
                    info!("{:?}", p);
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
                    info!("{:?}", p);
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
        info!("Modal playlist: {:?}", modal_result);
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
    info!("Selected playlist: {}", selected_playlist);

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

    let mut scores = HashMap::<User, u8>::new();
    {
        let players = players.read().await;
        for player in players.iter() {
            scores.insert(player.clone(), 0);
        }
    }

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

        channel
            .say(&ctx.http, format!("Round {}", round_counter))
            .await
            .unwrap();
        if let Some(handler_lock) = manager.get(guild_id) {
            let mut handler = handler_lock.lock().await;

            let source = match songbird::ytdl(&track.preview_url).await {
                Ok(source) => source,
                Err(why) => {
                    info!("Err starting source: {:?}", why);

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
            info!("Playing: {} by {}", track.song_name, track.artist_name);
            let (artist_result, title_result) = {
                let res = get_winners(
                    &ctx,
                    interaction.channel_id.clone(),
                    track.clone(),
                    players.clone(),
                )
                .await;
                (res.0, res.1)
            };
            match artist_result {
                Ok(author) => {
                    let (author_id, res) = author;
                    match res {
                        CollectionResult::Artist => {
                            scores.insert(author_id.clone(), scores.get(&author_id).unwrap() + 1);
                        }
                        _ => {}
                    }
                }
                Err(_) => {}
            }
            match title_result {
                Ok(title) => {
                    let (title, res) = title;
                    match res {
                        CollectionResult::Title => {
                            scores.insert(title.clone(), scores.get(&title).unwrap() + 1);
                        }
                        _ => {}
                    }
                }
                Err(_) => {}
            }
            handler.stop();
            round_counter += 1;

            let trackmsg = MessageBuilder::new()
                .push_bold_line(track.song_name)
                .push_bold_line(track.artist_name)
                .push_line(track.url)
                .build();

            channel.say(&ctx, trackmsg).await.unwrap();
        }
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

fn is_title_correct(guess: &str, track: &String, threshold: usize) -> bool {
    info!("Expecting Song-Title: {}", track);
    let dist = edit_distance(&guess, track);

    if dist <= threshold {
        true
    } else {
        false
    }
}

fn is_artist_correct(guess: &str, track: &Song, threshold: usize) -> bool {
    // TODO: Maybe only check first artist
    let invalid_chars = ['&', '#', '/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    let valid_name = &track.artist_name.to_lowercase();
    let guess = guess.to_lowercase();

    let binding = valid_name
        .chars()
        .filter(|c| !invalid_chars.contains(c))
        .collect::<String>();
    let guess = guess
        .chars()
        .filter(|c| !invalid_chars.contains(c))
        .collect::<String>();
    info!("Expecting Artist: {}", &binding);

    let dist = edit_distance(&guess, &binding.to_string());

    if dist <= threshold {
        true
    } else {
        false
    }
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

    if let Some(handler_lock) = manager.get(guild.id) {
        let mut handler = handler_lock.lock().await;

        let source = match songbird::ytdl("youtube.com/watch?v=MFw3E6X5aoA").await {
            Ok(source) => source,
            Err(why) => {
                info!("Err starting source: {:?}", why);

                check_msg(
                    interaction
                        .channel_id
                        .say(&ctx.http, "Error sourcing ffmpeg")
                        .await,
                );
                return Ok(());
            }
        };
        handler.play_source(source).set_volume(0.7).unwrap();
    }

    return Ok(());
}

struct SongEndNotifier {
    context: Context,
    guild_id: Guild,
}

#[async_trait]
impl VoiceEventHandler for SongEndNotifier {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<songbird::Event> {
        if let EventContext::Track(_track_list) = ctx {
            let manager = songbird::get(&self.context)
                .await
                .expect("Songbird Voice client placed in at initialisation.")
                .clone();
            let has_handler = manager.get(self.guild_id.id).is_some();
            if has_handler {
                manager.remove(self.guild_id.id).await.unwrap();
            }
        }
        None
    }
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

    if let Some(handler_lock) = manager.get(guild.id) {
        let mut handler = handler_lock.lock().await;

        let source = match songbird::ytdl("https://www.youtube.com/watch?v=dAqLGeXPKz4").await {
            Ok(source) => source,
            Err(why) => {
                info!("Err starting source: {:?}", why);

                check_msg(
                    interaction
                        .channel_id
                        .say(&ctx.http, "Error sourcing ffmpeg")
                        .await,
                );
                return Ok(());
            }
        };
        let song = handler.play_source(source);
        song.set_volume(1.0).unwrap();
        song.add_event(
            songbird::Event::Track(TrackEvent::End),
            SongEndNotifier {
                context: ctx.clone(),
                guild_id: guild,
            },
        )
        .unwrap();
    }

    Ok(())
}
