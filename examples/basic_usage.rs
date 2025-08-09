/// Long-running example showing how to use the atproto-oauth crate with a web server
mod schema;
mod templates;

use atproto_oauth::{
    // Core OAuth functionality
    OAuthClientBuilder, AtprotoOAuthClient, AuthorizeOptions, CallbackParams, 
    KnownScope, Scope, Handle,
    // Database and agent types
    Agent, PoolBuilder,
    // Web framework types
    Query, State, Redirect, get, Router,
};
use schema::create_tables_in_database;
use templates::{HomeTemplate, SuccessTemplate, ErrorTemplate, UserInfo};
use std::sync::Arc;
// Removed unused import

type AppState = Arc<AtprotoOAuthClient>;

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
        .db_pool(db_pool)
        .build()?;

    println!("✅ OAuth client created successfully!");
    println!("🔗 Redirect URI: http://127.0.0.1:3000/oauth/callback");

    // Create router with OAuth endpoints
    let app = Router::new()
        .route("/", get(home_handler))
        .route("/login", get(login_handler))
        .route("/oauth/callback", get(callback_handler))
        .with_state(oauth_client);

    println!("\n🌐 Server running on http://127.0.0.1:3000");
    println!("📝 Visit http://127.0.0.1:3000 to test OAuth flow");
    println!("⏹️  Press Ctrl+C to stop");

    // Run the server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn home_handler() -> HomeTemplate {
    HomeTemplate
}

async fn login_handler(
    Query(params): Query<std::collections::HashMap<String, String>>,
    State(oauth_client): State<AppState>,
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
    match oauth_client.authorize(
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
    State(oauth_client): State<AppState>,
) -> Result<SuccessTemplate, ErrorTemplate> {
    println!("🔄 Processing OAuth callback");
    
    match oauth_client.callback(params).await {
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

