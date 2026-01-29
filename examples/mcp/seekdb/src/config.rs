//! Configuration for SeekDB MCP Server

use anyhow::{Result, anyhow};

/// Server configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// SeekDB host address
    pub host: String,
    /// SeekDB port (default: 2881)
    pub port: u16,
    /// Database user
    pub user: String,
    /// Database password
    pub password: String,
    /// Database name
    pub database: String,
}

impl ServerConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        let host = std::env::var("SEEKDB_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port: u16 = std::env::var("SEEKDB_PORT")
            .unwrap_or_else(|_| "2881".to_string())
            .parse()
            .map_err(|_| anyhow!("Invalid SEEKDB_PORT"))?;
        let user = std::env::var("SEEKDB_USER").unwrap_or_else(|_| "root".to_string());
        let password = std::env::var("SEEKDB_PASSWORD").unwrap_or_default();
        let database = std::env::var("SEEKDB_DATABASE")
            .map_err(|_| anyhow!("SEEKDB_DATABASE environment variable is required"))?;

        Ok(Self {
            host,
            port,
            user,
            password,
            database,
        })
    }

    /// Get the MySQL connection URL
    pub fn connection_url(&self) -> String {
        format!(
            "mysql://{}:{}@{}:{}/{}",
            self.user, self.password, self.host, self.port, self.database
        )
    }
}
