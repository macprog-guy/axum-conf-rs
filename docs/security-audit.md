# Security Audit

This guide explains how to use `make audit` to scan your dependencies for known security vulnerabilities.

## Quick Start

Run the security audit:

```bash
make audit
```

Or directly with cargo:

```bash
cargo audit
```

## What It Checks

The `cargo audit` command scans your `Cargo.lock` file against the [RustSec Advisory Database](https://rustsec.org/), checking for:

| Category | Description |
|----------|-------------|
| **Vulnerabilities** | Known security issues (CVEs) in dependencies |
| **Unmaintained** | Crates that are no longer maintained |
| **Yanked** | Versions that have been removed from crates.io |
| **Unsound** | Code that may cause undefined behavior |

## Installation

Install cargo-audit if not already present:

```bash
cargo install cargo-audit
```

## Understanding Output

### Clean Report

```
    Fetching advisory database from `https://github.com/RustSec/advisory-db.git`
      Loaded 628 security advisories (from /Users/you/.cargo/advisory-db)
    Updating crates.io index
    Scanning Cargo.lock for vulnerabilities (423 crate dependencies)

No vulnerabilities found!
```

### Vulnerability Found

```
    Fetching advisory database from `https://github.com/RustSec/advisory-db.git`
      Loaded 628 security advisories (from /Users/you/.cargo/advisory-db)
    Updating crates.io index
    Scanning Cargo.lock for vulnerabilities (423 crate dependencies)

Crate:     hyper
Version:   0.14.18
Title:     Lenient `hyper` header parsing of `Content-Length` could allow request smuggling
Date:      2023-07-17
ID:        RUSTSEC-2023-0043
URL:       https://rustsec.org/advisories/RUSTSEC-2023-0043
Solution:  Upgrade to >=0.14.28, >=1.0.1
Dependency tree:
hyper 0.14.18
└── reqwest 0.11.14
    └── my-app 0.1.0

error: 1 vulnerability found!
```

## Fixing Vulnerabilities

### 1. Update Dependencies

Most vulnerabilities are fixed by updating:

```bash
# Update all dependencies
cargo update

# Update a specific crate
cargo update -p hyper

# Check for outdated dependencies
cargo outdated
```

### 2. Pin a Specific Version

If an update isn't possible, pin to a patched version:

```toml
# Cargo.toml
[dependencies]
hyper = "0.14.28"  # Patched version
```

### 3. Replace the Dependency

If no fix is available, consider alternatives:

```toml
# Cargo.toml
[dependencies]
# ureq instead of reqwest if it uses vulnerable hyper
ureq = "2.9"
```

### 4. Ignore (With Justification)

For false positives or accepted risks, create `.cargo/audit.toml`:

```toml
# .cargo/audit.toml
[advisories]
ignore = [
    "RUSTSEC-2023-0043",  # Only affects HTTP/1.1 and we use HTTP/2
]
```

**Warning**: Document why you're ignoring each advisory.

## CI Integration

### GitHub Actions

```yaml
# .github/workflows/security.yml
name: Security Audit

on:
  push:
    paths:
      - '**/Cargo.toml'
      - '**/Cargo.lock'
  schedule:
    - cron: '0 0 * * *'  # Daily at midnight

jobs:
  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: rustsec/audit-check@v1.4.1
        with:
          token: ${{ secrets.GITHUB_TOKEN }}
```

### GitLab CI

```yaml
# .gitlab-ci.yml
security-audit:
  image: rust:latest
  script:
    - cargo install cargo-audit
    - cargo audit
  only:
    changes:
      - Cargo.toml
      - Cargo.lock
  allow_failure: false
```

## Advisory Database

The advisory database is automatically updated on each run. To manually update:

```bash
cargo audit fetch
```

Database location: `~/.cargo/advisory-db/`

## Advanced Usage

### JSON Output

For programmatic processing:

```bash
cargo audit --json > audit-report.json
```

### Deny Warnings

Treat unmaintained crates as errors:

```bash
cargo audit --deny warnings
```

### Check Specific File

Audit a different lock file:

```bash
cargo audit --file /path/to/Cargo.lock
```

## Best Practices

1. **Run in CI** - Block PRs with vulnerabilities
2. **Schedule daily checks** - Catch new advisories quickly
3. **Update regularly** - Keep dependencies current
4. **Document exceptions** - Justify any ignored advisories
5. **Monitor changelogs** - Watch for security-related updates

## Related Commands

| Command | Purpose |
|---------|---------|
| `cargo audit` | Check for vulnerabilities |
| `cargo update` | Update dependencies |
| `cargo outdated` | Show outdated deps |
| `cargo deny check` | More comprehensive checks |
| `cargo vet` | Supply chain verification |

## Additional Tools

For more comprehensive security checking, consider:

### cargo-deny

```bash
cargo install cargo-deny
cargo deny check
```

Checks licenses, bans, advisories, and source verification.

### cargo-vet

```bash
cargo install cargo-vet
cargo vet
```

Verifies that dependencies have been audited by trusted organizations.

## Next Steps

- [Getting Started](getting-started.md) - Project setup
- [Configuration](configuration/overview.md) - Secure configuration
- [Middleware Security](middleware/security.md) - Security middleware
