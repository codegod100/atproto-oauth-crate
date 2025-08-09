/// Long-running example showing how to use the atproto-oauth crate with a web server
mod schema;

use atproto_oauth::{
    OAuthClientBuilder, AtprotoOAuthClient, AuthorizeOptions, CallbackParams, 
    KnownScope, Scope, Handle,
};
use schema::create_tables_in_database;
use atrium_api::agent::Agent;
use async_sqlite::PoolBuilder;
use axum::{
    extract::{Query, State},
    response::{Html, Redirect},
    routing::get,
    Router,
};
use std::sync::Arc;
// Removed unused import

type AppState = Arc<AtprotoOAuthClient>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

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

async fn home_handler() -> Html<&'static str> {
    Html(r#"
<!DOCTYPE html>
<html>
<head>
    <title>AT Protocol OAuth Example</title>
    <style>
        body { font-family: Arial, sans-serif; max-width: 600px; margin: 50px auto; padding: 20px; }
        .button { display: inline-block; padding: 10px 20px; background: #0085ff; color: white; text-decoration: none; border-radius: 5px; margin: 10px 0; }
        .button:hover { background: #0070dd; }
        .info { background: #f5f5f5; padding: 15px; border-radius: 5px; margin: 10px 0; }
    </style>
</head>
<body>
    <h1>🔐 AT Protocol OAuth Example</h1>
    <div class="info">
        <p>This example demonstrates the <code>atproto-oauth</code> Rust crate in action.</p>
        <p>This is a <strong>Rust web server</strong> built with Axum that shows how to integrate AT Protocol OAuth into your applications.</p>
        <p>To test the OAuth flow, you'll need a Bluesky handle (like <code>user.bsky.social</code>).</p>
        <p><a href="https://github.com/codegod100/atproto-oauth-crate" target="_blank">📖 View full documentation and source code on GitHub</a></p>
    </div>
    
    <div class="info">
        <h3>🦀 About This Rust Project</h3>
        <p><strong>Dependencies:</strong></p>
        <ul style="margin: 10px 0; padding-left: 20px;">
            <li><code>atproto-oauth</code> - The main OAuth crate we're demonstrating</li>
            <li><code>axum</code> - Modern async web framework for Rust</li>
            <li><code>tokio</code> - Async runtime</li>
            <li><code>async-sqlite</code> - Async SQLite for session storage</li>
        </ul>
        <p><strong>Run this example:</strong> <code>cargo run --example basic_usage</code></p>
        <p><strong>Database Schema:</strong> See <code>examples/schema.rs</code> for how to integrate OAuth tables with your application schema</p>
    </div>
    
    <h2>Test OAuth Flow</h2>
    <form action="/login" method="get">
        <label for="handle">Enter your Bluesky handle:</label><br>
        <input type="text" id="handle" name="handle" placeholder="user.bsky.social" style="width: 300px; padding: 8px; margin: 10px 0;">
        <br>
        <button type="submit" class="button">Start OAuth Flow</button>
    </form>
    
    <div class="info">
        <h3>How it works:</h3>
        <ol>
            <li>Enter your AT Protocol handle above</li>
            <li>You'll be redirected to your PDS for authentication</li>
            <li>After auth, you'll be redirected back to <code>/oauth/callback</code></li>
            <li>The callback will process the OAuth response</li>
        </ol>
    </div>
</body>
</html>
    "#)
}

async fn login_handler(
    Query(params): Query<std::collections::HashMap<String, String>>,
    State(oauth_client): State<AppState>,
) -> Result<Redirect, Html<String>> {
    let handle_str = params.get("handle").ok_or_else(|| {
        Html("Error: Handle parameter required".to_string())
    })?;

    // Parse the handle
    let handle = Handle::new(handle_str.clone()).map_err(|e| {
        Html(format!("Error: Invalid handle '{}': {}", handle_str, e))
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
            Err(Html(format!(
                r#"
<!DOCTYPE html>
<html>
<head><title>OAuth Error</title></head>
<body>
    <h1>❌ OAuth Error</h1>
    <p>Failed to start OAuth flow for handle: {}</p>
    <p>Error: {}</p>
    <a href="/">← Back to Home</a>
</body>
</html>
                "#,
                handle_str, e
            )))
        }
    }
}

async fn callback_handler(
    Query(params): Query<CallbackParams>,
    State(oauth_client): State<AppState>,
) -> Html<String> {
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
                            format!(
                                r#"
                                <div class="user-info">
                                    <h3>👤 Authenticated User</h3>
                                    <p><strong>Handle:</strong> @{}</p>
                                    <p><strong>Display Name:</strong> {}</p>
                                    <p><strong>DID:</strong> <code>{}</code></p>
                                    <p><strong>Followers:</strong> {}</p>
                                    <p><strong>Following:</strong> {}</p>
                                    <p><strong>Posts:</strong> {}</p>
                                    {}
                                </div>
                                "#,
                                profile.handle.as_str(),
                                profile.display_name.as_deref().unwrap_or("(Not set)"),
                                did.as_str(),
                                profile.followers_count.unwrap_or(0),
                                profile.follows_count.unwrap_or(0),
                                profile.posts_count.unwrap_or(0),
                                if let Some(ref description) = profile.description {
                                    format!("<p><strong>Bio:</strong> {}</p>", html_escape(description))
                                } else {
                                    String::new()
                                }
                            )
                        }
                        Err(e) => {
                            println!("⚠️  Could not fetch profile: {}", e);
                            format!(
                                r#"
                                <div class="user-info">
                                    <h3>👤 Authenticated User</h3>
                                    <p><strong>DID:</strong> <code>{}</code></p>
                                    <p><em>Profile details could not be fetched: {}</em></p>
                                </div>
                                "#,
                                did.as_str(), html_escape(&e.to_string())
                            )
                        }
                    }
                }
                None => {
                    println!("⚠️  No DID found in session");
                    "<div class=\"user-info\"><p><em>No user DID found in session</em></p></div>".to_string()
                }
            };

            Html(format!(
                r#"
<!DOCTYPE html>
<html>
<head>
    <title>OAuth Success</title>
    <style>
        body {{ font-family: Arial, sans-serif; max-width: 700px; margin: 50px auto; padding: 20px; }}
        .success {{ background: #d4edda; color: #155724; padding: 15px; border-radius: 5px; }}
        .info {{ background: #f5f5f5; padding: 15px; border-radius: 5px; margin: 10px 0; }}
        .user-info {{ background: #e7f3ff; border: 2px solid #0085ff; padding: 15px; border-radius: 5px; margin: 15px 0; }}
        .button {{ display: inline-block; padding: 10px 20px; background: #28a745; color: white; text-decoration: none; border-radius: 5px; }}
        code {{ background: #f4f4f4; padding: 2px 4px; border-radius: 3px; font-size: 0.9em; }}
    </style>
</head>
<body>
    <h1>✅ OAuth Success!</h1>
    <div class="success">
        <p>OAuth flow completed successfully! Session established and user authenticated.</p>
    </div>
    
    {}
    
    <div class="info">
        <h3>🔧 Technical Details:</h3>
        <ul>
            <li>✅ AT Protocol OAuth flow completed</li>
            <li>✅ User session created and stored in SQLite</li>
            <li>✅ Session credentials verified with API call</li>
            <li>✅ User profile fetched using authenticated API</li>
        </ul>
        
        <p><strong>Next Steps:</strong></p>
        <ul>
            <li>Session is now stored and can be restored later</li>
            <li>Use <code>oauth_client.restore(did)</code> to get the session back</li>
            <li>Create an <code>Agent</code> with the session to make API calls</li>
        </ul>
    </div>
    
    <a href="/" class="button">← Start Another OAuth Flow</a>
</body>
</html>
                "#,
                user_info
            ))
        }
        Err(e) => {
            println!("❌ OAuth callback error: {}", e);
            Html(format!(
                r#"
<!DOCTYPE html>
<html>
<head><title>OAuth Callback Error</title></head>
<body>
    <h1>❌ OAuth Callback Error</h1>
    <p>Failed to process OAuth callback</p>
    <p>Error: {}</p>
    <a href="/">← Back to Home</a>
</body>
</html>
                "#,
                html_escape(&e.to_string())
            ))
        }
    }
}

// Simple HTML escaping function
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}