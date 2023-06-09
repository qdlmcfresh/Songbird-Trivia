use sqlx::{types::chrono, SqlitePool};

#[derive(sqlx::FromRow, Debug)]
pub struct Game {
    pub id: i64,
    pub playlist_id: i64,
    pub game_length: i64,
    pub started_at: chrono::NaiveDateTime,
}
impl Game {
    pub fn new(
        id: i64,
        playlist_id: i64,
        game_length: i64,
        started_at: chrono::NaiveDateTime,
    ) -> Self {
        Self {
            id,
            playlist_id,
            game_length,
            started_at,
        }
    }
}
#[derive(sqlx::FromRow, Debug)]
pub struct Score {
    pub player_id: i64,
    pub game_id: i64,
    pub score: i64,
}

impl Score {
    pub fn new(player_id: i64, game_id: i64, score: i64) -> Self {
        Self {
            player_id,
            game_id,
            score,
        }
    }
}

pub async fn insert_game(
    pool: &SqlitePool,
    game: &Game,
    scores: &Vec<Score>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO games (playlist_id, game_length, started_at)
        VALUES (?, ?, ?)
        "#,
        game.playlist_id,
        game.game_length,
        game.started_at
    )
    .execute(pool)
    .await?;

    let game_id = sqlx::query!(
        r#"
        SELECT id FROM games WHERE playlist_id = ? AND started_at = ?
        "#,
        game.playlist_id,
        game.started_at
    )
    .fetch_one(pool)
    .await?
    .id;
    let mut transaction = pool.begin().await?;
    for score in scores {
        sqlx::query!(
            r#"
            INSERT INTO scores (player_id, game_id, score)
            VALUES (?, ?, ?)
            "#,
            score.player_id,
            game_id,
            score.score
        )
        .execute(&mut transaction)
        .await?;
    }
    transaction.commit().await.unwrap();
    Ok(())
}

pub async fn read_leaderboard(pool: &SqlitePool) -> Result<Vec<(u64, i32)>, sqlx::Error> {
    let leaderboard = sqlx::query!(
        r#"SELECT player_id, SUM(score) as "total_score!:i32"
    FROM scores
    GROUP BY player_id
    ORDER BY "total_score!:i32" DESC;"#,
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let leaderboard_vec = leaderboard
        .iter()
        .map(|score| (score.player_id as u64, score.total_score))
        .collect::<Vec<_>>();
    Ok(leaderboard_vec)
}
