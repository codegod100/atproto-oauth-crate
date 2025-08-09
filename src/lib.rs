/// A reusable crate for AT Protocol OAuth functionality
/// 
/// This crate provides OAuth client setup, session/state storage, and DNS resolution
/// components that can be reused across AT Protocol applications.

pub mod oauth;
pub mod storage;
pub mod resolver;
pub mod db;

// Re-export commonly used types and traits for convenience
pub use oauth::{OAuthClientBuilder, AtprotoOAuthClient};
pub use storage::{SqliteSessionStore, SqliteStateStore, SqliteStoreError};
pub use resolver::HickoryDnsTxtResolver;

// Re-export key external types that users will need
pub use atrium_oauth::{
    OAuthClient, OAuthClientConfig, Scope, KnownScope, AuthorizeOptions, CallbackParams,
    AtprotoLocalhostClientMetadata, DefaultHttpClient, OAuthResolverConfig
};
pub use atrium_api::types::string::{Did, Handle};
pub use atrium_identity::{
    did::{CommonDidResolver, CommonDidResolverConfig, DEFAULT_PLC_DIRECTORY_URL},
    handle::{AtprotoHandleResolver, AtprotoHandleResolverConfig},
};