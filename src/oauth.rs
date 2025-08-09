/// OAuth client builder and utilities for AT Protocol
use crate::{
    resolver::HickoryDnsTxtResolver,
    storage::{SqliteSessionStore, SqliteStateStore},
};
use async_sqlite::Pool;
use atrium_identity::{
    did::{CommonDidResolver, CommonDidResolverConfig, DEFAULT_PLC_DIRECTORY_URL},
    handle::{AtprotoHandleResolver, AtprotoHandleResolverConfig},
};
use atrium_oauth::{
    AtprotoLocalhostClientMetadata, DefaultHttpClient, KnownScope, OAuthClient, OAuthClientConfig,
    OAuthResolverConfig, Scope,
};
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum OAuthClientError {
    #[error("Failed to create OAuth client: {0}")]
    ClientCreationError(#[from] atrium_oauth::Error),
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
}

/// Type alias for a commonly used OAuth client configuration
pub type AtprotoOAuthClient = OAuthClient<
    SqliteStateStore,
    SqliteSessionStore,
    CommonDidResolver<DefaultHttpClient>,
    AtprotoHandleResolver<HickoryDnsTxtResolver, DefaultHttpClient>,
>;

/// Builder for creating AT Protocol OAuth clients with sensible defaults
pub struct OAuthClientBuilder {
    host: String,
    port: u16,
    db_pool: Option<Pool>,
    scopes: Vec<Scope>,
    plc_directory_url: String,
}

impl OAuthClientBuilder {
    /// Create a new OAuth client builder
    pub fn new() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            db_pool: None,
            scopes: vec![
                Scope::Known(KnownScope::Atproto),
                Scope::Known(KnownScope::TransitionGeneric),
            ],
            plc_directory_url: DEFAULT_PLC_DIRECTORY_URL.to_string(),
        }
    }

    /// Set the host for OAuth callbacks (default: "127.0.0.1")
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Set the port for OAuth callbacks (default: 8080)
    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the database pool for session/state storage (required)
    pub fn db_pool(mut self, pool: Pool) -> Self {
        self.db_pool = Some(pool);
        self
    }

    /// Set custom OAuth scopes (default: Atproto + TransitionGeneric)
    pub fn scopes(mut self, scopes: Vec<Scope>) -> Self {
        self.scopes = scopes;
        self
    }

    /// Set custom PLC directory URL (default: official AT Protocol PLC directory)
    pub fn plc_directory_url(mut self, url: impl Into<String>) -> Self {
        self.plc_directory_url = url.into();
        self
    }

    /// Build the OAuth client
    pub fn build(self) -> Result<Arc<AtprotoOAuthClient>, OAuthClientError> {
        let db_pool = self
            .db_pool
            .ok_or_else(|| OAuthClientError::InvalidConfiguration("Database pool is required".to_string()))?;

        let http_client = Arc::new(DefaultHttpClient::default());

        let config = OAuthClientConfig {
            client_metadata: AtprotoLocalhostClientMetadata {
                redirect_uris: Some(vec![format!(
                    "http://{}:{}/oauth/callback",
                    self.host, self.port
                )]),
                scopes: Some(self.scopes),
            },
            keys: None,
            resolver: OAuthResolverConfig {
                did_resolver: CommonDidResolver::new(CommonDidResolverConfig {
                    plc_directory_url: self.plc_directory_url,
                    http_client: http_client.clone(),
                }),
                handle_resolver: AtprotoHandleResolver::new(AtprotoHandleResolverConfig {
                    dns_txt_resolver: HickoryDnsTxtResolver::default(),
                    http_client: http_client.clone(),
                }),
                authorization_server_metadata: Default::default(),
                protected_resource_metadata: Default::default(),
            },
            state_store: SqliteStateStore::new(db_pool.clone()),
            session_store: SqliteSessionStore::new(db_pool),
        };

        let client = OAuthClient::new(config)?;
        Ok(Arc::new(client))
    }
}

impl Default for OAuthClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_default() {
        let builder = OAuthClientBuilder::new();
        assert_eq!(builder.host, "127.0.0.1");
        assert_eq!(builder.port, 8080);
        assert_eq!(builder.plc_directory_url, DEFAULT_PLC_DIRECTORY_URL);
    }

    #[test]
    fn test_builder_customization() {
        let builder = OAuthClientBuilder::new()
            .host("localhost")
            .port(3000)
            .plc_directory_url("https://custom-plc.example.com");
        
        assert_eq!(builder.host, "localhost");
        assert_eq!(builder.port, 3000);
        assert_eq!(builder.plc_directory_url, "https://custom-plc.example.com");
    }
}