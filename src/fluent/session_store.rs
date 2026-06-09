//! Built-in `tower_sessions::SessionStore` backends.
//!
//! These are hand-rolled against `tower-sessions` 0.15's `SessionStore` trait
//! because the official `tower-sessions-sqlx-store` / `tower-sessions-redis-store`
//! crates currently pin older, mutually-incompatible `tower-sessions-core`
//! versions. Each store serializes the whole [`Record`] with MessagePack.

#[cfg(feature = "session-postgres")]
pub(crate) use postgres::PostgresSessionStore;
#[cfg(feature = "session-redis")]
pub(crate) use redis::RedisSessionStore;

#[cfg(feature = "session-postgres")]
mod postgres {
    use async_trait::async_trait;
    use sqlx_postgres::PgPool;
    use tower_sessions::{
        SessionStore,
        session::{Id, Record},
        session_store::{self, Error},
    };

    /// PostgreSQL-backed session store using a single `tower_sessions` table.
    #[derive(Clone, Debug)]
    pub(crate) struct PostgresSessionStore {
        pool: PgPool,
    }

    impl PostgresSessionStore {
        pub(crate) fn new(pool: PgPool) -> Self {
            Self { pool }
        }

        /// Creates the session table if it does not yet exist.
        pub(crate) async fn migrate(&self) -> Result<(), sqlx::Error> {
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS tower_sessions (\
                     id TEXT PRIMARY KEY, \
                     data BYTEA NOT NULL, \
                     expiry_date TIMESTAMPTZ NOT NULL\
                 )",
            )
            .execute(&self.pool)
            .await?;
            Ok(())
        }

        /// Deletes rows whose expiry has passed.
        pub(crate) async fn delete_expired(&self) -> Result<(), sqlx::Error> {
            sqlx::query("DELETE FROM tower_sessions WHERE expiry_date < now()")
                .execute(&self.pool)
                .await?;
            Ok(())
        }
    }

    #[async_trait]
    impl SessionStore for PostgresSessionStore {
        async fn save(&self, record: &Record) -> session_store::Result<()> {
            let data = rmp_serde::to_vec(record).map_err(|e| Error::Encode(e.to_string()))?;
            sqlx::query(
                "INSERT INTO tower_sessions (id, data, expiry_date) VALUES ($1, $2, $3) \
                 ON CONFLICT (id) DO UPDATE \
                 SET data = EXCLUDED.data, expiry_date = EXCLUDED.expiry_date",
            )
            .bind(record.id.to_string())
            .bind(data)
            .bind(record.expiry_date)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Backend(e.to_string()))?;
            Ok(())
        }

        async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
            let row: Option<(Vec<u8>,)> = sqlx::query_as(
                "SELECT data FROM tower_sessions WHERE id = $1 AND expiry_date > now()",
            )
            .bind(session_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Error::Backend(e.to_string()))?;

            match row {
                Some((data,)) => Ok(Some(
                    rmp_serde::from_slice(&data).map_err(|e| Error::Decode(e.to_string()))?,
                )),
                None => Ok(None),
            }
        }

        async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
            sqlx::query("DELETE FROM tower_sessions WHERE id = $1")
                .bind(session_id.to_string())
                .execute(&self.pool)
                .await
                .map_err(|e| Error::Backend(e.to_string()))?;
            Ok(())
        }
    }
}

#[cfg(feature = "session-redis")]
mod redis {
    use async_trait::async_trait;
    use fred::{clients::Pool, prelude::KeysInterface, types::Expiration};
    use tower_sessions::{
        SessionStore,
        session::{Id, Record},
        session_store::{self, Error},
    };

    /// Redis-backed session store. Relies on Redis key expiry (`EXAT`) for
    /// cleanup, so no background sweep is needed.
    #[derive(Clone)]
    pub(crate) struct RedisSessionStore {
        pool: Pool,
    }

    impl std::fmt::Debug for RedisSessionStore {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("RedisSessionStore").finish_non_exhaustive()
        }
    }

    impl RedisSessionStore {
        pub(crate) fn new(pool: Pool) -> Self {
            Self { pool }
        }

        fn key(id: &Id) -> String {
            format!("tower-sessions:{id}")
        }
    }

    #[async_trait]
    impl SessionStore for RedisSessionStore {
        async fn save(&self, record: &Record) -> session_store::Result<()> {
            let data = rmp_serde::to_vec(record).map_err(|e| Error::Encode(e.to_string()))?;
            let expire = Some(Expiration::EXAT(record.expiry_date.unix_timestamp()));
            let _: () = self
                .pool
                .set(Self::key(&record.id), data.as_slice(), expire, None, false)
                .await
                .map_err(|e| Error::Backend(e.to_string()))?;
            Ok(())
        }

        async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
            let data: Option<Vec<u8>> = self
                .pool
                .get(Self::key(session_id))
                .await
                .map_err(|e| Error::Backend(e.to_string()))?;
            match data {
                Some(bytes) => Ok(Some(
                    rmp_serde::from_slice(&bytes).map_err(|e| Error::Decode(e.to_string()))?,
                )),
                None => Ok(None),
            }
        }

        async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
            let _: () = self
                .pool
                .del(Self::key(session_id))
                .await
                .map_err(|e| Error::Backend(e.to_string()))?;
            Ok(())
        }
    }
}
