/// Long-running example showing how to use the atproto-oauth crate with a web server
mod schema;
mod templates;
mod lexicon;
mod codegen;

use atproto_oauth::{
    // Core OAuth functionality
    OAuthClientBuilder, AtprotoOAuthClient, AuthorizeOptions, CallbackParams, 
    KnownScope, Scope, Handle, Did,
    // Database and agent types
    Agent, PoolBuilder, Pool,
    // Storage types - not needed anymore
    // Web framework types
    Query, State, Redirect, Router,
};
use atrium_api::types::{TryIntoUnknown, string::{Nsid, RecordKey}};
use atrium_api::agent::SessionManager;
use axum::{
    // HTTP methods and JSON
    routing::{post, get},
    Json,
    // Response types
    http::{StatusCode, HeaderMap},
    response::{Response, Html},
    // Form handling
    extract::Form,
};
use schema::{create_tables_in_database, BlogPostFromDb};
use templates::{HomeTemplate, SuccessTemplate, ErrorTemplate, UserInfo, BlogListTemplate, BlogCreateTemplate, BlogEditTemplate, BlogViewTemplate, BlogDeleteTemplate, BlogPostInfo};
use askama::Template;
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
        // Blog form routes (HTML interface)
        .route("/blog", get(blog_list_handler))
        .route("/blog/new", get(blog_create_form_handler))
        .route("/blog/create", post(blog_create_form_handler_post))
        .route("/blog/view/:uri", get(blog_view_handler))
        .route("/blog/edit/:uri", get(blog_edit_form_handler))
        .route("/blog/update/:uri", post(blog_edit_form_handler_post))
        .route("/blog/delete/:uri", get(blog_delete_form_handler))
        .route("/blog/delete/:uri", post(blog_delete_form_handler_post))
        // Blog CRUD API routes
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

// ========== Authentication Middleware ==========

/// Session data extracted from authenticated requests
#[derive(Clone, Debug)]
struct SessionData {
    did: String,
}

/// Extract session data from request headers or cookies
async fn extract_session(
    headers: HeaderMap,
    State(_app_state): State<AppState>,
) -> Result<SessionData, StatusCode> {
    // Try to get DID from Authorization header first
    let did_str = if let Some(auth_header) = headers.get("Authorization") {
        // Bearer token authentication (for API endpoints)
        auth_header
            .to_str()
            .ok()
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(|s| s.to_string())
    } else if let Some(cookie_header) = headers.get("Cookie") {
        // Cookie-based authentication (for form endpoints)
        cookie_header
            .to_str()
            .ok()
            .and_then(|cookies| {
                // Parse cookies to find session_did
                for cookie in cookies.split(';') {
                    let cookie = cookie.trim();
                    if let Some(did) = cookie.strip_prefix("session_did=") {
                        return Some(did.to_string());
                    }
                }
                None
            })
    } else {
        None
    };

    let did_str = did_str.ok_or(StatusCode::UNAUTHORIZED)?;
    
    // Validate DID format
    if !did_str.starts_with("did:") {
        return Err(StatusCode::UNAUTHORIZED);
    }
    
    // Just return the DID - we'll create agents on demand when needed
    Ok(SessionData {
        did: did_str,
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
) -> Result<(StatusCode, HeaderMap, Html<String>), ErrorTemplate> {
    println!("🔄 Processing OAuth callback");
    
    match (&*app_state.oauth_client).callback(params).await {
        Ok((session, _)) => {
            println!("✅ OAuth callback successful");
            
            // Get user DID from session
            let user_info = match session.did().await {
                Some(did) => {
                    println!("👤 User DID: {}", did.as_str());
                    
                    // Create agent to fetch profile
                    let agent = Agent::new(session);
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

            // Create response with session cookie
            let mut headers = HeaderMap::new();
            
            // Set session cookie with the DID
            if let Some(ref info) = user_info {
                if let Some(ref did) = info.did {
                    let cookie_value = format!("session_did={}; Path=/; HttpOnly; SameSite=Lax", did);
                    headers.insert("Set-Cookie", cookie_value.parse().unwrap());
                }
            }

            let template = SuccessTemplate {
                user_info,
                error_message: None,
            };
            
            let html = template.render().unwrap();
            
            Ok((StatusCode::OK, headers, Html(html)))
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

// ========== Form Handler Routes ==========

/// Display the blog list page
async fn blog_list_handler(
    State(app_state): State<AppState>,
) -> Result<BlogListTemplate, ErrorTemplate> {
    // Load all posts from database for display (for now, let's show all posts)
    let db_pool_arc = Arc::new(app_state.db_pool.clone());
    let posts = BlogPostFromDb::load_latest_posts(&db_pool_arc).await
        .map_err(|e| {
            ErrorTemplate {
                title: "Database Error".to_string(),
                handle: None,
                action: Some("load blog posts".to_string()),
                error: format!("Failed to load posts: {}", e),
            }
        })?;

    // Convert to template format
    let blog_posts: Vec<BlogPostInfo> = posts.iter().map(|p| BlogPostInfo {
        uri: p.uri.clone(),
        title: p.title.clone(),
        content: p.content.clone(),
        summary: p.summary.clone(),
        tags: p.tags.clone(),
        published: p.published,
        created_at: p.created_at.to_rfc3339(),
        updated_at: p.updated_at.to_rfc3339(),
    }).collect();

    Ok(BlogListTemplate {
        posts: blog_posts,
    })
}

/// Display the create blog post form
async fn blog_create_form_handler() -> BlogCreateTemplate {
    BlogCreateTemplate
}

/// Form data for creating a blog post
#[derive(Deserialize)]
struct CreateBlogPostForm {
    title: String,
    content: String,
    summary: Option<String>,
    tags: Option<String>,
    published: Option<String>, // Form checkboxes come as strings
}

/// Handle form submission to create a blog post
async fn blog_create_form_handler_post(
    headers: HeaderMap,
    State(app_state): State<AppState>,
    Form(form): Form<CreateBlogPostForm>,
) -> Result<Redirect, ErrorTemplate> {
    // Extract authenticated session
    let session = match extract_session(headers, State(app_state.clone())).await {
        Ok(session) => session,
        Err(_) => {
            return Err(ErrorTemplate {
                title: "Authentication Error".to_string(),
                handle: None,
                action: Some("create blog post".to_string()),
                error: "Authentication required to create blog posts".to_string(),
            });
        }
    };

    // Generate a unique record key (rkey) for this blog post
    let rkey = format!("post-{}", chrono::Utc::now().timestamp_millis());
    let uri = format!("at://{}/xyz.blogosphere.post/{}", session.did, rkey);

    // Parse tags from comma-separated string
    let tags = form.tags
        .map(|t| t.split(',').map(|tag| tag.trim().to_string()).collect::<Vec<_>>())
        .filter(|tags| !tags.is_empty());

    // Create BlogPostRecordData from form
    let record_data = BlogPostRecordData {
        title: form.title,
        content: form.content,
        summary: form.summary.filter(|s| !s.is_empty()),
        tags,
        published: Some(form.published.is_some()),
        created_at: atrium_api::types::string::Datetime::new(chrono::Utc::now().into()),
        updated_at: Some(atrium_api::types::string::Datetime::new(chrono::Utc::now().into())),
    };

    // Convert to database model and save locally
    let blog_post = BlogPostFromDb::from_codegen_record_data(
        uri.clone(),
        session.did.clone(),
        &record_data
    ).map_err(|e| {
        ErrorTemplate {
            title: "Conversion Error".to_string(),
            handle: None,
            action: Some("create blog post".to_string()),
            error: format!("Failed to convert record data: {}", e),
        }
    })?;

    let db_pool_arc = Arc::new(app_state.db_pool.clone());
    blog_post.save(&db_pool_arc).await.map_err(|e| {
        ErrorTemplate {
            title: "Database Error".to_string(),
            handle: None,
            action: Some("save blog post".to_string()),
            error: format!("Failed to save to database: {}", e),
        }
    })?;

    println!("✅ Successfully saved blog post locally: {}", blog_post.title);

    // Now attempt to post to the PDS
    let did_parsed = Did::new(session.did.clone()).map_err(|_| {
        ErrorTemplate {
            title: "Authentication Error".to_string(),
            handle: None,
            action: Some("create blog post".to_string()),
            error: "Invalid DID format".to_string(),
        }
    })?;
    
    match app_state.oauth_client.restore(&did_parsed).await {
        Ok(oauth_session) => {
            // Create agent from the restored OAuth session
            let agent = Agent::new(oauth_session);
            
            // Create the record on the PDS
            let create_record_input = atrium_api::com::atproto::repo::create_record::InputData {
                repo: did_parsed.into(),
                collection: Nsid::new("xyz.blogosphere.post".to_string()).unwrap(),
                rkey: Some(RecordKey::new(rkey).unwrap()),
                validate: Some(true),
                swap_commit: None,
                record: record_data.clone().try_into_unknown().unwrap(),
            };

            match agent.api.com.atproto.repo.create_record(create_record_input.into()).await {
                Ok(response) => {
                    println!("🎉 Successfully posted to PDS! URI: {}", response.data.uri);
                    println!("📝 CID: {:?}", response.data.cid);
                }
                Err(e) => {
                    println!("⚠️  Failed to post to PDS (saved locally): {}", e);
                    // We still continue since the post is saved locally
                }
            }
        }
        Err(e) => {
            println!("⚠️  Could not authenticate with PDS (saved locally): {}", e);
            // We still continue since the post is saved locally
        }
    }

    println!("✅ Blog post creation completed");
    Ok(Redirect::to("/blog"))
}

/// Display a specific blog post
async fn blog_view_handler(
    State(app_state): State<AppState>,
    axum::extract::Path(uri): axum::extract::Path<String>,
) -> Result<BlogViewTemplate, ErrorTemplate> {
    // Load the specific post from database
    let db_pool_arc = Arc::new(app_state.db_pool.clone());
    let posts = BlogPostFromDb::load_latest_posts(&db_pool_arc).await
        .map_err(|e| {
            ErrorTemplate {
                title: "Database Error".to_string(),
                handle: None,
                action: Some("load blog post".to_string()),
                error: format!("Failed to load posts: {}", e),
            }
        })?;

    // Find the post with the matching URI
    let post = posts.into_iter().find(|p| p.uri == uri)
        .ok_or_else(|| ErrorTemplate {
            title: "Not Found".to_string(),
            handle: None,
            action: Some("find blog post".to_string()),
            error: "Blog post not found".to_string(),
        })?;

    let blog_post_info = BlogPostInfo {
        uri: post.uri.clone(),
        title: post.title.clone(),
        content: post.content.clone(),
        summary: post.summary.clone(),
        tags: post.tags.clone(),
        published: post.published,
        created_at: post.created_at.to_rfc3339(),
        updated_at: post.updated_at.to_rfc3339(),
    };

    Ok(BlogViewTemplate {
        post: blog_post_info,
    })
}

/// Display the edit form for a blog post
async fn blog_edit_form_handler(
    State(app_state): State<AppState>,
    axum::extract::Path(uri): axum::extract::Path<String>,
) -> Result<BlogEditTemplate, ErrorTemplate> {
    // Load the specific post from database
    let db_pool_arc = Arc::new(app_state.db_pool.clone());
    let posts = BlogPostFromDb::load_latest_posts(&db_pool_arc).await
        .map_err(|e| {
            ErrorTemplate {
                title: "Database Error".to_string(),
                handle: None,
                action: Some("load blog post".to_string()),
                error: format!("Failed to load posts: {}", e),
            }
        })?;

    // Find the post with the matching URI
    let post = posts.into_iter().find(|p| p.uri == uri)
        .ok_or_else(|| ErrorTemplate {
            title: "Not Found".to_string(),
            handle: None,
            action: Some("find blog post".to_string()),
            error: "Blog post not found".to_string(),
        })?;

    let blog_post_info = BlogPostInfo {
        uri: post.uri.clone(),
        title: post.title.clone(),
        content: post.content.clone(),
        summary: post.summary.clone(),
        tags: post.tags.clone(),
        published: post.published,
        created_at: post.created_at.to_rfc3339(),
        updated_at: post.updated_at.to_rfc3339(),
    };

    Ok(BlogEditTemplate {
        post: blog_post_info,
    })
}

/// Form data for updating a blog post
#[derive(Deserialize)]
struct UpdateBlogPostForm {
    title: String,
    content: String,
    summary: Option<String>,
    tags: Option<String>,
    published: Option<String>, // Form checkboxes come as strings
}

/// Handle form submission to update a blog post
async fn blog_edit_form_handler_post(
    State(app_state): State<AppState>,
    axum::extract::Path(uri): axum::extract::Path<String>,
    Form(form): Form<UpdateBlogPostForm>,
) -> Result<Redirect, ErrorTemplate> {
    // Load the existing post from database
    let db_pool_arc = Arc::new(app_state.db_pool.clone());
    let posts = BlogPostFromDb::load_latest_posts(&db_pool_arc).await
        .map_err(|e| {
            ErrorTemplate {
                title: "Database Error".to_string(),
                handle: None,
                action: Some("load blog post".to_string()),
                error: format!("Failed to load posts: {}", e),
            }
        })?;

    // Find the post with the matching URI
    let existing_post = posts.into_iter().find(|p| p.uri == uri)
        .ok_or_else(|| ErrorTemplate {
            title: "Not Found".to_string(),
            handle: None,
            action: Some("find blog post".to_string()),
            error: "Blog post not found".to_string(),
        })?;

    // Convert existing post to record data for updating
    let mut record_data = existing_post.to_codegen_record_data()
        .map_err(|e| {
            ErrorTemplate {
                title: "Conversion Error".to_string(),
                handle: None,
                action: Some("convert blog post".to_string()),
                error: format!("Failed to convert existing post: {}", e),
            }
        })?;

    // Apply updates from form
    record_data.title = form.title;
    record_data.content = form.content;
    record_data.summary = form.summary.filter(|s| !s.is_empty());
    
    // Parse tags from comma-separated string
    record_data.tags = form.tags
        .map(|t| t.split(',').map(|tag| tag.trim().to_string()).collect::<Vec<_>>())
        .filter(|tags| !tags.is_empty());
    
    record_data.published = Some(form.published.is_some());
    
    // Update timestamps
    record_data.updated_at = Some(atrium_api::types::string::Datetime::new(chrono::Utc::now().into()));

    // Convert back to database model
    let updated_post = BlogPostFromDb::from_codegen_record_data(
        existing_post.uri.clone(),
        existing_post.author_did.clone(),
        &record_data
    ).map_err(|e| {
        ErrorTemplate {
            title: "Conversion Error".to_string(),
            handle: None,
            action: Some("convert updated post".to_string()),
            error: format!("Failed to convert updated record data: {}", e),
        }
    })?;

    // Save updated post to database
    updated_post.save_or_update(&app_state.db_pool).await
        .map_err(|e| {
            ErrorTemplate {
                title: "Database Error".to_string(),
                handle: None,
                action: Some("update blog post".to_string()),
                error: format!("Failed to update post: {}", e),
            }
        })?;

    println!("✅ Successfully updated blog post: {}", updated_post.title);
    Ok(Redirect::to("/blog"))
}

/// Display the delete confirmation for a blog post
async fn blog_delete_form_handler(
    State(app_state): State<AppState>,
    axum::extract::Path(uri): axum::extract::Path<String>,
) -> Result<BlogDeleteTemplate, ErrorTemplate> {
    // Load the specific post from database
    let db_pool_arc = Arc::new(app_state.db_pool.clone());
    let posts = BlogPostFromDb::load_latest_posts(&db_pool_arc).await
        .map_err(|e| {
            ErrorTemplate {
                title: "Database Error".to_string(),
                handle: None,
                action: Some("load blog post".to_string()),
                error: format!("Failed to load posts: {}", e),
            }
        })?;

    // Find the post with the matching URI
    let post = posts.into_iter().find(|p| p.uri == uri)
        .ok_or_else(|| ErrorTemplate {
            title: "Not Found".to_string(),
            handle: None,
            action: Some("find blog post".to_string()),
            error: "Blog post not found".to_string(),
        })?;

    let blog_post_info = BlogPostInfo {
        uri: post.uri.clone(),
        title: post.title.clone(),
        content: post.content.clone(),
        summary: post.summary.clone(),
        tags: post.tags.clone(),
        published: post.published,
        created_at: post.created_at.to_rfc3339(),
        updated_at: post.updated_at.to_rfc3339(),
    };

    Ok(BlogDeleteTemplate {
        post: blog_post_info,
    })
}

/// Handle form submission to delete a blog post
async fn blog_delete_form_handler_post(
    State(app_state): State<AppState>,
    axum::extract::Path(uri): axum::extract::Path<String>,
) -> Result<Redirect, ErrorTemplate> {
    // Delete the post from database
    BlogPostFromDb::delete_by_uri(&app_state.db_pool, uri.clone()).await
        .map_err(|e| {
            ErrorTemplate {
                title: "Database Error".to_string(),
                handle: None,
                action: Some("delete blog post".to_string()),
                error: format!("Failed to delete post: {}", e),
            }
        })?;

    println!("✅ Successfully deleted blog post with URI: {}", uri);
    Ok(Redirect::to("/blog"))
}