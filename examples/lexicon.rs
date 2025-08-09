/// Lexicon types for xyz.blogosphere.post
/// 
/// This module contains Rust types that correspond to the AT Protocol lexicon
/// defined in lexicons/status.json. These types provide type-safe interaction
/// with blog post records.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Blog post record as defined by xyz.blogosphere.post lexicon
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlogPostRecord {
    /// The title of the blog post (1-200 characters)
    pub title: String,
    /// The main content of the blog post in markdown (1-10000 characters)
    pub content: String,
    /// Optional summary/excerpt of the post (max 500 characters)
    pub summary: Option<String>,
    /// Tags for categorizing the post (max 10 tags, each max 50 characters)
    pub tags: Vec<String>,
    /// Whether the post is published or draft
    pub published: bool,
    /// When the post was created
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    /// When the post was last updated
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
}

impl BlogPostRecord {
    /// Create a new blog post record
    pub fn new(title: String, content: String) -> Result<Self, String> {
        let mut post = Self {
            title,
            content,
            summary: None,
            tags: Vec::new(),
            published: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        
        post.validate()?;
        Ok(post)
    }

    /// Add a tag to the post
    pub fn add_tag(&mut self, tag: String) -> Result<(), String> {
        if tag.len() > 50 {
            return Err("Tag cannot be longer than 50 characters".to_string());
        }
        if self.tags.len() >= 10 {
            return Err("Cannot have more than 10 tags".to_string());
        }
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
        }
        Ok(())
    }

    /// Set the summary for the post
    pub fn set_summary(&mut self, summary: String) -> Result<(), String> {
        if summary.len() > 500 {
            return Err("Summary cannot be longer than 500 characters".to_string());
        }
        self.summary = Some(summary);
        Ok(())
    }

    /// Publish the post
    pub fn publish(&mut self) {
        self.published = true;
        self.updated_at = Utc::now();
    }

    /// Mark as draft
    pub fn unpublish(&mut self) {
        self.published = false;
        self.updated_at = Utc::now();
    }

    /// Validate the blog post record according to the lexicon
    pub fn validate(&self) -> Result<(), String> {
        if self.title.is_empty() || self.title.len() > 200 {
            return Err("Title must be between 1 and 200 characters".to_string());
        }
        
        if self.content.is_empty() || self.content.len() > 10000 {
            return Err("Content must be between 1 and 10000 characters".to_string());
        }
        
        if let Some(ref summary) = self.summary {
            if summary.len() > 500 {
                return Err("Summary cannot be longer than 500 characters".to_string());
            }
        }
        
        if self.tags.len() > 10 {
            return Err("Cannot have more than 10 tags".to_string());
        }
        
        for tag in &self.tags {
            if tag.len() > 50 {
                return Err("Each tag cannot be longer than 50 characters".to_string());
            }
        }
        
        Ok(())
    }
}