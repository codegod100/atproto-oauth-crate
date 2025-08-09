# AT Protocol OAuth Crate

A reusable Rust crate for implementing OAuth authentication with the AT Protocol (used by Bluesky and other atproto services).

## Features

- **Easy OAuth Setup**: Builder pattern for quick OAuth client configuration
- **Persistent Storage**: SQLite-based session and state storage
- **DNS Resolution**: Built-in DNS resolver for AT Protocol handles
- **Type Safety**: Strongly typed interfaces with comprehensive error handling
- **Async Support**: Full async/await support with tokio

## Examples

The crate includes comprehensive examples in the `examples/` directory:

1. `basic_usage.rs` - A complete web application demonstrating OAuth authentication and blog post CRUD operations
2. `schema.rs` - Database schema definitions for OAuth sessions and blog posts
3. `templates.rs` - HTML templates for the web interface
4. `lexicon.rs` - AT Protocol lexicon definitions
5. Code generation outputs in the `codegen/` directory

To run the basic example:

```bash
cargo run --example basic_usage
```

This will start a web server on `http://127.0.0.1:3000` with:

- OAuth authentication flow
- Blog post CRUD operations (Create, Read, Update, Delete)
- Database persistence using SQLite
- Type-safe integration with AT Protocol lexicons

## API Endpoints

The example application provides the following API endpoints:

### OAuth Endpoints
- `GET /` - Home page
- `GET /login?handle={handle}` - Start OAuth flow for a given handle
- `GET /oauth/callback` - OAuth callback handler

### Blog Post CRUD Endpoints
- `POST /api/posts` - Create a new blog post (requires authentication)
- `GET /api/posts` - List all published blog posts (public)
- `GET /api/posts/my` - List authenticated user's blog posts (requires authentication)
- `GET /api/posts/{uri}` - Get a specific blog post (requires authentication)
- `PUT /api/posts/{uri}` - Update a specific blog post (requires authentication)
- `DELETE /api/posts/{uri}` - Delete a specific blog post (requires authentication)

All authenticated endpoints require an `Authorization: Bearer {did}` header where `{did}` is a valid DID (Decentralized Identifier).

## Web Framework Integration

This crate is designed to work with web frameworks like Axum. The example demonstrates full integration with:

- OAuth authentication flow
- Session management
- CRUD operations with database persistence
- Type-safe API endpoints

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