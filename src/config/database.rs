use {
    crate::{Error, Result},
    serde::Deserialize,
    std::env,
    std::time::Duration,
};

///
/// Configuration for the database connection pool.
///
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// Database connection URL.
    /// This should be a valid Postgres connection string in URL format.
    /// For example, "postgres://user:password@localhost:5432/database".
    /// This value is required.
    #[serde(default = "DatabaseConfig::default_url")]
    pub url: String,

    /// Sets the maximum number of connections in the pool.
    /// By default `max_pool_size` is set to 1.
    #[serde(default = "DatabaseConfig::default_min_pool_size")]
    pub min_pool_size: u8,

    /// Sets the maximum number of connections in the pool.
    /// By default `max_pool_size` is set to 2.
    #[serde(default = "DatabaseConfig::default_max_pool_size")]
    pub max_pool_size: u8,

    /// Maximum idle time for connections in the pool.
    /// Connections that have been idle for longer than this duration
    /// will be closed. For example, a value of "5m" would set the
    /// maximum idle time to 5 minutes. By default `max_idle_time` is None.
    #[serde(default, with = "humantime_serde")]
    pub max_idle_time: Option<Duration>,
}

impl DatabaseConfig {
    fn default_url() -> String {
        env::var("DATABASE_URL").unwrap_or_else(|_| String::from("postgres://localhost:5432/test"))
    }
    fn default_min_pool_size() -> u8 {
        1
    }
    fn default_max_pool_size() -> u8 {
        2
    }
    pub fn validate(&self) -> Result<()> {
        // Check if URL is empty or only whitespace
        if self.url.trim().is_empty() {
            return Err(Error::database_config(
                "URL is required. Set DATABASE_URL env var or [database] url in config.",
            ));
        }

        // Validate URL format (basic check for postgres:// prefix)
        if !self.url.starts_with("postgres://") && !self.url.starts_with("postgresql://") {
            return Err(Error::database_config(
                "URL must start with postgres:// or postgresql://",
            ));
        }

        // Validate pool sizes
        if self.max_pool_size == 0 {
            return Err(Error::database_config("max_pool_size must be > 0"));
        }

        Ok(())
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        DatabaseConfig {
            url: Self::default_url(),
            min_pool_size: Self::default_min_pool_size(),
            max_pool_size: Self::default_max_pool_size(),
            max_idle_time: None,
        }
    }
}
