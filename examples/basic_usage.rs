/// Long-running example showing how to use the atproto-oauth crate with a web server
mod schema;
mod templates;
mod lexicon;
mod codegen;

use atproto_oauth::{
    // Core OAuth functionality
    OAuthClientBuilder, AtprotoOAuthClient, AuthorizeOptions, CallbackParams, 
    KnownScope, Scope, Handle,
    // Database and agent types
    Agent, PoolBuilder, Pool,
    // Web framework types
    Query, State, Redirect, Router,
};
use axum::{
    // HTTP methods and JSON
    routing::{post, get},
    Json,
    // Response types
    http::{StatusCode, HeaderMap},
};
use schema::{create_tables_in_database, BlogPostFromDb};
use templates::{HomeTemplate, SuccessTemplate, ErrorTemplate, UserInfo};
use codegen::xyz::blogosphere::post::RecordData as BlogPostRecordData;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
// Removed unused import

// Enhanced app state that includes both OAuth client and database pool
#[derive(Clone)]
struct AppState {
    oauth_client: Arc<AtprotoOAuthClient>,
    db_pool: Arc<Pool>,
}

// Session data extracted from authenticated requests
#[derive(Clone, Debug)]
struct SessionData {
    did: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    atproto_oauth::env_logger::init();

    println!("🚀 Starting AT Protocol OAuth Example Server");

    // Create database connection
    let db_pool = PoolBuilder::new()
        .path("oauth_example.sqlite3")
        .open()
        .await?;

    // Create database tables - this example shows how to integrate OAuth tables 
    // with your application-specific schema. See schema.rs for implementation details.
    create_tables_in_database(&db_pool).await?;
    println!("✅ Database initialized");

    // Build OAuth client with the builder pattern
    let oauth_client = OAuthClientBuilder::new()
        .host("127.0.0.1")
        .port(3000)
        .db_pool(db_pool.clone())
        .build()?;

    println!("✅ OAuth client created successfully!");
    println!("🔗 Redirect URI: http://127.0.0.1:3000/oauth/callback");

    // Create app state with both OAuth client and database pool
    let app_state = AppState {
        oauth_client,
        db_pool: Arc::new(db_pool),
    };

    // Create router with OAuth and blog CRUD endpoints
    let app = Router::new()
        // OAuth routes
        .route("/", get(home_handler))
        .route("/login", get(login_handler))
        .route("/oauth/callback", get(callback_handler))
        // Blog CRUD routes
        .route("/api/posts", post(create_blog_post).get(list_published_posts))
        .route("/api/posts/my", get(list_my_posts))
        .route("/api/posts/:uri", get(get_blog_post).put(update_blog_post).delete(delete_blog_post))
        .with_state(app_state);

    println!("\n🌐 Server running on http://127.0.0.1:3000");
    println!("📝 Visit http://127.0.0.1:3000 to test OAuth flow");
    println!("⏹️  Press Ctrl+C to stop");

    // Run the server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// ========== Authentication Middleware ==========\n

/// Extract session data from request headers
async fn extract_session(
    headers: HeaderMap,
    State(_app_state): State<AppState>,
) -> Result<SessionData, StatusCode> {
    // In a real application, you'd validate a session token from headers
    // For this example, we'll use a simplified approach
    let session_token = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    
    // TODO: Validate session token against database
    // For now, we'll mock this by checking if it's a valid DID format
    if !session_token.starts_with("did:") {
        return Err(StatusCode::UNAUTHORIZED);
    }
    
    // For simplicity, we'll just store the DID
    // In a real application, you'd validate the session token and restore the Agent
    
    Ok(SessionData {
        did: session_token.to_string(),
    })
}

// ========== Blog CRUD API Routes ==========\n

// Request/Response DTOs
#[derive(Deserialize)]
struct CreateBlogPostRequest {
    title: String,
    content: String,
    summary: Option<String>,
    tags: Option<Vec<String>>,
    published: Option<bool>,
}

#[derive(Deserialize)]
struct UpdateBlogPostRequest {
    title: Option<String>,
    content: Option<String>,
    summary: Option<String>,
    tags: Option<Vec<String>>,
    published: Option<bool>,
}

#[derive(Serialize)]
struct BlogPostResponse {
    uri: String,
    author_did: String,
    title: String,
    content: String,
    summary: Option<String>,
    tags: Vec<String>,
    published: bool,
    created_at: String,
    updated_at: String,
    indexed_at: String,
}

#[derive(Serialize)]
struct ApiError {
    error: String,
    message: String,
}

impl From<&BlogPostFromDb> for BlogPostResponse {
    fn from(post: &BlogPostFromDb) -> Self {
        Self {
            uri: post.uri.clone(),
            author_did: post.author_did.clone(),
            title: post.title.clone(),
            content: post.content.clone(),
            summary: post.summary.clone(),
            tags: post.get_tags().unwrap_or_default(),
            published: post.published,
            created_at: post.created_at.to_rfc3339(),
            updated_at: post.updated_at.to_rfc3339(),
            indexed_at: post.indexed_at.to_rfc3339(),
        }
    }
}

async fn home_handler() -> HomeTemplate {
    HomeTemplate
}

async fn login_handler(
    Query(params): Query<std::collections::HashMap<String, String>>,
    State(app_state): State<AppState>,
) -> Result<Redirect, ErrorTemplate> {
    let handle_str = params.get("handle").ok_or_else(|| {
        ErrorTemplate {
            title: "Missing Handle".to_string(),
            handle: None,
            action: Some("start OAuth flow".to_string()),
            error: "Handle parameter required".to_string(),
        }
    })?;

    // Parse the handle
    let handle = Handle::new(handle_str.clone()).map_err(|e| {
        ErrorTemplate {
            title: "Invalid Handle".to_string(),
            handle: Some(handle_str.clone()),
            action: Some("parse handle".to_string()),
            error: e.to_string(),
        }
    })?;

    // Start OAuth flow
    match (&*app_state.oauth_client).authorize(
        &handle,
        AuthorizeOptions {
            scopes: vec![
                Scope::Known(KnownScope::Atproto),
                Scope::Known(KnownScope::TransitionGeneric),
            ],
            ..Default::default()
        },
    ).await {
        Ok(oauth_url) => {
            println!("🔄 Starting OAuth flow for handle: {}", handle_str);
            println!("🔗 Redirecting to: {}", oauth_url);
            Ok(Redirect::to(&oauth_url))
        }
        Err(e) => {
            println!("❌ OAuth error for {}: {}", handle_str, e);
            Err(ErrorTemplate {
                title: "OAuth Error".to_string(),
                handle: Some(handle_str.clone()),
                action: Some("start OAuth flow".to_string()),
                error: e.to_string(),
            })
        }
    }
}

async fn callback_handler(
    Query(params): Query<CallbackParams>,
    State(app_state): State<AppState>,
) -> Result<SuccessTemplate, ErrorTemplate> {
    println!("🔄 Processing OAuth callback");
    
    match (&*app_state.oauth_client).callback(params).await {
        Ok((session, _)) => {
            println!("✅ OAuth flow completed successfully!");
            
            // Create an agent to fetch user info
            let agent = Agent::new(session);
            
            // Try to fetch user profile to showcase the working credentials
            let user_info = match agent.did().await {
                Some(did) => {
                    println!("🔍 Fetching profile for DID: {}", did.as_str());
                    match agent
                        .api
                        .app
                        .bsky
                        .actor
                        .get_profile(
                            atrium_api::app::bsky::actor::get_profile::ParametersData {
                                actor: atrium_api::types::string::AtIdentifier::Did(did.clone()),
                            }
                            .into(),
                        )
                        .await
                    {
                        Ok(profile) => {
                            println!("✅ Successfully fetched profile for: {}", profile.handle.as_str());
                            Some(UserInfo {
                                handle: Some(profile.handle.as_str().to_string()),
                                display_name: profile.display_name.clone(),
                                did: Some(did.as_str().to_string()),
                                followers_count: profile.followers_count.map(|c| c as u32),
                                follows_count: profile.follows_count.map(|c| c as u32),
                                posts_count: profile.posts_count.map(|c| c as u32),
                                description: profile.description.clone(),
                            })
                        }
                        Err(e) => {
                            println!("⚠️  Could not fetch profile: {}", e);
                            Some(UserInfo {
                                handle: None,
                                display_name: None,
                                did: Some(did.as_str().to_string()),
                                followers_count: None,
                                follows_count: None,
                                posts_count: None,
                                description: None,
                            })
                        }
                    }
                }
                None => {
                    println!("⚠️  No DID found in session");
                    None
                }
            };

            // TODO: Demonstrate creating a sample blog post using generated codegen types
            // This would require restructuring the app state to include the database pool
            // if let Some(ref info) = user_info {
            //     if let Some(ref did) = info.did {
            //         let _ = create_sample_blog_post(&db_pool, did).await;
            //     }
            // }

            Ok(SuccessTemplate {
                user_info,
                error_message: None,
            })
        }
        Err(e) => {
            println!("❌ OAuth callback error: {}", e);
            Err(ErrorTemplate {
                title: "OAuth Callback Error".to_string(),
                handle: None,
                action: Some("process OAuth callback".to_string()),
                error: e.to_string(),
            })
        }
    }
}

/// Creates a sample blog post to demonstrate the generated codegen types
async fn create_sample_blog_post(pool: &atproto_oauth::Pool, author_did: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔬 Creating sample blog post using generated codegen types...");

    // Create a sample blog post using the generated RecordData
    let record_data = BlogPostRecordData {
        title: "Welcome to AT Protocol Blogging!".to_string(),
        content: r#"# Hello AT Protocol!

This is a sample blog post created using the **xyz.blogosphere.post** lexicon.

## Features

- ✅ **Type-safe** record creation using generated Rust types
- 🔐 **OAuth authenticated** - you're logged in with your AT Protocol identity  
- 📝 **Rich content** - Markdown support for formatting
- 🏷️ **Tags** - Categorize your posts
- 📅 **Timestamps** - Automatic created/updated tracking

## Implementation

This post was created using:

```rust
let record_data = BlogPostRecordData {
    title: "Welcome to AT Protocol Blogging!".to_string(),
    content: "...".to_string(),
    // ... other fields
};
```

The lexicon ensures type safety and validation according to the AT Protocol schema!"#.to_string(),
        summary: Some("A sample blog post demonstrating the xyz.blogosphere.post lexicon with AT Protocol OAuth integration.".to_string()),
        tags: Some(vec![
            "atproto".to_string(),
            "rust".to_string(),
            "oauth".to_string(),
            "lexicon".to_string(),
            "demo".to_string()
        ]),
        published: Some(true),
        created_at: atrium_api::types::string::Datetime::new(chrono::Utc::now().into()),
        updated_at: Some(atrium_api::types::string::Datetime::new(chrono::Utc::now().into())),
    };

    // Create a sample URI for this post
    let sample_uri = format!("at://{}/xyz.blogosphere.post/{}", author_did, "sample-post-123");
    
    // Convert to our database model
    let blog_post = BlogPostFromDb::from_codegen_record_data(
        sample_uri,
        author_did.to_string(),
        &record_data
    )?;

    // Save to database
    let pool_arc = std::sync::Arc::new(pool.clone());
    blog_post.save(&pool_arc).await?;

    println!("✅ Sample blog post created and saved to database!");
    println!("   Title: {}", blog_post.title);
    println!("   Tags: {}", blog_post.tags);
    println!("   Published: {}", blog_post.published);

    Ok(())
}

// ========== CRUD Route Handlers ==========\n

/// Create a new blog post and store it both locally and on the PDS
async fn create_blog_post(
    headers: HeaderMap,
    State(app_state): State<AppState>,
    Json(request): Json<CreateBlogPostRequest>,
) -> Result<Json<BlogPostResponse>, (StatusCode, Json<ApiError>)> {
    // Authenticate user
    let session = extract_session(headers, State(app_state.clone())).await.map_err(|_| {
        (StatusCode::UNAUTHORIZED, Json(ApiError {
            error: "unauthorized".to_string(),
            message: "Authentication required".to_string(),
        }))
    })?;

    // Generate a unique record key (rkey) for this blog post
    let rkey = format!("post-{}", chrono::Utc::now().timestamp_millis());
    let uri = format!("at://{}/xyz.blogosphere.post/{}", session.did, rkey);

    // Create BlogPostRecordData from request
    let record_data = BlogPostRecordData {
        title: request.title,
        content: request.content,
        summary: request.summary,
        tags: request.tags,
        published: request.published,
        created_at: atrium_api::types::string::Datetime::new(chrono::Utc::now().into()),
        updated_at: Some(atrium_api::types::string::Datetime::new(chrono::Utc::now().into())),
    };

    // Convert to database model
    let blog_post = BlogPostFromDb::from_codegen_record_data(
        uri.clone(),
        session.did.clone(),
        &record_data
    ).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError {
            error: "conversion_error".to_string(),
            message: format!("Failed to convert record data: {}", e),
        }))
    })?;

    // TODO: Store in PDS using AT Protocol (requires proper authenticated Agent)
    // For now, we'll just store locally in the database
    println!("📝 Creating blog post: {}", blog_post.title);
    
    // Store locally in database
    let db_pool_arc = Arc::new(app_state.db_pool.clone());
    blog_post.save(&db_pool_arc).await.map_err(|e| {
        println!("⚠️  Failed to save to local database: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError {
            error: "database_error".to_string(),
            message: format!("Failed to save to database: {}", e),
        }))
    })?;

    println!("✅ Successfully stored blog post locally");
    Ok(Json(BlogPostResponse::from(&blog_post)))
}

async fn get_blog_post(
    headers: HeaderMap,
    State(app_state): State<AppState>,
    axum::extract::Path(uri): axum::extract::Path<String>,
) -> Result<Json<BlogPostResponse>, (StatusCode, Json<ApiError>)> {
    // Authenticate user
    let _session = extract_session(headers, State(app_state.clone())).await.map_err(|_| {
        (StatusCode::UNAUTHORIZED, Json(ApiError {
            error: "unauthorized".to_string(),
            message: "Authentication required".to_string(),
        }))
    })?;

    // Load the specific post from database
    // We need to create a method to load a post by URI
    let db_pool_arc = Arc::new(app_state.db_pool.clone());
    
    // For now, let's load all posts and filter (this should be optimized)
    let posts = BlogPostFromDb::load_latest_posts(&db_pool_arc).await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError {
                error: "database_error".to_string(),
                message: format!("Failed to load posts: {}", e),
            }))
        })?;

    // Find the post with the matching URI
    if let Some(post) = posts.into_iter().find(|p| p.uri == uri) {
        Ok(Json(BlogPostResponse::from(&post)))
    } else {
        Err((StatusCode::NOT_FOUND, Json(ApiError {
            error: "not_found".to_string(),
            message: "Blog post not found".to_string(),
        })))
    }
}

/// Update an existing blog post
async fn update_blog_post(
    headers: HeaderMap,
    State(app_state): State<AppState>,
    axum::extract::Path(uri): axum::extract::Path<String>,
    Json(request): Json<UpdateBlogPostRequest>,
) -> Result<Json<BlogPostResponse>, (StatusCode, Json<ApiError>)> {
    // Authenticate user
    let session = extract_session(headers, State(app_state.clone())).await.map_err(|_| {
        (StatusCode::UNAUTHORIZED, Json(ApiError {
            error: "unauthorized".to_string(),
            message: "Authentication required".to_string(),
        }))
    })?;

    // Load the existing post from database
    let db_pool_arc = Arc::new(app_state.db_pool.clone());
    let posts = BlogPostFromDb::load_latest_posts(&db_pool_arc).await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError {
                error: "database_error".to_string(),
                message: format!("Failed to load posts: {}", e),
            }))
        })?;

    // Find the post with the matching URI
    let existing_post = posts.into_iter().find(|p| p.uri == uri)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(ApiError {
            error: "not_found".to_string(),
            message: "Blog post not found".to_string(),
        })))?;

    // Check if user is authorized to update this post
    if existing_post.author_did != session.did {
        return Err((StatusCode::FORBIDDEN, Json(ApiError {
            error: "forbidden".to_string(),
            message: "You are not authorized to update this post".to_string(),
        })));
    }

    // Convert existing post to record data for updating
    let mut record_data = existing_post.to_codegen_record_data()
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError {
                error: "conversion_error".to_string(),
                message: format!("Failed to convert existing post: {}", e),
            }))
        })?;

    // Apply updates from request
    if let Some(title) = request.title {
        record_data.title = title;
    }
    if let Some(content) = request.content {
        record_data.content = content;
    }
    if let Some(summary) = request.summary {
        record_data.summary = Some(summary);
    }
    if let Some(tags) = request.tags {
        record_data.tags = Some(tags);
    }
    if let Some(published) = request.published {
        record_data.published = Some(published);
    }
    
    // Update timestamps
    record_data.updated_at = Some(atrium_api::types::string::Datetime::new(chrono::Utc::now().into()));

    // Convert back to database model
    let updated_post = BlogPostFromDb::from_codegen_record_data(
        uri.clone(),
        session.did.clone(),
        &record_data
    ).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError {
            error: "conversion_error".to_string(),
            message: format!("Failed to convert updated record data: {}", e),
        }))
    })?;

    // Save updated post to database
    updated_post.save_or_update(&app_state.db_pool).await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError {
                error: "database_error".to_string(),
                message: format!("Failed to update post: {}", e),
            }))
        })?;

    println!("✅ Successfully updated blog post: {}", updated_post.title);
    Ok(Json(BlogPostResponse::from(&updated_post)))
}

/// Delete a blog post
async fn delete_blog_post(
    headers: HeaderMap,
    State(app_state): State<AppState>,
    axum::extract::Path(uri): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    // Authenticate user
    let session = extract_session(headers, State(app_state.clone())).await.map_err(|_| {
        (StatusCode::UNAUTHORIZED, Json(ApiError {
            error: "unauthorized".to_string(),
            message: "Authentication required".to_string(),
        }))
    })?;

    // Load the existing post from database
    let db_pool_arc = Arc::new(app_state.db_pool.clone());
    let posts = BlogPostFromDb::load_latest_posts(&db_pool_arc).await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError {
                error: "database_error".to_string(),
                message: format!("Failed to load posts: {}", e),
            }))
        })?;

    // Find the post with the matching URI
    let existing_post = posts.into_iter().find(|p| p.uri == uri)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(ApiError {
            error: "not_found".to_string(),
            message: "Blog post not found".to_string(),
        })))?;

    // Check if user is authorized to delete this post
    if existing_post.author_did != session.did {
        return Err((StatusCode::FORBIDDEN, Json(ApiError {
            error: "forbidden".to_string(),
            message: "You are not authorized to delete this post".to_string(),
        })));
    }

    // Delete the post from database
    BlogPostFromDb::delete_by_uri(&app_state.db_pool, uri.clone()).await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError {
                error: "database_error".to_string(),
                message: format!("Failed to delete post: {}", e),
            }))
        })?;

    println!("✅ Successfully deleted blog post: {}", existing_post.title);
    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Post deleted successfully"
    })))
}

/// List all blog posts for the authenticated user
async fn list_my_posts(
    headers: HeaderMap,
    State(app_state): State<AppState>,
) -> Result<Json<Vec<BlogPostResponse>>, (StatusCode, Json<ApiError>)> {
    // Authenticate user
    let session = extract_session(headers, State(app_state.clone())).await.map_err(|_| {
        (StatusCode::UNAUTHORIZED, Json(ApiError {
            error: "unauthorized".to_string(),
            message: "Authentication required".to_string(),
        }))
    })?;

    // Load user's latest posts from database
    let posts = BlogPostFromDb::my_latest_post(&app_state.db_pool, &session.did).await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError {
                error: "database_error".to_string(),
                message: format!("Failed to load posts: {}", e),
            }))
        })?;

    // Convert to response format
    let responses = if let Some(post) = posts {
        vec![BlogPostResponse::from(&post)]
    } else {
        vec![]
    };

    Ok(Json(responses))
}

/// List published blog posts (public endpoint)
async fn list_published_posts(
    State(app_state): State<AppState>,
) -> Result<Json<Vec<BlogPostResponse>>, (StatusCode, Json<ApiError>)> {
    // Load published posts from database
    let posts = BlogPostFromDb::load_published_posts(&app_state.db_pool).await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError {
                error: "database_error".to_string(),
                message: format!("Failed to load published posts: {}", e),
            }))
        })?;

    // Convert to response format
    let responses: Vec<BlogPostResponse> = posts.iter().map(|p| BlogPostResponse::from(p)).collect();

    Ok(Json(responses))
}