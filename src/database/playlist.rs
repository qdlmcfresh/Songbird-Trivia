use sqlx::{types::chrono, SqlitePool};
#[derive(sqlx::FromRow, Debug)]
pub struct Playlist {
    pub id: i64,
    pub spotify_id: String,
    pub name: String,
    pub amount_songs: i64,
    pub last_update: chrono::NaiveDateTime,
}
impl Playlist {
    pub fn new(
        id: i64,
        spotify_id: String,
        name: String,
        amount_songs: i64,
        last_update: chrono::NaiveDateTime,
    ) -> Self {
        Self {
            id,
            spotify_id,
            name,
            amount_songs,
            last_update,
        }
    }
}

pub async fn insert_playlist(pool: &SqlitePool, playlist: &Playlist) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO playlists (spotify_id, name, amount_songs)
        VALUES (?, ?, ?)
        "#,
        playlist.spotify_id,
        playlist.name,
        playlist.amount_songs
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn read_playlists(pool: &SqlitePool) -> Result<Vec<Playlist>, sqlx::Error> {
    let playlists = sqlx::query_as!(
        Playlist,
        r#"
        SELECT id, spotify_id, name, amount_songs, last_update FROM playlists
        ORDER BY last_update DESC
        "#
    )
    .fetch_all(pool)
    .await?;
    Ok(playlists)
}

pub async fn read_playlist_id(pool: &SqlitePool, spotify_id: &str) -> Result<i64, sqlx::Error> {
    let playlist_id = sqlx::query!(
        r#"
        SELECT id FROM playlists WHERE spotify_id = ?
        "#,
        spotify_id
    )
    .fetch_one(pool)
    .await?;
    Ok(playlist_id.id)
}
