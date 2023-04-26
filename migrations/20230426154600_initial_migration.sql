CREATE TABLE IF NOT EXISTS playlists
(
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    spotify_id VARCHAR(50) NOT NULL,
    name VARCHAR(255) NOT NULL,
    amount_songs INTEGER NOT NULL,
    last_update TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(spotify_id)
);
CREATE TABLE IF NOT EXISTS songs
(
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    spotify_id VARCHAR(50) NOT NULL,
    song_name VARCHAR(255) NOT NULL,
    artist_name VARCHAR(255) NOT NULL,
    preview_url VARCHAR(255) NOT NULL,
    UNIQUE(spotify_id)
);

CREATE TABLE IF NOT EXISTS playlist_songs
(
    playlist_id INTEGER NOT NULL,
    song_id INTEGER NOT NULL,
    PRIMARY KEY (playlist_id, song_id),
    FOREIGN KEY(playlist_id) REFERENCES playlists(id),
    FOREIGN KEY(song_id) REFERENCES songs(id)
);

CREATE TABLE IF NOT EXISTS games
(
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    playlist_id INTEGER NOT NULL,
    game_length INTEGER NOT NULL,
    started_at TIMESTAMP NOT NULL,
    FOREIGN KEY(playlist_id) REFERENCES playlists(id)
);
CREATE TABLE IF NOT EXISTS scores
(
    player_id INTEGER NOT NULL,
    game_id INTEGER NOT NULL,
    score INTEGER NOT NULL,
    FOREIGN KEY(game_id) REFERENCES games(id)
);

