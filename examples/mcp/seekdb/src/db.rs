//! Database connection utilities for SeekDB

use anyhow::Result;
use mysql_async::{Pool, Opts, OptsBuilder};

use crate::config::ServerConfig;

/// Create a MySQL connection pool for SeekDB
pub fn create_pool(config: &ServerConfig) -> Result<Pool> {
    let opts = OptsBuilder::default()
        .ip_or_hostname(&config.host)
        .tcp_port(config.port)
        .user(Some(&config.user))
        .pass(Some(&config.password))
        .db_name(Some(&config.database));

    let pool = Pool::new(Opts::from(opts));
    Ok(pool)
}
