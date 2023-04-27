use std::sync::Arc;

use rspotify::{
    model::{PlayableItem, PlaylistId},
    prelude::BaseClient,
    ClientCredsSpotify, ClientError,
};
use sqlx::types::chrono;

use crate::database::{playlist::Playlist, song::Song};

const PLAYLIST_ID_REGEX: &str = r"/playlist/(.{22})";

pub fn get_id_from_url(url: &str) -> String {
    println!("Trying to get id from url: {}", url);
    regex::Regex::new(PLAYLIST_ID_REGEX)
        .unwrap()
        .captures(url)
        .unwrap()[1]
        .to_string()
}

pub async fn get_playlist_data(
    spotify: &Arc<ClientCredsSpotify>,
    url: String,
) -> Result<Playlist, ()> {
    let id_from_url = get_id_from_url(&url);
    spotify.auto_reauth().await.unwrap();
    println!("{}", id_from_url);
    let playlist_id = PlaylistId::from_id(id_from_url).unwrap();

    let playlist = spotify.playlist(playlist_id, None, None).await;

    match playlist {
        Ok(p) => {
            return Ok(Playlist::new(
                0,
                p.id.to_string(),
                p.name,
                p.tracks.total as i64,
                chrono::NaiveDateTime::default(),
            ));
        }
        Err(_) => {
            return Err(());
        }
    }
}

pub async fn get_tracks(
    spotify: &Arc<ClientCredsSpotify>,
    uri: String,
) -> Result<Vec<Song>, ClientError> {
    spotify.auto_reauth().await.unwrap();
    let mut tracks = Vec::new();
    let playlist_id = PlaylistId::from_id_or_uri(&uri).unwrap();
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
                0,
                full_track.id.unwrap().to_string(),
                full_track.name,
                artits_string,
                full_track.preview_url.unwrap(),
            ));
        }
        offset += limit;
        if pl.next.is_none() {
            break;
        }
    }
    Ok(tracks)
}

const PLAYLIST_VALID_REGEX: &str =
    r"^(https?://)?(www\.)?(open\.)?spotify\.com/playlist/[a-zA-Z0-9]+(\?.*)*$";

pub fn validate_url(url: &String) -> bool {
    regex::Regex::new(PLAYLIST_VALID_REGEX)
        .unwrap()
        .is_match(&url)
}
