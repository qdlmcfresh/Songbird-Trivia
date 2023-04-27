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
    for song in songs {
        match sqlx::query!(
            r#"
            INSERT INTO songs (spotify_id, song_name, artist_name, preview_url)
            VALUES (?, ?, ?, ?)
            ON CONFLICT (spotify_id) DO NOTHING
            "#,
            song.spotify_id,
            song.song_name,
            song.artist_name,
            song.preview_url,
        )
        .execute(pool)
        .await
        {
            Ok(_) => {}
            Err(e) => {
                println!("Error adding Song {}\n {}", song.spotify_id, e.to_string())
            }
        }
        let song_id = sqlx::query!(
            r#"
            SELECT id FROM songs WHERE spotify_id = ?
            "#,
            song.spotify_id
        )
        .fetch_one(pool)
        .await?
        .id;

        match sqlx::query!(
            r#"
            INSERT INTO playlist_songs (playlist_id, song_id)
            VALUES (?, ?)
            "#,
            playlist_id,
            song_id
        )
        .execute(pool)
        .await
        {
            Ok(_) => {}
            Err(e) => {
                println!(
                    "Song {} in Playlist {} already exists \n {}",
                    song.id,
                    playlist_id,
                    e.to_string()
                )
            }
        }
    }
    Ok(())
}
