//! Built-in `tower_sessions::SessionStore` backends.
//!
//! These are hand-rolled against `tower-sessions` 0.15's `SessionStore` trait
//! because the official `tower-sessions-sqlx-store` / `tower-sessions-redis-store`
//! crates currently pin older, mutually-incompatible `tower-sessions-core`
//! versions. Each store serializes the whole [`Record`] with MessagePack.
//!
//! ## Integrity
//!
//! Because these stores persist records **outside** the process, the serialized
//! bytes are HMAC-SHA256 tagged with an operator-supplied key before storage and
//! verified on load. A tampered or forged record (e.g. written by an attacker
//! with access to the database/cache to escalate roles via the stored ID-token
//! claims) fails verification and is treated as "session not found", forcing
//! re-authentication. See [`seal`]/[`open`].

#[cfg(feature = "session-postgres")]
pub(crate) use postgres::PostgresSessionStore;
#[cfg(feature = "session-redis")]
pub(crate) use redis::RedisSessionStore;

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Length in bytes of the prepended HMAC-SHA256 tag.
const TAG_LEN: usize = 32;

/// Prepends an HMAC-SHA256 tag to `data`. Layout: `tag(32) || data`.
// HMAC accepts a key of any length, so `new_from_slice` is infallible here.
#[allow(clippy::expect_used)]
fn seal(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts a key of any length");
    mac.update(data);
    let tag = mac.finalize().into_bytes();
    let mut out = Vec::with_capacity(TAG_LEN + data.len());
    out.extend_from_slice(&tag);
    out.extend_from_slice(data);
    out
}

/// Verifies and strips the HMAC tag produced by [`seal`]. Returns the original
/// data, or `None` if the input is malformed or the tag does not verify (i.e. the
/// stored bytes were tampered with or written without the signing key).
// HMAC accepts a key of any length, so `new_from_slice` is infallible here.
#[allow(clippy::expect_used)]
fn open(key: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < TAG_LEN {
        return None;
    }
    let (tag, data) = sealed.split_at(TAG_LEN);
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts a key of any length");
    mac.update(data);
    // `verify_slice` compares in constant time and errors on mismatch.
    mac.verify_slice(tag).ok()?;
    Some(data.to_vec())
}

#[cfg(feature = "session-postgres")]
mod postgres {
    use super::{open, seal};
    use async_trait::async_trait;
    use sqlx_postgres::PgPool;
    use std::sync::Arc;
    use tower_sessions::{
        SessionStore,
        session::{Id, Record},
        session_store::{self, Error},
    };

    /// PostgreSQL-backed session store using a single `tower_sessions` table.
    ///
    /// The `data` column holds the HMAC-sealed MessagePack record; `expiry_date`
    /// is a separate column so expiry-driven cleanup never depends on the blob.
    #[derive(Clone)]
    pub(crate) struct PostgresSessionStore {
        pool: PgPool,
        signing_key: Arc<[u8]>,
    }

    impl std::fmt::Debug for PostgresSessionStore {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("PostgresSessionStore")
                .finish_non_exhaustive()
        }
    }

    impl PostgresSessionStore {
        pub(crate) fn new(pool: PgPool, signing_key: Arc<[u8]>) -> Self {
            Self { pool, signing_key }
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
            let sealed = seal(&self.signing_key, &data);
            sqlx::query(
                "INSERT INTO tower_sessions (id, data, expiry_date) VALUES ($1, $2, $3) \
                 ON CONFLICT (id) DO UPDATE \
                 SET data = EXCLUDED.data, expiry_date = EXCLUDED.expiry_date",
            )
            .bind(record.id.to_string())
            .bind(sealed)
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
                Some((sealed,)) => match open(&self.signing_key, &sealed) {
                    Some(data) => Ok(Some(
                        rmp_serde::from_slice(&data).map_err(|e| Error::Decode(e.to_string()))?,
                    )),
                    None => {
                        tracing::warn!(
                            session_id = %session_id,
                            "Rejected a session record that failed HMAC verification \
                             (tampered or signed with a different key)"
                        );
                        Ok(None)
                    }
                },
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
    use super::{open, seal};
    use async_trait::async_trait;
    use fred::{clients::Pool, prelude::KeysInterface, types::Expiration};
    use std::sync::Arc;
    use tower_sessions::{
        SessionStore,
        session::{Id, Record},
        session_store::{self, Error},
    };

    /// Redis-backed session store. Relies on Redis key expiry (`EXAT`) for
    /// cleanup, so no background sweep is needed. The stored value is the
    /// HMAC-sealed MessagePack record; expiry is carried by `EXAT`, separate from
    /// the value.
    #[derive(Clone)]
    pub(crate) struct RedisSessionStore {
        pool: Pool,
        signing_key: Arc<[u8]>,
    }

    impl std::fmt::Debug for RedisSessionStore {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("RedisSessionStore").finish_non_exhaustive()
        }
    }

    impl RedisSessionStore {
        pub(crate) fn new(pool: Pool, signing_key: Arc<[u8]>) -> Self {
            Self { pool, signing_key }
        }

        fn key(id: &Id) -> String {
            format!("tower-sessions:{id}")
        }
    }

    #[async_trait]
    impl SessionStore for RedisSessionStore {
        async fn save(&self, record: &Record) -> session_store::Result<()> {
            let data = rmp_serde::to_vec(record).map_err(|e| Error::Encode(e.to_string()))?;
            let sealed = seal(&self.signing_key, &data);
            let expire = Some(Expiration::EXAT(record.expiry_date.unix_timestamp()));
            let _: () = self
                .pool
                .set(
                    Self::key(&record.id),
                    sealed.as_slice(),
                    expire,
                    None,
                    false,
                )
                .await
                .map_err(|e| Error::Backend(e.to_string()))?;
            Ok(())
        }

        async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
            let sealed: Option<Vec<u8>> = self
                .pool
                .get(Self::key(session_id))
                .await
                .map_err(|e| Error::Backend(e.to_string()))?;
            match sealed {
                Some(bytes) => match open(&self.signing_key, &bytes) {
                    Some(data) => Ok(Some(
                        rmp_serde::from_slice(&data).map_err(|e| Error::Decode(e.to_string()))?,
                    )),
                    None => {
                        tracing::warn!(
                            session_id = %session_id,
                            "Rejected a session record that failed HMAC verification \
                             (tampered or signed with a different key)"
                        );
                        Ok(None)
                    }
                },
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

#[cfg(test)]
mod tests {
    use super::{open, seal};

    #[test]
    fn seal_then_open_round_trips() {
        let key = b"0123456789abcdef0123456789abcdef";
        let data = b"a serialized session record";
        let sealed = seal(key, data);
        assert_eq!(sealed.len(), super::TAG_LEN + data.len());
        assert_eq!(open(key, &sealed).as_deref(), Some(&data[..]));
    }

    #[test]
    fn open_rejects_tampered_data() {
        let key = b"0123456789abcdef0123456789abcdef";
        let mut sealed = seal(key, b"role=user");
        // Flip a byte in the data region (after the 32-byte tag).
        let last = sealed.len() - 1;
        sealed[last] ^= 0xff;
        assert!(open(key, &sealed).is_none());
    }

    #[test]
    fn open_rejects_wrong_key() {
        let sealed = seal(b"0123456789abcdef0123456789abcdef", b"role=admin");
        assert!(open(b"DIFFERENT-key-DIFFERENT-key-1234", &sealed).is_none());
    }

    #[test]
    fn open_rejects_truncated_input() {
        assert!(open(b"0123456789abcdef0123456789abcdef", b"short").is_none());
    }
}
