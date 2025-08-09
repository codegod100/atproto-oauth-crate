use async_sqlite::{
    Pool, rusqlite,
    rusqlite::{Error, Row},
};
use atrium_api::types::string::Did;
use chrono::{DateTime, Datelike, Utc};
use rusqlite::types::Type;
use serde::{Deserialize, Serialize};
use std::{fmt::Debug, sync::Arc};

/// Creates the OAuth-specific tables in the database.
/// This creates the minimal tables needed for OAuth functionality.
/// Applications should create their own schema setup function that includes these tables.
pub async fn create_oauth_tables(pool: &Pool) -> Result<(), async_sqlite::Error> {
    pool.conn(move |conn| {
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();

        // auth_session
        conn.execute(
            "CREATE TABLE IF NOT EXISTS auth_session (
            key TEXT PRIMARY KEY,
            session TEXT NOT NULL
        )",
            [],
        )
        .unwrap();

        // auth_state
        conn.execute(
            "CREATE TABLE IF NOT EXISTS auth_state (
            key TEXT PRIMARY KEY,
            state TEXT NOT NULL
        )",
            [],
        )
        .unwrap();
        Ok(())
    })
    .await?;
    Ok(())
}


/// AuthSession table data type
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthSession {
    pub key: String,
    pub session: String,
}

impl AuthSession {
    /// Creates a new [AuthSession]
    pub fn new<V>(key: String, session: V) -> Self
    where
        V: Serialize,
    {
        let session = serde_json::to_string(&session).unwrap();
        Self {
            key: key.to_string(),
            session,
        }
    }

    /// Helper to map from [Row] to [AuthSession]
    fn map_from_row(row: &Row) -> Result<Self, Error> {
        let key: String = row.get(0)?;
        let session: String = row.get(1)?;
        Ok(Self { key, session })
    }

    /// Gets a session by the users did(key)
    pub async fn get_by_did(pool: &Pool, did: String) -> Result<Option<Self>, async_sqlite::Error> {
        let did = Did::new(did).unwrap();
        pool.conn(move |conn| {
            let mut stmt = conn.prepare("SELECT * FROM auth_session WHERE key = ?1")?;
            stmt.query_row([did.as_str()], |row| Self::map_from_row(row))
                .map(Some)
                .or_else(|err| {
                    if err == Error::QueryReturnedNoRows {
                        Ok(None)
                    } else {
                        Err(err)
                    }
                })
        })
        .await
    }

    /// Saves or updates the session by its did(key)
    pub async fn save_or_update(&self, pool: &Pool) -> Result<(), async_sqlite::Error> {
        let cloned_self = self.clone();
        pool.conn(move |conn| {
            //We check to see if the session already exists, if so we need to update not insert
            let mut stmt = conn.prepare("SELECT COUNT(*) FROM auth_session WHERE key = ?1")?;
            let count: i64 = stmt.query_row([&cloned_self.key], |row| row.get(0))?;
            match count > 0 {
                true => {
                    let mut update_stmt =
                        conn.prepare("UPDATE auth_session SET session = ?2 WHERE key = ?1")?;
                    update_stmt.execute([&cloned_self.key, &cloned_self.session])?;
                    Ok(())
                }
                false => {
                    conn.execute(
                        "INSERT INTO auth_session (key, session) VALUES (?1, ?2)",
                        [&cloned_self.key, &cloned_self.session],
                    )?;
                    Ok(())
                }
            }
        })
        .await?;
        Ok(())
    }

    /// Deletes the session by did
    pub async fn delete_by_did(pool: &Pool, did: String) -> Result<(), async_sqlite::Error> {
        pool.conn(move |conn| {
            let mut stmt = conn.prepare("DELETE FROM auth_session WHERE key = ?1")?;
            stmt.execute([&did])
        })
        .await?;
        Ok(())
    }

    /// Deletes all the sessions
    pub async fn delete_all(pool: &Pool) -> Result<(), async_sqlite::Error> {
        pool.conn(move |conn| {
            let mut stmt = conn.prepare("DELETE FROM auth_session")?;
            stmt.execute([])
        })
        .await?;
        Ok(())
    }
}

/// AuthState table datatype
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthState {
    pub key: String,
    pub state: String,
}

impl AuthState {
    /// Creates a new [AuthState]
    pub fn new<V>(key: String, state: V) -> Self
    where
        V: Serialize,
    {
        let state = serde_json::to_string(&state).unwrap();
        Self {
            key: key.to_string(),
            state,
        }
    }

    /// Helper to map from [Row] to [AuthState]
    fn map_from_row(row: &Row) -> Result<Self, Error> {
        let key: String = row.get(0)?;
        let state: String = row.get(1)?;
        Ok(Self { key, state })
    }

    /// Gets a state by the users key
    pub async fn get_by_key(pool: &Pool, key: String) -> Result<Option<Self>, async_sqlite::Error> {
        pool.conn(move |conn| {
            let mut stmt = conn.prepare("SELECT * FROM auth_state WHERE key = ?1")?;
            stmt.query_row([key.as_str()], |row| Self::map_from_row(row))
                .map(Some)
                .or_else(|err| {
                    if err == Error::QueryReturnedNoRows {
                        Ok(None)
                    } else {
                        Err(err)
                    }
                })
        })
        .await
    }

    /// Saves or updates the state by its key
    pub async fn save_or_update(&self, pool: &Pool) -> Result<(), async_sqlite::Error> {
        let cloned_self = self.clone();
        pool.conn(move |conn| {
            //We check to see if the state already exists, if so we need to update
            let mut stmt = conn.prepare("SELECT COUNT(*) FROM auth_state WHERE key = ?1")?;
            let count: i64 = stmt.query_row([&cloned_self.key], |row| row.get(0))?;
            match count > 0 {
                true => {
                    let mut update_stmt =
                        conn.prepare("UPDATE auth_state SET state = ?2 WHERE key = ?1")?;
                    update_stmt.execute([&cloned_self.key, &cloned_self.state])?;
                    Ok(())
                }
                false => {
                    conn.execute(
                        "INSERT INTO auth_state (key, state) VALUES (?1, ?2)",
                        [&cloned_self.key, &cloned_self.state],
                    )?;
                    Ok(())
                }
            }
        })
        .await?;
        Ok(())
    }

    pub async fn delete_by_key(pool: &Pool, key: String) -> Result<(), async_sqlite::Error> {
        pool.conn(move |conn| {
            let mut stmt = conn.prepare("DELETE FROM auth_state WHERE key = ?1")?;
            stmt.execute([&key])
        })
        .await?;
        Ok(())
    }

    pub async fn delete_all(pool: &Pool) -> Result<(), async_sqlite::Error> {
        pool.conn(move |conn| {
            let mut stmt = conn.prepare("DELETE FROM auth_state")?;
            stmt.execute([])
        })
        .await?;
        Ok(())
    }
}
