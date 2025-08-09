/// Example database schema implementation showing how to integrate OAuth tables
/// with your application-specific tables using generated lexicon types.
use async_sqlite::{
    Pool, rusqlite,
    rusqlite::{Error, Row},
};
use chrono::{DateTime, Datelike, Utc};
use rusqlite::types::Type;
use serde::{Deserialize, Serialize};
use std::{fmt::Debug, sync::Arc};

// Import the generated codegen types
use crate::codegen::com::crabdance::nandi::post::{Record as BlogPostRecord, RecordData as BlogPostRecordData};
use crate::codegen::record::KnownRecord;

/// Creates all tables needed for this example application.
/// This shows how to combine OAuth tables with your own application schema.
pub async fn create_tables_in_database(pool: &Pool) -> Result<(), async_sqlite::Error> {
    pool.conn(move |conn| {
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();

        // OAuth tables - these are required for the OAuth functionality
        conn.execute(
            "CREATE TABLE IF NOT EXISTS auth_session (
            key TEXT PRIMARY KEY,
            session TEXT NOT NULL
        )",
            [],
        )
        .unwrap();

        conn.execute(
            "CREATE TABLE IF NOT EXISTS auth_state (
            key TEXT PRIMARY KEY,
            state TEXT NOT NULL
        )",
            [],
        )
        .unwrap();

        // Application-specific tables - this is an example of your own schema
        conn.execute(
            "CREATE TABLE IF NOT EXISTS blog_posts (
            uri TEXT PRIMARY KEY,
            authorDid TEXT NOT NULL,
            title TEXT NOT NULL,
            content TEXT NOT NULL,
            summary TEXT,
            tags TEXT NOT NULL DEFAULT '[]',
            published BOOLEAN NOT NULL DEFAULT 0,
            createdAt INTEGER NOT NULL,
            updatedAt INTEGER NOT NULL,
            indexedAt INTEGER NOT NULL
        )",
            [],
        )
        .unwrap();
        
        Ok(())
    })
    .await?;
    Ok(())
}

/// Example application-specific model - Blog Post table datatype
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BlogPostFromDb {
    pub uri: String,
    pub author_did: String,
    pub title: String,
    pub content: String,
    pub summary: Option<String>,
    pub tags: String, // JSON serialized array
    pub published: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub indexed_at: DateTime<Utc>,
    pub handle: Option<String>,
}

impl BlogPostFromDb {
    /// Creates a new [BlogPostFromDb] from lexicon record
    pub fn new(uri: String, author_did: String, title: String, content: String) -> Self {
        let now = chrono::Utc::now();
        Self {
            uri,
            author_did,
            title,
            content,
            summary: None,
            tags: "[]".to_string(),
            published: false,
            created_at: now,
            updated_at: now,
            indexed_at: now,
            handle: None,
        }
    }

    /// Create from generated codegen BlogPostRecord
    pub fn from_codegen_record(uri: String, author_did: String, record: &BlogPostRecord) -> Result<Self, serde_json::Error> {
        let tags_json = serde_json::to_string(&record.data.tags.as_ref().unwrap_or(&vec![]))?;
        
        Ok(Self {
            uri,
            author_did,
            title: record.data.title.clone(),
            content: record.data.content.clone(),
            summary: record.data.summary.clone(),
            tags: tags_json,
            published: record.data.published.unwrap_or(false),
            created_at: (*record.data.created_at.as_ref()).into(),
            updated_at: record.data.updated_at.as_ref().map(|dt| (*dt.as_ref()).into()).unwrap_or_else(|| chrono::Utc::now()),
            indexed_at: chrono::Utc::now(),
            handle: None,
        })
    }

    /// Create from generated BlogPostRecordData
    pub fn from_codegen_record_data(uri: String, author_did: String, data: &BlogPostRecordData) -> Result<Self, serde_json::Error> {
        let tags_json = serde_json::to_string(&data.tags.as_ref().unwrap_or(&vec![]))?;
        
        Ok(Self {
            uri,
            author_did,
            title: data.title.clone(),
            content: data.content.clone(),
            summary: data.summary.clone(),
            tags: tags_json,
            published: data.published.unwrap_or(false),
            created_at: (*data.created_at.as_ref()).into(),
            updated_at: data.updated_at.as_ref().map(|dt| (*dt.as_ref()).into()).unwrap_or_else(|| chrono::Utc::now()),
            indexed_at: chrono::Utc::now(),
            handle: None,
        })
    }

    /// Convert to generated BlogPostRecordData
    pub fn to_codegen_record_data(&self) -> Result<BlogPostRecordData, serde_json::Error> {
        let tags: Vec<String> = serde_json::from_str(&self.tags)?;
        
        Ok(BlogPostRecordData {
            title: self.title.clone(),
            content: self.content.clone(),
            summary: self.summary.clone(),
            tags: if tags.is_empty() { None } else { Some(tags) },
            published: Some(self.published),
            created_at: atrium_api::types::string::Datetime::new(self.created_at.into()),
            updated_at: Some(atrium_api::types::string::Datetime::new(self.updated_at.into())),
        })
    }

    /// Convert to KnownRecord for AT Protocol operations
    pub fn to_known_record(&self) -> Result<KnownRecord, serde_json::Error> {
        let record_data = self.to_codegen_record_data()?;
        Ok(KnownRecord::from(record_data))
    }

    /// Helper to map from [Row] to [BlogPostFromDb]
    fn map_from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            uri: row.get(0)?,
            author_did: row.get(1)?,
            title: row.get(2)?,
            content: row.get(3)?,
            summary: row.get(4)?,
            tags: row.get(5)?,
            published: row.get(6)?,
            //DateTimes are stored as INTEGERS then parsed into a DateTime<UTC>
            created_at: {
                let timestamp: i64 = row.get(7)?;
                DateTime::from_timestamp(timestamp, 0).ok_or_else(|| {
                    Error::InvalidColumnType(7, "Invalid timestamp".parse().unwrap(), Type::Text)
                })?
            },
            updated_at: {
                let timestamp: i64 = row.get(8)?;
                DateTime::from_timestamp(timestamp, 0).ok_or_else(|| {
                    Error::InvalidColumnType(8, "Invalid timestamp".parse().unwrap(), Type::Text)
                })?
            },
            indexed_at: {
                let timestamp: i64 = row.get(9)?;
                DateTime::from_timestamp(timestamp, 0).ok_or_else(|| {
                    Error::InvalidColumnType(9, "Invalid timestamp".parse().unwrap(), Type::Text)
                })?
            },
            handle: None,
        })
    }

    /// Parse tags from JSON string
    pub fn get_tags(&self) -> Result<Vec<String>, serde_json::Error> {
        serde_json::from_str(&self.tags)
    }

    /// Helper for the UI to see if indexed_at date is today or not
    pub fn is_today(&self) -> bool {
        let now = Utc::now();

        self.indexed_at.day() == now.day()
            && self.indexed_at.month() == now.month()
            && self.indexed_at.year() == now.year()
    }

    /// Saves the [BlogPostFromDb]
    pub async fn save(&self, pool: &Arc<Pool>) -> Result<(), async_sqlite::Error> {
        let cloned_self = self.clone();
        pool.conn(move |conn| {
            Ok(conn.execute(
                "INSERT INTO blog_posts (uri, authorDid, title, content, summary, tags, published, createdAt, updatedAt, indexedAt) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                [
                    &cloned_self.uri,
                    &cloned_self.author_did,
                    &cloned_self.title,
                    &cloned_self.content,
                    &cloned_self.summary.unwrap_or_default(),
                    &cloned_self.tags,
                    &(if cloned_self.published { "1" } else { "0" }).to_string(),
                    &cloned_self.created_at.timestamp().to_string(),
                    &cloned_self.updated_at.timestamp().to_string(),
                    &cloned_self.indexed_at.timestamp().to_string(),
                ],
            )?)
        })
            .await?;
        Ok(())
    }

    /// Saves or updates a blog post by its uri
    pub async fn save_or_update(&self, pool: &Pool) -> Result<(), async_sqlite::Error> {
        let cloned_self = self.clone();
        pool.conn(move |conn| {
            //We check to see if the post already exists, if so we need to update not insert
            let mut stmt = conn.prepare("SELECT COUNT(*) FROM blog_posts WHERE uri = ?1")?;
            let count: i64 = stmt.query_row([&cloned_self.uri], |row| row.get(0))?;
            match count > 0 {
                true => {
                    let mut update_stmt = conn.prepare("UPDATE blog_posts SET title = ?2, content = ?3, summary = ?4, tags = ?5, published = ?6, updatedAt = ?7, indexedAt = ?8 WHERE uri = ?1")?;
                    update_stmt.execute([
                        &cloned_self.uri,
                        &cloned_self.title,
                        &cloned_self.content,
                        &cloned_self.summary.unwrap_or_default(),
                        &cloned_self.tags,
                        &(if cloned_self.published { "1" } else { "0" }).to_string(),
                        &cloned_self.updated_at.timestamp().to_string(),
                        &cloned_self.indexed_at.timestamp().to_string(),
                    ])?;
                    Ok(())
                }
                false => {
                    conn.execute(
                        "INSERT INTO blog_posts (uri, authorDid, title, content, summary, tags, published, createdAt, updatedAt, indexedAt) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        [
                            &cloned_self.uri,
                            &cloned_self.author_did,
                            &cloned_self.title,
                            &cloned_self.content,
                            &cloned_self.summary.unwrap_or_default(),
                            &cloned_self.tags,
                            &(if cloned_self.published { "1" } else { "0" }).to_string(),
                            &cloned_self.created_at.timestamp().to_string(),
                            &cloned_self.updated_at.timestamp().to_string(),
                            &cloned_self.indexed_at.timestamp().to_string(),
                        ],
                    )?;
                    Ok(())
                }
            }
        })
        .await?;
        Ok(())
    }

    pub async fn delete_by_uri(pool: &Pool, uri: String) -> Result<(), async_sqlite::Error> {
        pool.conn(move |conn| {
            let mut stmt = conn.prepare("DELETE FROM blog_posts WHERE uri = ?1")?;
            stmt.execute([&uri])
        })
        .await?;
        Ok(())
    }

    /// Loads the last 10 blog posts we have saved
    pub async fn load_latest_posts(
        pool: &Arc<Pool>,
    ) -> Result<Vec<Self>, async_sqlite::Error> {
        Ok(pool
            .conn(move |conn| {
                let mut stmt =
                    conn.prepare("SELECT * FROM blog_posts ORDER BY indexedAt DESC LIMIT 10")?;
                let posts_iter = stmt
                    .query_map([], |row| Ok(Self::map_from_row(row).unwrap()))
                    .unwrap();

                let mut posts = Vec::new();
                for post in posts_iter {
                    posts.push(post?);
                }
                Ok(posts)
            })
            .await?)
    }

    /// Loads only published blog posts
    pub async fn load_published_posts(
        pool: &Arc<Pool>,
    ) -> Result<Vec<Self>, async_sqlite::Error> {
        Ok(pool
            .conn(move |conn| {
                let mut stmt =
                    conn.prepare("SELECT * FROM blog_posts WHERE published = 1 ORDER BY createdAt DESC LIMIT 20")?;
                let posts_iter = stmt
                    .query_map([], |row| Ok(Self::map_from_row(row).unwrap()))
                    .unwrap();

                let mut posts = Vec::new();
                for post in posts_iter {
                    posts.push(post?);
                }
                Ok(posts)
            })
            .await?)
    }

    /// Loads the logged-in user's latest blog post
    pub async fn my_latest_post(
        pool: &Arc<Pool>,
        did: &str,
    ) -> Result<Option<Self>, async_sqlite::Error> {
        let did = did.to_string();
        pool.conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT * FROM blog_posts WHERE authorDid = ?1 ORDER BY createdAt DESC LIMIT 1",
            )?;
            stmt.query_row([did.as_str()], |row| Self::map_from_row(row))
                .map(Some)
                .or_else(|err| {
                    if err == rusqlite::Error::QueryReturnedNoRows {
                        Ok(None)
                    } else {
                        Err(err)
                    }
                })
        })
        .await
    }

    /// Load a specific blog post by URI
    pub async fn load_by_uri(
        pool: &Arc<Pool>,
        uri: &str,
    ) -> Result<Option<Self>, async_sqlite::Error> {
        let uri = uri.to_string();
        pool.conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT * FROM blog_posts WHERE uri = ?1",
            )?;
            stmt.query_row([uri.as_str()], |row| Self::map_from_row(row))
                .map(Some)
                .or_else(|err| {
                    if err == rusqlite::Error::QueryReturnedNoRows {
                        Ok(None)
                    } else {
                        Err(err)
                    }
                })
        })
        .await
    }

    /// UI helper to show a handle or DID if the handle cannot be found
    pub fn author_display_name(&self) -> String {
        match self.handle.as_ref() {
            Some(handle) => handle.to_string(),
            None => self.author_did.to_string(),
        }
    }

    /// Get a truncated summary for display
    pub fn display_summary(&self) -> String {
        if let Some(ref summary) = self.summary {
            if summary.len() > 100 {
                format!("{}...", &summary[..100])
            } else {
                summary.clone()
            }
        } else {
            // Generate summary from content
            let content_preview = if self.content.len() > 150 {
                format!("{}...", &self.content[..150])
            } else {
                self.content.clone()
            };
            content_preview
        }
    }
}