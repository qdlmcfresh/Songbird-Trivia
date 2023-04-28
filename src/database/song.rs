use sqlx::SqlitePool;

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct Song {
    pub id: i64,
    pub spotify_id: String,
    pub song_name: String,
    pub artist_name: String,
    pub preview_url: String,
}
impl Song {
    pub fn new(
        id: i64,
        spotify_id: String,
        song_name: String,
        artist_name: String,
        preview_url: String,
    ) -> Self {
        Self {
            id,
            spotify_id,
            song_name,
            artist_name,
            preview_url,
        }
    }
    pub fn get_url(&self) -> String {
        format!(
            "https://open.spotify.com/track/{}",
            self.spotify_id.split(":").last().unwrap()
        )
    }
}

pub async fn read_songs(pool: &SqlitePool, playlist_id: i64) -> Result<Vec<Song>, sqlx::Error> {
    let songs = sqlx::query_as!(
        Song,
        r#"
        SELECT songs.id, songs.spotify_id, songs.song_name, songs.artist_name, songs.preview_url
        FROM songs
        INNER JOIN playlist_songs ON playlist_songs.song_id = songs.id
        WHERE playlist_songs.playlist_id = ?
        "#,
        playlist_id
    )
    .fetch_all(pool)
    .await?;
    Ok(songs)
}

pub async fn insert_songs(
    pool: &SqlitePool,
    songs: &Vec<Song>,
    playlist_id: i64,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    for song in songs {
        println!("{:?}\n{}", song, playlist_id);
        sqlx::query!(
            r#"
            INSERT OR IGNORE INTO songs (spotify_id, song_name, artist_name, preview_url)
            VALUES(?, ?, ?, ?);
            INSERT OR IGNORE INTO playlist_songs(playlist_id, song_id)
            VALUES(?, (SELECT id FROM songs WHERE spotify_id = ?));
            "#,
            song.spotify_id,
            song.song_name,
            song.artist_name,
            song.preview_url,
            playlist_id,
            song.spotify_id
        )
        .execute(&mut tx)
        .await
        .unwrap();
    }
    tx.commit().await?;
    Ok(())
}
