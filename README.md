# AT Protocol OAuth Crate

A reusable Rust crate for implementing OAuth authentication with the AT Protocol (used by Bluesky and other atproto services).

## Features

- **Easy OAuth Setup**: Builder pattern for quick OAuth client configuration
- **Persistent Storage**: SQLite-based session and state storage
- **DNS Resolution**: Built-in DNS resolver for AT Protocol handles
- **Type Safety**: Strongly typed interfaces with comprehensive error handling
- **Async Support**: Full async/await support with tokio

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
atproto-oauth = "0.1.0"
async-sqlite = "0.5.0"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

## Basic Usage

```rust
use atproto_oauth::{OAuthClientBuilder, db::create_tables_in_database};
use async_sqlite::PoolBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create database connection
    let db_pool = PoolBuilder::new()
        .path("oauth.sqlite3")
        .open()
        .await?;

    // Create database tables
    create_tables_in_database(&db_pool).await?;

    // Build OAuth client
    let oauth_client = OAuthClientBuilder::new()
        .host("localhost")
        .port(3000)
        .db_pool(db_pool)
        .build()?;

    // Use in your web application...
    Ok(())
}
```

## Web Framework Integration

This crate is designed to work with web frameworks like Actix-web, Axum, or Warp. Here's an example with Actix-web:

```rust
use actix_web::{web, App, HttpServer, HttpResponse};
use atproto_oauth::{OAuthClientBuilder, AuthorizeOptions, KnownScope, Scope};

async fn login_handler(
    oauth_client: web::Data<AtprotoOAuthClient>,
    form: web::Form<LoginForm>,
) -> HttpResponse {
    let oauth_url = oauth_client
        .authorize(
            &form.handle,
            AuthorizeOptions {
                scopes: vec![
                    Scope::Known(KnownScope::Atproto),
                    Scope::Known(KnownScope::TransitionGeneric),
                ],
                ..Default::default()
            },
        )
        .await;

    match oauth_url {
        Ok(url) => HttpResponse::Found()
            .append_header(("Location", url))
            .finish(),
        Err(err) => HttpResponse::InternalServerError()
            .body(format!("OAuth error: {}", err)),
    }
}
```

## Configuration Options

The `OAuthClientBuilder` supports several configuration options:

- `host()` - Set the callback host (default: "127.0.0.1")
- `port()` - Set the callback port (default: 8080)
- `db_pool()` - Set the database connection pool (required)
- `scopes()` - Set OAuth scopes (default: Atproto + TransitionGeneric)
- `plc_directory_url()` - Set custom PLC directory URL

## Components

### Storage
- `SqliteSessionStore` - Persistent OAuth session storage
- `SqliteStateStore` - Persistent OAuth state storage

### DNS Resolution
- `HickoryDnsTxtResolver` - DNS TXT record resolution for AT Protocol handles

### Database
- `create_tables_in_database()` - Creates required database tables
- Database models for auth sessions and state

## License

MIT

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.