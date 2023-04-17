use std::sync::Arc;

use rspotify::{
    model::{PlayableItem, PlaylistId},
    prelude::BaseClient,
    ClientCredsSpotify, ClientError,
};
use serenity::{model::channel::Message, Result as SerenityResult};
use sqlx::{sqlite::SqliteQueryResult, types::chrono::NaiveDateTime, Pool, Sqlite};

use crate::structs::{Playlist, Song};
pub fn check_msg(result: SerenityResult<Message>) {
    if let Err(why) = result {
        println!("Error sending message: {:?}", why);
    }
}

const PLAYLIST_ID_REGEX: &str = r"/playlist/(.+)\?";

pub fn get_id_from_url(url: &str) -> String {
    regex::Regex::new(PLAYLIST_ID_REGEX)
        .unwrap()
        .captures(url)
        .unwrap()[1]
        .to_string()
}

pub async fn get_playlist_data(spotify: &Arc<ClientCredsSpotify>, url: String) -> Playlist {
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

pub async fn get_tracks(
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

pub async fn add_playlist_to_db(
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
pub async fn read_playlists_from_db(db: Pool<Sqlite>) -> Result<Vec<Playlist>, sqlx::Error> {
    let playlists = sqlx::query_as!(Playlist, "SELECT * FROM playlists")
        .fetch_all(&db)
        .await;
    playlists
}

const PLAYLIST_VALID_REGEX: &str =
    r"^(https?://)?(www\.)?(open\.)?spotify\.com/playlist/[a-zA-Z0-9]+(\?.*)*$";

pub fn validate_url(url: &String) -> bool {
    regex::Regex::new(PLAYLIST_VALID_REGEX)
        .unwrap()
        .is_match(&url)
}
