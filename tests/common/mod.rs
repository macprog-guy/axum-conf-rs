//! Shared helpers for the integration-test crates.
//!
//! Included via `mod common;` in each integration test file. (Files under
//! `tests/common/` are not compiled as their own test binary by Cargo.)

/// Returns whether a Docker daemon appears reachable.
///
/// The `tests/*` integration suites use testcontainers to stand up real
/// Postgres / Redis / Keycloak instances. On a machine without Docker (the
/// documented `make test` path), starting a container panics. Tests call
/// [`docker_unavailable`] to skip cleanly instead.
///
/// Local sockets are probed by **connecting**, not merely checking existence —
/// a restricted sandbox can expose `/var/run/docker.sock` while denying
/// connections, and only a real connect attempt distinguishes the two.
pub fn docker_available() -> bool {
    // An explicitly configured (possibly remote) daemon — trust the operator.
    if std::env::var_os("DOCKER_HOST").is_some() {
        return true;
    }

    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        let mut candidates = vec![std::path::PathBuf::from("/var/run/docker.sock")];
        // Docker Desktop on macOS exposes a per-user socket.
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(std::path::Path::new(&home).join(".docker/run/docker.sock"));
        }
        candidates.iter().any(|p| UnixStream::connect(p).is_ok())
    }

    #[cfg(not(unix))]
    {
        false
    }
}

/// Returns `true` and prints a skip notice when Docker is unavailable, so a test
/// can bail with `if common::docker_unavailable() { return; }`.
#[must_use]
pub fn docker_unavailable() -> bool {
    if docker_available() {
        return false;
    }
    eprintln!(
        "SKIP: no reachable Docker daemon (set DOCKER_HOST or start Docker) — \
         skipping container-backed integration test"
    );
    true
}
