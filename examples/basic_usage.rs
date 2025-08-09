/// Long-running example showing how to use the atproto-oauth crate with a web server
mod schema;
mod templates;
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
    response::Html,
    // Form handling
    extract::Form,
};
use schema::{create_tables_in_database, BlogPostFromDb};
use templates::{HomeTemplate, SuccessTemplate, ErrorTemplate, UserInfo, BlogListTemplate, BlogCreateTemplate, BlogEditTemplate, BlogViewTemplate, BlogPostInfo};
use askama::Template;
use codegen::com::crabdance::nandi::post::RecordData as BlogPostRecordData;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
// Removed unused import

// Enhanced app state that includes both OAuth client and database pool
#[derive(Clone)]
struct AppState {
    oauth_client: Arc<AtprotoOAuthClient>,
    db_pool: Arc<Pool>,
}

async fn register_custom_lexicon(
    agent: &Agent<impl SessionManager + Send + Sync>,
    did: &str, 
    lexicon_nsid: &str
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Read the lexicon definition from file
    let lexicon_json = std::fs::read_to_string("examples/lexicons/post.json")?;
    let mut lexicon_data: serde_json::Value = serde_json::from_str(&lexicon_json)?;

    // Ensure required $type is present per spec (com.atproto.lexicon.schema)
    if let serde_json::Value::Object(map) = &mut lexicon_data {
        map.entry("$type").or_insert(serde_json::Value::String("com.atproto.lexicon.schema".to_string()));
    }
    
    // Create the lexicon schema record
    let did_parsed = did.parse::<Did>()?;
    let rkey = lexicon_nsid; // Use the NSID as the record key
    
    let create_record_input = atrium_api::com::atproto::repo::create_record::InputData {
        repo: did_parsed.into(),
        collection: Nsid::new("com.atproto.lexicon.schema".to_string()).unwrap(),
        rkey: Some(RecordKey::new(rkey.to_string()).unwrap()),
        // Allow server-side validation (must pass); if this fails we log and continue
        validate: Some(true),
        swap_commit: None,
        record: lexicon_data.try_into_unknown()?,
    };

    match agent.api.com.atproto.repo.create_record(create_record_input.into()).await {
        Ok(response) => {
            println!("✅ Successfully registered lexicon! URI: {}", response.data.uri);
            Ok(())
        }
        Err(e) => {
            eprintln!("❌ Failed to register lexicon: {}", e);
            Err(Box::new(e))
        }
    }
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
    .route("/healthz", get(|| async { "ok" }))
        // Blog form routes (HTML interface)
        .route("/posts", get(blog_list_handler))
        .route("/posts/new", get(blog_create_form_handler))
    // Support accidental GET navigation to /posts/create by redirecting to the form at /posts/new
    .route("/posts/create", get(|| async { Redirect::to("/posts/new") }).post(blog_create_form_handler_post))
        // Use wildcard *uri so the full at:// URI (which contains slashes) is captured
        .route("/posts/view/*uri", get(blog_view_handler))
        .route("/posts/edit/*uri", get(blog_edit_form_handler))
        .route("/posts/update/*uri", post(blog_edit_form_handler_post))
    .route("/posts/delete/:rkey", post(blog_delete_form_handler_post))
        // Blog CRUD API routes
        .route("/api/posts", post(create_blog_post).get(list_published_posts))
        .route("/api/posts/my", get(list_my_posts))
    // Wildcard to allow full at:// URIs in path
    .route("/api/posts/*uri", get(get_blog_post).put(update_blog_post).delete(delete_blog_post))
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
    original_uri: axum::extract::OriginalUri,
    Query(params): Query<CallbackParams>,
    State(app_state): State<AppState>,
) -> Result<(StatusCode, HeaderMap, Html<String>), ErrorTemplate> {
    use std::time::Instant;
    let start = Instant::now();
    let code_preview = params.code.chars().take(8).collect::<String>();
    let state_preview = params.state.as_ref().map(|s| s.chars().take(8).collect::<String>()).unwrap_or_else(|| "<none>".to_string());
    println!("[CALLBACK][START] uri='{}' code_preview='{}' state_preview='{}' timestamp={}ms", original_uri.0, code_preview, state_preview, chrono::Utc::now().timestamp_millis());
    
    match (&*app_state.oauth_client).callback(params).await {
        Ok((session, _)) => {
            println!("[CALLBACK][SUCCESS] Session established in {}ms", start.elapsed().as_millis());
            
            // Get user DID from session
            let user_info = match session.did().await {
                Some(did) => {
                    println!("[CALLBACK][SESSION] DID={}", did.as_str());
                    
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
                            println!("[CALLBACK][PROFILE][SUCCESS] handle={} followers={:?} follows={:?}", profile.handle.as_str(), profile.followers_count, profile.follows_count);
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
                            println!("[CALLBACK][PROFILE][WARN] fetch failed error={}", e);
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
                    println!("[CALLBACK][WARN] No DID in session");
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
            println!("[CALLBACK][ERROR] error={} elapsed_ms={}", e, start.elapsed().as_millis());
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

This is a sample blog post created using the **com.crabdance.nandi.post** lexicon.

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
    summary: Some("A sample blog post demonstrating the com.crabdance.nandi.post lexicon with AT Protocol OAuth integration.".to_string()),
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
    let sample_uri = format!("at://{}/com.crabdance.nandi.post/{}", author_did, "sample-post-123");
    
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
    let uri = format!("at://{}/com.crabdance.nandi.post/{}", session.did, rkey);

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
    let start = std::time::Instant::now();
    println!("[BLOG][UPDATE][START] uri='{}' ts={}ms", uri, chrono::Utc::now().timestamp_millis());
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

    // Save updated post to database first (local source of truth)
    updated_post.save_or_update(&app_state.db_pool).await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError {
                error: "database_error".to_string(),
                message: format!("Failed to update post: {}", e),
            }))
        })?;

    // Attempt to update the record on the PDS as well (best-effort)
    // We derive rkey from the URI: at://did/collection/rkey
    let rkey_opt = uri.rsplit('/').next().map(|s| s.to_string());
    let collection_opt = uri.split('/').nth(3).map(|s| s.to_string()); // after at:, '', did, collection
    if let (Some(rkey), Some(collection)) = (rkey_opt, collection_opt) {
        // Only proceed if this is our custom collection (avoid touching unrelated URIs)
        if collection == "com.crabdance.nandi.post" {
            if let Ok(did_parsed) = Did::new(session.did.clone()) {
                match app_state.oauth_client.restore(&did_parsed).await {
                    Ok(oauth_session) => {
                        let agent = Agent::new(oauth_session);

                        // Build record JSON and inject $type
                        let mut record_value = serde_json::to_value(&record_data).unwrap_or_else(|_| serde_json::json!({}));
                        if let serde_json::Value::Object(obj) = &mut record_value {
                            obj.insert("$type".to_string(), serde_json::Value::String(collection.clone()));
                        }

                        // We try a put_record first (update). If that fails with not found, fallback to create.
                        let attempt_put = |validate_flag: bool, record_json: &serde_json::Value| {
                            atrium_api::com::atproto::repo::put_record::InputData {
                                repo: did_parsed.clone().into(),
                                collection: Nsid::new(collection.clone()).unwrap(),
                                rkey: RecordKey::new(rkey.clone()).unwrap(),
                                validate: Some(validate_flag),
                                swap_record: None,
                                swap_commit: None,
                                record: record_json.clone().try_into_unknown().unwrap(),
                            }
                        };

                        let mut put_input = attempt_put(true, &record_value);
                        let mut did_put = false;
                        match agent.api.com.atproto.repo.put_record(put_input.clone().into()).await {
                            Ok(resp) => {
                                println!("[BLOG][UPDATE][PDS][PUT_SUCCESS] uri={} cid={:?}", resp.data.uri, resp.data.cid);
                                did_put = true;
                            }
                            Err(e) => {
                                let msg = format!("{}", e);
                                if msg.contains("Lexicon not found") || msg.contains("schema") {
                                    println!("[BLOG][UPDATE][PDS][PUT_RETRY] validation=false reason=lexicon_not_found");
                                    put_input = attempt_put(false, &record_value);
                                    match agent.api.com.atproto.repo.put_record(put_input.into()).await {
                                        Ok(resp2) => {
                                            println!("[BLOG][UPDATE][PDS][PUT_SUCCESS_NO_VALIDATION] uri={}", resp2.data.uri);
                                            did_put = true;
                                        }
                                        Err(e2) => {
                                            println!("[BLOG][UPDATE][PDS][PUT_FAIL_RETRY] error={}", e2);
                                        }
                                    }
                                } else if msg.contains("Record not found") || msg.contains("Could not find record") {
                                    // We'll fall back to create below
                                    println!("[BLOG][UPDATE][PDS][PUT_MISSING] will_create error={}", msg);
                                } else {
                                    println!("[BLOG][UPDATE][PDS][PUT_FAIL] error={}", msg);
                                }
                            }
                        }

                        if !did_put {
                            // Fallback: create the record (idempotent-ish if not existing)
                            let attempt_create = |validate_flag: bool, record_json: &serde_json::Value| {
                                atrium_api::com::atproto::repo::create_record::InputData {
                                    repo: did_parsed.clone().into(),
                                    collection: Nsid::new(collection.clone()).unwrap(),
                                    rkey: Some(RecordKey::new(rkey.clone()).unwrap()),
                                    validate: Some(validate_flag),
                                    swap_commit: None,
                                    record: record_json.clone().try_into_unknown().unwrap(),
                                }
                            };
                            let mut create_input = attempt_create(true, &record_value);
                            match agent.api.com.atproto.repo.create_record(create_input.clone().into()).await {
                                Ok(resp) => {
                                    println!("[BLOG][UPDATE][PDS][CREATE_SUCCESS] uri={} cid={:?}", resp.data.uri, resp.data.cid);
                                }
                                Err(e) => {
                                    let msg = format!("{}", e);
                                    if msg.contains("Lexicon not found") || msg.contains("schema") {
                                        println!("[BLOG][UPDATE][PDS][CREATE_RETRY] validation=false reason=lexicon_not_found");
                                        create_input = attempt_create(false, &record_value);
                                        match agent.api.com.atproto.repo.create_record(create_input.into()).await {
                                            Ok(resp2) => println!("[BLOG][UPDATE][PDS][CREATE_SUCCESS_NO_VALIDATION] uri={}", resp2.data.uri),
                                            Err(e2) => println!("[BLOG][UPDATE][PDS][CREATE_FAIL_RETRY] error={}", e2),
                                        }
                                    } else {
                                        println!("[BLOG][UPDATE][PDS][CREATE_FAIL] error={}", msg);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => println!("[BLOG][UPDATE][PDS][AUTH_FAIL] error={} local_update=true", e),
                }
            }
        }
    }

    println!("✅ Successfully updated blog post: {} (local + attempted PDS sync) elapsed_ms={}", updated_post.title, start.elapsed().as_millis());
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
    Query(params): Query<std::collections::HashMap<String, String>>,
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
        formatted_tags: serde_json::from_str::<Vec<String>>(&p.tags).ok()
            .map(|v| v.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", "))
            .unwrap_or_default(),
        published: p.published,
        created_at: p.created_at.to_rfc3339(),
        updated_at: p.updated_at.to_rfc3339(),
    }).collect();

    Ok(BlogListTemplate {
        posts: blog_posts,
        success_message: params.get("success").cloned(),
        error_message: params.get("error").cloned(),
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

/// Parse tags input which may be either a JSON array string (e.g. ["rust","atproto"]) or a
/// comma-separated list (e.g. rust, atproto). Returns None if empty/blank.
fn parse_tags_input(raw: &str) -> Option<Vec<String>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() { return None; }

    // Try JSON first if it looks like JSON array
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        if let Ok(mut vec) = serde_json::from_str::<Vec<String>>(trimmed) {
            // Handle double-encoded array: ["[\"tag1\",\"tag2\"]"]
            if vec.len() == 1 {
                let inner = vec[0].trim();
                if inner.starts_with('[') && inner.ends_with(']') {
                    if let Ok(vec2) = serde_json::from_str::<Vec<String>>(inner) {
                        vec = vec2;
                    }
                }
            }
            let cleaned: Vec<String> = vec.into_iter()
                .map(|s| s.trim().trim_matches('\"').to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !cleaned.is_empty() { return Some(cleaned); } else { return None; }
        }
    }

    // Fallback: comma separated
    let parts: Vec<String> = trimmed.split(',')
        .map(|s| s.trim().trim_matches('\"').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() { None } else { Some(parts) }
}

/// Handle form submission to create a blog post
async fn blog_create_form_handler_post(
    headers: HeaderMap,
    State(app_state): State<AppState>,
    Form(form): Form<CreateBlogPostForm>,
) -> Result<Redirect, ErrorTemplate> {
    let start = std::time::Instant::now();
    println!("[BLOG][CREATE][START] title='{}' published_flag={} time={}ms", form.title, form.published.is_some(), chrono::Utc::now().timestamp_millis());
    // Extract authenticated session
    let session = match extract_session(headers, State(app_state.clone())).await {
        Ok(session) => session,
        Err(_) => {
            println!("[BLOG][CREATE][AUTH][FAIL] no session_did elapsed_ms={}", start.elapsed().as_millis());
            return Ok(Redirect::to("/posts?error=Auth%20required"));
        }
    };

    // Generate a unique record key (rkey) for this blog post
    let rkey = format!("post-{}", chrono::Utc::now().timestamp_millis());
    let uri = format!("at://{}/com.crabdance.nandi.post/{}", session.did, rkey);

    // Parse tags (supports JSON array or comma-separated list)
    let tags = form.tags.as_ref().and_then(|s| parse_tags_input(s));

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

    println!("[BLOG][CREATE][LOCAL][OK] uri={} elapsed_ms={}", blog_post.uri, start.elapsed().as_millis());

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
            
            // First, try to register our custom lexicon
            let lexicon_nsid = "com.crabdance.nandi.post";
            if let Err(e) = register_custom_lexicon(&agent, &session.did, lexicon_nsid).await {
                eprintln!("⚠️ Failed to register lexicon (continuing anyway): {}", e);
            }
            
            // Build record JSON and inject $type (required for records)
            let mut record_value = serde_json::to_value(&record_data).unwrap_or_else(|_| serde_json::json!({}));
            if let serde_json::Value::Object(obj) = &mut record_value {
                obj.insert("$type".to_string(), serde_json::Value::String("com.crabdance.nandi.post".to_string()));
            }

            // Try with validation first; if lexicon unresolved, retry without validation (best-effort)
            let attempt_create = |validate_flag: bool, record_json: &serde_json::Value| {
                atrium_api::com::atproto::repo::create_record::InputData {
                    repo: did_parsed.clone().into(),
                    collection: Nsid::new("com.crabdance.nandi.post".to_string()).unwrap(),
                    rkey: Some(RecordKey::new(rkey.clone()).unwrap()),
                    validate: Some(validate_flag),
                    swap_commit: None,
                    record: record_json.clone().try_into_unknown().unwrap(),
                }
            };

            let mut create_record_input = attempt_create(true, &record_value);

            match agent.api.com.atproto.repo.create_record(create_record_input.into()).await {
                Ok(response) => {
                    println!("[BLOG][CREATE][PDS][SUCCESS] uri={} cid={:?} elapsed_ms={}", response.data.uri, response.data.cid, start.elapsed().as_millis());
                }
                Err(e) => {
                    println!("[BLOG][CREATE][PDS][WARN] first_attempt_failed error={}", e);
                    let msg = format!("{}", e);
                    if msg.contains("Lexicon not found") || msg.contains("schema") {
                        println!("[BLOG][CREATE][PDS][RETRY] validation=false reason=lexicon_not_found");
                        create_record_input = attempt_create(false, &record_value);
                        match agent.api.com.atproto.repo.create_record(create_record_input.into()).await {
                            Ok(response2) => {
                                println!("[BLOG][CREATE][PDS][SUCCESS_NO_VALIDATION] uri={} elapsed_ms={}", response2.data.uri, start.elapsed().as_millis());
                            }
                            Err(e2) => {
                                println!("[BLOG][CREATE][PDS][ERROR_RETRY] error={}", e2);
                            }
                        }
                    } else {
                        println!("[BLOG][CREATE][PDS][FAIL] error={} saved_locally=true", msg);
                    }
                }
            }
        }
        Err(e) => {
        println!("[BLOG][CREATE][PDS][AUTH_FAIL] error={} saved_locally=true", e);
            // We still continue since the post is saved locally
        }
    }

    println!("[BLOG][CREATE][END] total_elapsed_ms={}", start.elapsed().as_millis());
    Ok(Redirect::to("/posts?success=Created%20post"))
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
        formatted_tags: serde_json::from_str::<Vec<String>>(&post.tags).ok()
            .map(|v| v.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", "))
            .unwrap_or_default(),
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
        formatted_tags: serde_json::from_str::<Vec<String>>(&post.tags).ok()
            .map(|v| v.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", "))
            .unwrap_or_default(),
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
    let start = std::time::Instant::now();
    println!("[BLOG][EDIT_FORM][START] uri='{}' ts={}ms", uri, chrono::Utc::now().timestamp_millis());
    // (Future) enforce auth here as well (e.g. compare session cookie DID to post DID)
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
    record_data.tags = form.tags.as_ref().and_then(|s| parse_tags_input(s));
    
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

    // Attempt to sync to PDS (best-effort, non-fatal). We use the post's author DID.
    if let Ok(did_parsed) = Did::new(updated_post.author_did.clone()) {
        match app_state.oauth_client.restore(&did_parsed).await {
            Ok(oauth_session) => {
                let agent = Agent::new(oauth_session);
                // Derive collection and rkey from URI at://did/collection/rkey
                let parts: Vec<&str> = updated_post.uri.split('/').collect();
                if parts.len() >= 5 { // at:, '', did, collection, rkey
                    let collection = parts[3].to_string();
                    let rkey = parts[4].to_string();
                    if collection == "com.crabdance.nandi.post" {
                        // Build record JSON with $type
                        let mut record_value = serde_json::to_value(&record_data).unwrap_or_else(|_| serde_json::json!({}));
                        if let serde_json::Value::Object(obj) = &mut record_value {
                            obj.insert("$type".to_string(), serde_json::Value::String(collection.clone()));
                        }
                        let attempt_put = |validate_flag: bool, record_json: &serde_json::Value| {
                            atrium_api::com::atproto::repo::put_record::InputData {
                                repo: did_parsed.clone().into(),
                                collection: Nsid::new(collection.clone()).unwrap(),
                                rkey: RecordKey::new(rkey.clone()).unwrap(),
                                validate: Some(validate_flag),
                                swap_record: None,
                                swap_commit: None,
                                record: record_json.clone().try_into_unknown().unwrap(),
                            }
                        };
                        let mut put_input = attempt_put(true, &record_value);
                        let mut did_put = false;
                        match agent.api.com.atproto.repo.put_record(put_input.clone().into()).await {
                            Ok(resp) => { println!("[BLOG][EDIT_FORM][PDS][PUT_SUCCESS] uri={} cid={:?}", resp.data.uri, resp.data.cid); did_put = true; }
                            Err(e) => {
                                let msg = format!("{}", e);
                                if msg.contains("Lexicon not found") || msg.contains("schema") { // retry without validation
                                    println!("[BLOG][EDIT_FORM][PDS][PUT_RETRY] validation=false reason=lexicon_not_found");
                                    put_input = attempt_put(false, &record_value);
                                    match agent.api.com.atproto.repo.put_record(put_input.into()).await {
                                        Ok(resp2) => { println!("[BLOG][EDIT_FORM][PDS][PUT_SUCCESS_NO_VALIDATION] uri={}", resp2.data.uri); did_put = true; }
                                        Err(e2) => println!("[BLOG][EDIT_FORM][PDS][PUT_FAIL_RETRY] error={}", e2),
                                    }
                                } else if msg.contains("Record not found") || msg.contains("Could not find record") {
                                    println!("[BLOG][EDIT_FORM][PDS][PUT_MISSING] will_attempt_create");
                                } else {
                                    println!("[BLOG][EDIT_FORM][PDS][PUT_FAIL] error={}", msg);
                                }
                            }
                        }
                        if !did_put {
                            let attempt_create = |validate_flag: bool, record_json: &serde_json::Value| {
                                atrium_api::com::atproto::repo::create_record::InputData {
                                    repo: did_parsed.clone().into(),
                                    collection: Nsid::new(collection.clone()).unwrap(),
                                    rkey: Some(RecordKey::new(rkey.clone()).unwrap()),
                                    validate: Some(validate_flag),
                                    swap_commit: None,
                                    record: record_json.clone().try_into_unknown().unwrap(),
                                }
                            };
                            let mut create_input = attempt_create(true, &record_value);
                            match agent.api.com.atproto.repo.create_record(create_input.clone().into()).await {
                                Ok(resp) => println!("[BLOG][EDIT_FORM][PDS][CREATE_SUCCESS] uri={} cid={:?}", resp.data.uri, resp.data.cid),
                                Err(e) => {
                                    let msg = format!("{}", e);
                                    if msg.contains("Lexicon not found") || msg.contains("schema") {
                                        println!("[BLOG][EDIT_FORM][PDS][CREATE_RETRY] validation=false reason=lexicon_not_found");
                                        create_input = attempt_create(false, &record_value);
                                        match agent.api.com.atproto.repo.create_record(create_input.into()).await {
                                            Ok(resp2) => println!("[BLOG][EDIT_FORM][PDS][CREATE_SUCCESS_NO_VALIDATION] uri={}", resp2.data.uri),
                                            Err(e2) => println!("[BLOG][EDIT_FORM][PDS][CREATE_FAIL_RETRY] error={}", e2),
                                        }
                                    } else {
                                        println!("[BLOG][EDIT_FORM][PDS][CREATE_FAIL] error={}", msg);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => println!("[BLOG][EDIT_FORM][PDS][AUTH_FAIL] error={} local_update=true", e),
        }
    }

    println!("✅ Successfully updated blog post (form) elapsed_ms={}", start.elapsed().as_millis());
    Ok(Redirect::to("/posts?success=Updated%20post"))
}

/// Display the delete confirmation for a blog post

/// Handle form submission to delete a blog post
async fn blog_delete_form_handler_post(
    State(app_state): State<AppState>,
    axum::extract::Path(rkey): axum::extract::Path<String>,
) -> Result<Redirect, ErrorTemplate> {
    // (Future) enforce auth here as well
    // Load posts to resolve full URI from record key
    let db_pool_arc = Arc::new(app_state.db_pool.clone());
    let posts = BlogPostFromDb::load_latest_posts(&db_pool_arc).await.map_err(|e| ErrorTemplate {
        title: "Database Error".to_string(),
        handle: None,
        action: Some("load blog posts".to_string()),
        error: format!("Failed to load posts: {}", e),
    })?;
    let uri = match posts.into_iter().find(|p| p.uri.rsplit('/').next() == Some(rkey.as_str())) {
        Some(p) => p.uri,
        None => return Err(ErrorTemplate { title: "Not Found".to_string(), handle: None, action: Some("delete blog post".to_string()), error: "Blog post not found".to_string() }),
    };
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
    Ok(Redirect::to("/posts?success=Deleted%20post"))
}