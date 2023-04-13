use sqlx::types::chrono;


#[derive(Debug)]
pub struct Playlist {
    pub id : i64,
    pub playlist_url: String,
    pub playlist_name :String,
    pub amount_songs: i64,
    pub added_by: String,
    pub added_at: chrono::NaiveDateTime,
    pub last_update: chrono::NaiveDateTime
}
impl Playlist{
    pub fn new(id : i64, playlist_url: String, playlist_name :String, amount_songs: i64, added_by: String, added_at: chrono::NaiveDateTime, last_update: chrono::NaiveDateTime) -> Self{
        Self{
            id,
            playlist_url,
            playlist_name,
            amount_songs,
            added_by,
            added_at,
            last_update
        }
    }
}
#[derive(Clone)]
pub struct Song{
    pub preview_url: String,
    pub song_name: String,
    pub artist_name: String,
    pub album_name: String,
}
impl Song{
    pub fn new(preview_url: String, song_name: String, artist_name: String, album_name: String) -> Self{
        Self{
            preview_url,
            song_name,
            artist_name,
            album_name,
        }
    }
}
pub struct Genre{
    id: u32,
    name: String
}
impl Genre{
    pub fn new(id: u32, name: String) -> Self{
        Self{
            id,
            name
        }
    }
}

pub struct GameResult{
    id:u32,
    playlist_id: u32,
    amount_songs: u16,
    length: u32
}
impl GameResult{
    pub fn new(id:u32, playlist_id: u32, amount_songs: u16, length: u32) -> Self{
        Self{
            id,
            playlist_id,
            amount_songs,
            length
        }
    }
}

pub struct Ranking{
    game_id: u32,
    user_id: u32,
    score: u16,
}
impl Ranking{
    pub fn new(game_id: u32, user_id: u32, score: u16) -> Self{
        Self{
            game_id,
            user_id,
            score
        }
    }
}