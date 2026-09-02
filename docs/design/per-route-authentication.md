# Design: Per-Route Authentication Method Selection

**Status:** Proposed — not yet implemented
**Motivating use case:** secure the Prometheus `/metrics` endpoint with HTTP Basic Auth while the
rest of the application authenticates with OIDC.

## 1. Problem

`axum-conf` currently picks **one** authentication method for the whole application. `[http.oidc]`
and `[http.basic_auth]` cannot both be configured unless OIDC auth-code flow is enabled
(`redirect_uri` set), and even then the two methods aren't scoped to different routes — they
coexist on **every** route, and which one handles a given request is decided by which credential
shape shows up on it (see §2.2), not by which route it is.

There is no way to say "route A always uses Basic Auth, route B always uses OIDC." The concrete
case driving this doc: the built-in `/metrics` endpoint should be reachable with a Prometheus
scrape credential (Basic Auth) while the rest of the app enforces OIDC bearer tokens — without
weakening OIDC enforcement anywhere else, and without accepting Basic Auth credentials on
application routes.

Today `/metrics` isn't protected by *either* method — see §2.4 for why.

## 2. Current architecture (constraints this design must respect)

### 2.1 The middleware stack is one linear chain, and layers only wrap what's already registered

`FluentRouter::setup_middleware()` (`src/fluent/builder.rs:124-191`) builds the app as a single
sequence of `.route(...)` / `.route_layer(...)` / `.layer(...)` calls, added innermost-to-outermost
(the **last** layer added is **outermost** and runs **first** on a request — documented in
`CLAUDE.md`'s middleware order table, generated from `MIDDLEWARE_ORDER` in
`src/fluent/tests/middleware/ordering.rs`). Crucially, per this codebase's own confirmed Axum
semantics: **a `.layer()`/`.route_layer()` call wraps only the routes already registered in the
router at the point it is called** — routes registered later are simply not covered by it. Call
order controls what's protected, not any per-route annotation.

Relevant positions today:

| # | Step | Notes |
|--:|---|---|
| 1 | `setup_protected_files` | registers protected static routes **before** auth |
| 2 | `setup_browser_login_redirect` | route_layer (`keycloak`) |
| 3 | `setup_oidc` | route_layer (`keycloak`) — Bearer JWT auth |
| 4 | `setup_basic_auth` | route_layer (`basic-auth`) |
| 5 | `setup_proxy_oidc` | route_layer |
| 6 | `setup_public_files` | registers public static routes **after** auth |
| … | … | … |
| 20 | `setup_metrics` | registers `/metrics` **and** applies the Prometheus instrumentation layer |

### 2.2 The existing OIDC + Basic Auth coexistence is credential-based, not route-based

When OIDC auth-code flow is enabled, `setup_basic_auth()` (`src/fluent/auth.rs:199-230`) runs
`basic_auth_middleware` with `passthrough = true`. Its logic
(`src/fluent/basic_auth.rs:212-244`):

```rust
pub(crate) async fn basic_auth_middleware(
    config: Arc<HttpBasicAuthConfig>,
    passthrough: bool,
    mut request: Request,
    next: axum::middleware::Next,
) -> Response {
    match try_authenticate(&config, request.headers()) {
        AuthOutcome::Authenticated(identity) => { /* insert identity, continue */ }
        AuthOutcome::InvalidCredentials(response) => response,     // always 401s
        AuthOutcome::NoCredentials if passthrough => next.run(request).await, // defer to OIDC
        AuthOutcome::NoCredentials => unauthorized_response(&config),
    }
}
```

Basic Auth is added at position 4, **after** OIDC's route_layer at position 3 — so it is
*outermost* and runs *first*. If the request carries Basic/API-Key credentials, Basic Auth
handles it and OIDC never runs; if not, it passes through to OIDC. This applies to **every**
route equally. `bearer_auth_middleware` (`src/fluent/oidc_bearer.rs:469-503`) has the mirror-image
passthrough gate on missing/invalid Bearer tokens. This is documented (accurately) in
`docs/features/keycloak.md:332-359`, "OIDC + Basic Auth Coexistence."

This is **not** what the motivating use case needs: routing by credential shape would let a
Basic-Auth-carrying request hit *any* application route, and an OIDC bearer token would be
accepted on `/metrics` — neither is desired. We need routing by **path**, not by which header
happens to be present.

### 2.3 Hard mutual exclusion outside auth-code-flow mode

`HttpConfig::validate()` (`src/config/http/mod.rs:490-498`) currently hard-errors:

```rust
#[cfg(all(feature = "basic-auth", feature = "keycloak"))]
if let (Some(_basic_auth), Some(oidc)) = (&self.basic_auth, &self.oidc)
    && !oidc.auth_code_flow_enabled()
{
    return Err(crate::Error::invalid_input(
        "Cannot configure both [http.basic_auth] and [http.oidc] in bearer-only mode. \
         Either enable auth code flow by setting redirect_uri, or choose one authentication method.",
    ));
}
```

i.e. today's flagship use case (bearer-only OIDC app-wide, Basic Auth on `/metrics` only) is
flatly rejected at config-load time. (Two more blocks at lines 500-514 apply the same exclusion
between `basic_auth`/`proxy_oidc` and `oidc`/`proxy_oidc`.)

`docs/features/basic-auth.md:278-285` states this even more bluntly ("You cannot use both
`basic-auth` and `keycloak` features together") without mentioning the auth-code-flow exception
that `keycloak.md` documents correctly — this doc is stale and should be corrected as part of
implementing this design (see §7).

### 2.4 Why `/metrics` isn't protected by anything today

`setup_metrics()` (`src/fluent/observability.rs:188-245`) registers `GET /metrics` and applies the
`axum-prometheus` instrumentation layer at position 20 — long after the auth route_layers at
positions 3-5 were applied to "the routes registered so far." Per §2.1, `/metrics` is therefore
**not wrapped by any auth layer today**, regardless of what `[http.oidc]`/`[http.basic_auth]` say.
This is the root blocker: you cannot retroactively protect a route with a `route_layer` that was
already applied earlier in the chain to a different route set.

### 2.5 The one existing precedent for "some routes see auth, others don't"

`StaticDirConfig.protected: bool` (`src/config/http/staticdir.rs:93`) splits static directories
into two registration call sites: `setup_protected_files()` (`src/fluent/router.rs:664-666`,
called at position 1, **before** the auth route_layers) and `setup_public_files()`
(`router.rs:646-648`, called at position 6, **after** them). Protection is pure registration
order — `setup_directories(bool)` (`router.rs:562-598`) itself is auth-agnostic; it just filters a
`Vec<StaticDirConfig>` and calls `nest_service`. This confirms registration order is the
established technique in this codebase for scoping auth to specific routes, but it only proves
the binary case (authenticated-by-whatever's-configured vs. not) — it has no precedent for routing
different paths to **different** auth *methods*.

### 2.6 Downstream compatibility surface

All methods that set identity write the same shape,
`request.extensions_mut().insert(Arc::new(identity: AuthenticatedIdentity))`
(`basic_auth.rs:225`, `oidc_bearer.rs:491`, `proxy_oidc.rs:35`), and
`AuthenticatedIdentity { method: AuthMethod, .. }` (`src/config/http/identity.rs:44-59`) records
which of `BasicAuth | ApiKey | Oidc | ProxyOidc` produced it. The role extractors —
`WithRole`/`AnyRole`/`AllRoles` (`src/config/http/role_extractors.rs`) — read only the identity
extension, never `AuthMethod` or any auth-specific state. **Any design that installs different
auth per route needs zero changes here**: as long as exactly one method inserts an
`AuthenticatedIdentity` per request, everything downstream keeps working unmodified.

One repo-wide gotcha worth carrying forward: `basic_auth_middleware`, `bearer_auth_middleware`,
and `proxy_oidc_middleware` all do an unconditional `.insert()` with no "already present" guard
(only `session_to_identity` checks first). If two of these ever ran **stacked** on the same
request, the outer one would silently clobber the inner one's identity. Any design that keeps
methods on strictly separate route subtrees (rather than stacking them) sidesteps this by
construction — which is one reason the recommended design below is preferred over one that stacks
per-request path checks inside each middleware.

## 3. Goals / non-goals

**Goals**
- Let `/metrics` require Basic Auth while the rest of the app requires OIDC, with zero credential
  leakage in either direction (an OIDC bearer token must not satisfy `/metrics`'s auth, and a
  Basic Auth credential must not satisfy an application route's OIDC requirement).
- Generalize beyond `/metrics`: an application's own routes should be able to opt into a specific
  auth method, independent of the app-wide default.
- Preserve `HttpConfig::validate()`'s existing mutual-exclusion protection for anyone who hasn't
  opted into per-route scoping — zero behavior change for existing deployments.
- Don't touch `MIDDLEWARE_ORDER` / the doc-synced `CLAUDE.md` table unless unavoidable — see §4 for
  why this ended up avoidable.

**Non-goals (v1)**
- OIDC- or Proxy-OIDC-secured route groups (Basic Auth only in phase 1 — see §7).
- Securing `nest_service`-mounted routes (e.g. `ServeDir`) — `route_layer` doesn't wrap nested
  services (documented in this codebase's own `FluentRouter::route_layer` doc comment).
- `[http.middleware]` include/exclude awareness of per-route-group auth.
- Sharing credentials between `[http.basic_auth]` and a route-group's own Basic Auth config.

## 4. Design space explored

Three candidate designs were drafted and independently scored by two reviewers against
consistency with existing codebase idioms, generality beyond `/metrics`, implementation risk,
backward compatibility, and testability. Full detail on the two rejected designs is in the
appendix (§8); summary:

| Design | Consistency | Generality | Risk (low=better) | Back-compat | Testability | Verdict |
|---|--:|--:|--:|--:|--:|---|
| **A. Path-scoped config** (`Include`/`Exclude` prefix list per auth method) | 8.5 | 8 | 4.5 | 6 | 7 | Rejected |
| **B. Isolated secured sub-router** (`secure_with_basic_auth` + merge) | 7 | 7 | 8.5 | 9 | 8 | **Recommended** |
| **C. Unified auth dispatcher** (single `MatchedPath`-keyed routing table) | 4.5 | 8 | 2 | 4 | 5 | Rejected |

Design **B** won on every risk/compatibility/testability axis while being only modestly behind on
raw generality — and that gap closes in phase 2 (§7) without inheriting the other designs'
structural costs. Both independent reviews converged on it. The deciding factors:

- It requires **zero changes** to `MIDDLEWARE_ORDER` or `CLAUDE.md` — `setup_metrics()` keeps its
  exact call position, so both doc-sync tests (`order_positions_are_contiguous_and_setups_unique`,
  `claude_md_table_matches_generated` in `ordering.rs`) are untouched.
- It requires **zero changes** to the three existing mutual-exclusion checks in
  `HttpConfig::validate()` — the new config field is one the existing checks never inspect, so
  there is no new conditional branch in shared validation logic that future callers must also
  reason about.
- It requires **zero changes** to `basic_auth_middleware`, `bearer_auth_middleware`, or
  `proxy_oidc_middleware` themselves — it purely recomposes already-battle-tested functions via
  `axum::Router::merge`/`route_layer`, which are this repo's own documented semantics (§2.1),
  applied at the granularity of a whole sub-router instead of a per-request path check.
- Its security boundary is **exact by construction** — a route is in a secured group or it isn't,
  determined by Rust code at startup — rather than a runtime `str::starts_with` prefix match
  (Design A's approach, inherited from `browser_redirect.rs`'s UX-only `skip_paths`, which has a
  real footgun: `paths = ["/metrics"]` would also match `/metricsfoo`).

## 5. Recommended design: isolated secured sub-router

### 5.1 Core mechanism

Add a new builder method to `src/fluent/auth.rs`, alongside the existing `setup_oidc`,
`setup_basic_auth`, `setup_proxy_oidc`:

```rust
/// Wraps the routes built by `routes` in their own, independent Basic Auth
/// `route_layer` and merges them into the router — independent of whatever
/// `[http.basic_auth]` / `[http.oidc]` govern the rest of the app.
///
/// Because the sub-router starts empty, its `route_layer` wraps exactly the
/// routes the closure adds (see `route_layer` semantics, §2.1) — nothing else
/// in the app is affected, and this group is affected by nothing else.
#[cfg(feature = "basic-auth")]
#[must_use]
pub fn secure_with_basic_auth<F>(mut self, config: &crate::HttpBasicAuthConfig, routes: F) -> Self
where
    F: FnOnce(axum::Router<State>) -> axum::Router<State>,
{
    let sub = routes(axum::Router::new());
    let auth_config = std::sync::Arc::new(config.clone());
    let secured = sub.route_layer(axum::middleware::from_fn(move |request, next| {
        basic_auth::basic_auth_middleware(std::sync::Arc::clone(&auth_config), false, request, next)
    }));
    self.inner = self.inner.merge(secured);
    self
}
```

`passthrough` is hardcoded `false` — a secured group's auth is never optional or credential-sniffed;
that's the entire point of this feature versus §2.2's existing coexistence mode. `basic_auth_middleware`
(`src/fluent/basic_auth.rs:212`) is called completely unmodified.

**Why this is correct, not just plausible:** per the confirmed semantics in §2.1, `route_layer`
wraps only the routes present in *that* router when called. `Router::new()` starts empty, so
`sub.route_layer(...)` wraps exactly what the closure registered. `self.inner.merge(secured)`
unions two already-fully-built route tables — merge does not retroactively re-wrap either side
with the other's previously-applied layers. So wherever this method is called in the builder
chain, the merged-in routes get *only* this dedicated Basic Auth layer, never the app's `[http.oidc]`
layer (which was already applied, earlier, to a route table that didn't yet contain these routes) —
and the app's OIDC layer never sees these routes at all, so it can't accidentally authenticate
them either. This is the same "registration order controls what's wrapped" principle already used
by `setup_protected_files`/`setup_public_files` (§2.5), applied via `merge()` instead of position
in the linear chain.

### 5.2 Wiring `/metrics` as the first caller

Add to `HttpConfig` (`src/config/http/mod.rs`, next to the existing `basic_auth` field at
lines 291-296):

```rust
/// Basic Auth / API Key credentials that secure ONLY `metrics_route`, independent
/// of [http.basic_auth] / [http.oidc]. When `None` (default), `/metrics` keeps
/// today's behavior: registered plainly, not wrapped by any auth layer.
#[cfg(feature = "basic-auth")]
#[serde(default)]
pub metrics_basic_auth: Option<HttpBasicAuthConfig>,
```

Add `metrics_basic_auth: None,` to `Default::default()` (next to `basic_auth: None,` at line 647).
In `validate()`, next to the existing `basic_auth_config.validate()?` call (lines 482-485):

```rust
#[cfg(feature = "basic-auth")]
if let Some(metrics_basic_auth) = &self.metrics_basic_auth {
    metrics_basic_auth.validate()?;
    if !cfg!(feature = "metrics") {
        return Err(crate::Error::invalid_input(
            "[http.metrics_basic_auth] is configured but the `metrics` feature is not \
             enabled; it has no effect. Add `metrics` to your Cargo.toml features.",
        ));
    }
    if !self.with_metrics {
        tracing::warn!(
            "[http.metrics_basic_auth] is configured but with_metrics is false; it has no effect."
        );
    }
}
```

None of the three existing mutual-exclusion blocks (mod.rs:490-514) reference `metrics_basic_auth`
— it is never evaluated by that code path, so `[http.oidc]` bearer-only **and**
`[http.metrics_basic_auth]` coexist with zero new exception carved into shared validation logic.

In `src/fluent/observability.rs`, `setup_metrics()` (lines 188-245) keeps calling `build_pair()`
exactly once (unchanged — still produces `prometheus_layer` + `metrics_handle` together), then
branches on the new config field instead of always doing a plain `.route()`:

```rust
let metrics_service = axum::routing::get(|| async move { metrics_handle.render() });

#[cfg(feature = "basic-auth")]
let mut router = if let Some(basic_auth_cfg) = self.config.http.metrics_basic_auth.clone() {
    self.secure_with_basic_auth(&basic_auth_cfg, |r| r.route(metrics_path, metrics_service))
} else {
    self.inner = self.inner.route(metrics_path, metrics_service);
    self
};
#[cfg(not(feature = "basic-auth"))]
let mut router = { self.inner = self.inner.route(metrics_path, metrics_service); self };

router.inner = router.inner.layer(prometheus_layer);
router
```

`setup_metrics()`'s call position inside `setup_middleware()` (`src/fluent/builder.rs:213`,
position 20) is **unchanged** — this is the entire reason this design avoids the
`MIDDLEWARE_ORDER`/`CLAUDE.md` cost. The Prometheus instrumentation `.layer()` still applies
afterward, at the same call site, to the whole router including the just-merged `/metrics` route
— identical to today's behavior, `with_ignore_pattern(metrics_path)` still excludes it from being
counted.

A note on layers `/metrics` still won't get, unchanged from today: because `/metrics` is
registered at position 20 (whether via a plain `.route()` or via this new merge), layers applied
at earlier positions via `.layer()` — `setup_user_span` (position 8), `setup_logging`
(position 19) — don't wrap it, exactly as today. This design doesn't change that; it's called out
here only so it isn't mistaken for a regression during review.

### 5.3 Use-case walkthrough

```toml
# config/prod.toml
[http.oidc]
issuer_url = "https://idp.example.com/realms/myrealm"
client_id  = "my-service"
client_secret = "{{ OIDC_CLIENT_SECRET }}"
# bearer-only mode: no redirect_uri -> every ordinary app route requires a valid JWT

[http.metrics_basic_auth]
mode = "basic"

[[http.metrics_basic_auth.users]]
username = "prometheus"
password = "{{ METRICS_SCRAPE_PASSWORD }}"
```

1. Positions 1-19 run as today; position 3's `setup_oidc()` applies `bearer_auth_middleware`'s
   `route_layer` to whatever routes exist so far — `/metrics` doesn't exist yet, so it isn't
   wrapped by OIDC (unchanged from today).
2. Position 20's `setup_metrics()` runs; `metrics_basic_auth` is `Some`, so `secure_with_basic_auth`
   builds a one-route sub-router, wraps it in its own Basic Auth `route_layer`, merges it in.
3. `.layer(prometheus_layer)` applies to the whole router afterward, as today.
4. Result: `GET /metrics` with no credentials → 401 from `basic_auth_middleware`. With correct
   Basic creds → 200 with the Prometheus body. With a **valid Bearer JWT** but no Basic Auth
   header → 401 — the app's OIDC token does not leak into `/metrics`'s auth, i.e. genuine
   per-route method selection rather than the credential-union passthrough of §2.2.
   `GET /any/app/route` with a valid Bearer and no Basic Auth header → 200, still governed by the
   untouched app-wide OIDC layer.

Omitting `[http.metrics_basic_auth]` (every existing deployment, by definition) reproduces
today's `/metrics` behavior byte-for-byte: registered plainly, unauthenticated by construction.
There is no onboarding ambiguity and no silent behavior change for anyone who doesn't opt in.

### 5.4 Generality beyond `/metrics`

`secure_with_basic_auth` is a public `FluentRouter<State>` method usable directly by application
authors, not a metrics-only special case — `setup_metrics()` is simply its first internal caller.
An app can secure its own route group the same way, independent of `[http.oidc]`:

```rust
use axum::routing::get;

let router = FluentRouter::without_state(config)?
    .secure_with_basic_auth(&admin_basic_auth_cfg, |r| {
        r.route("/admin/debug", get(debug_handler))
    })
    .setup_middleware()
    .await?; // app-wide OIDC still governs every other route
```

## 6. Testing plan

No existing test in this repo constructs a `Config` with both `[http.basic_auth]` and bearer-only
`[http.oidc]` to assert `validate()` rejects it, and no test combines OIDC + Basic Auth
coexistence with an actual per-route split — both categories need to be written from scratch,
following the `oneshot()`-based idiom already used in `src/fluent/tests/middleware/ordering.rs`
(e.g. `health_probes_are_reachable`, `application_route_traverses_request_id_and_security_layers`).

Extend `src/fluent/tests/middleware/metrics.rs`:
1. Bearer-only `[http.oidc]` + `[http.metrics_basic_auth]` configured. `GET /metrics` with no
   credentials → 401.
2. Same config. `GET /metrics` with correct Basic creds → 200, Prometheus body present.
3. Same config. `GET /metrics` with a **valid** Bearer JWT, no Basic Auth header → 401 (proves no
   credential leakage in either direction).
4. Same config. `GET /some/app/route` with a valid Bearer, no Basic Auth header → 200 (app-wide
   OIDC unaffected by the metrics-specific scoping).
5. A `secure_with_basic_auth` test applied to an arbitrary user route (not `/metrics`), proving
   generality independent of the built-in wiring.

Extend `src/config/http/mod.rs`'s test module: `[http.metrics_basic_auth]` with invalid credentials
fails `HttpBasicAuthConfig::validate()`'s existing checks (reused unmodified); configured without
the `metrics` feature compiled in fails with the new explicit error; configured with `with_metrics
= false` warns but doesn't error.

## 7. Phased implementation plan

**Phase 1 — ships the motivating use case.**
- `secure_with_basic_auth` in `src/fluent/auth.rs`.
- `metrics_basic_auth` field + `validate()` block in `src/config/http/mod.rs`.
- `setup_metrics()` branch in `src/fluent/observability.rs`.
- Tests per §6.
- Fix the stale claim in `docs/features/basic-auth.md:278-285` ("You cannot use both `basic-auth`
  and `keycloak` features together") to reconcile with `keycloak.md:332-359`'s already-accurate
  auth-code-flow coexistence section, and add a new section documenting `secure_with_basic_auth` /
  `[http.metrics_basic_auth]`, following this repo's established feature-doc skeleton (Enable
  Feature → Configuration → Rust usage → Error Responses → Next Steps).

**Phase 2 — method parity.** Factor the `JwksProvider::new(...).await` + `BearerAuthConfig`
construction currently inlined in `setup_oidc()` (`src/fluent/auth.rs:77-100`) into a small shared
private helper, then add:
- `secure_with_oidc` (async, wraps `bearer_auth_middleware`, `src/fluent/oidc_bearer.rs:469`)
- `secure_with_proxy_oidc` (wraps `proxy_oidc_middleware`, `src/fluent/proxy_oidc.rs:19`)

so any route group can be scoped to any of the three methods symmetrically, not just Basic Auth.
This is mechanically identical to Phase 1's method, swapping which existing, path-agnostic
middleware function gets wrapped — the risk here is purely the JWKS-construction refactor, not new
design surface.

**Phase 3 — stretch, only on real demand.**
- `secure_service_with_basic_auth` (and OIDC/proxy-OIDC variants) using `.layer()` instead of
  `route_layer()`, to cover `nest_service`-mounted routes (static dirs, proxied services) — out of
  scope for v1 per §3.
- Revisit whether `[http.middleware]` include/exclude should gain awareness of user-registered
  `secure_with_*` groups.
- Revisit Design A's `Include`/`Exclude` path-scoping idiom (§8.1) *only* if users need pure-TOML,
  no-Rust-code scoping for routes they don't control directly — it remains a reasonable future
  extension, just not the v1 mechanism, given its `MIDDLEWARE_ORDER` cost and prefix-match
  precision tradeoffs.

## 8. Appendix: rejected alternatives

### 8.1 Design A — path-scoped config (`Include`/`Exclude` prefix lists per auth method)

Give each of `HttpBasicAuthConfig`/`HttpOidcConfig`/`HttpProxyOidcConfig` a `paths: AuthScope`
field (an `All`/`Include(Vec<String>)`/`Exclude(Vec<String>)` enum mirroring
`HttpMiddlewareConfig`'s existing Include/Exclude idiom, `src/config/http/middleware.rs:29-33`).
Each auth middleware gets an early-return: skip enforcement entirely if the request path doesn't
match its scope (reusing `browser_redirect.rs`'s proven `str::starts_with` prefix-match
technique). To bring `/metrics` into the chain, split `setup_metrics()` so its route registration
moves to a new early step (mirroring `setup_protected_files`), while its Prometheus instrumentation
layer stays put.

**Why not:** the strongest pull here is that it reuses two idioms already established in this
exact codebase, and it's genuinely free for arbitrary application routes (any route registered
before `setup_middleware()` already sits inside every auth `route_layer`; scoping it becomes pure
TOML). But: prefix-only matching promotes a UX-only technique (originally just a redirect-skip
list) into a security-enforcement boundary with a real footgun (`paths = ["/metrics"]` also
matches `/metricsfoo`). The scope-disjointness proof needed to safely relax `validate()`'s
mutual-exclusion checks can't handle `Exclude`/`Exclude` pairs or anything involving bare `All`,
forcing an unintuitive all-`Include` workaround. Moving `/metrics`'s registration earlier requires
renumbering all 27 `MIDDLEWARE_ORDER` positions and regenerating the `CLAUDE.md` table for what is,
conceptually, a single insertion, plus new cross-step mutable state on `FluentRouter` to carry the
Prometheus layer across the split. And its own draft flagged an unresolved backward-compatibility
gap: registering `/metrics` early by default (whenever the `metrics` feature is compiled in) would
silently wrap every existing app-wide-OIDC deployment's `/metrics` in OIDC the moment this ships,
unless gated behind an opt-in that wasn't fully specified.

### 8.2 Design C — unified auth dispatcher

Replace the three independently-stacked route_layers (`setup_oidc`/`setup_basic_auth`/
`setup_proxy_oidc`) with one `setup_auth_dispatch()` route_layer that resolves the request's path
(via `axum::extract::MatchedPath`, unused anywhere else in this codebase today) against a
config-driven `path → method` table, and invokes exactly one of the existing middleware functions
internally per request.

**Why not:** theoretically the most complete vision (one routing table, any of the three methods,
covers arbitrary routes), but by far the riskiest of the three. It introduces a dependency on
`MatchedPath` at an early `route_layer` position whose behavior in this router's
`nest_service`/`merge`-heavy construction (static dirs, the OIDC login/callback sub-router) is
asserted by the proposal, not verified against this codebase. It has the widest blast radius (7+
files: a new `auth_routing.rs`, a new `auth_dispatch.rs`, `config/http/mod.rs`, `builder.rs`,
`observability.rs`, `browser_redirect.rs`'s config-building call site, plus both `MIDDLEWARE_ORDER`
doc-sync tests). And its own draft admits an unresolved product question: whether adopting the
dispatcher fully replaces §2.2's auth-code-flow passthrough coexistence for non-adopters, or must
run alongside it — and if alongside, the dispatcher's `default` selector needs a compound
"OIDC-or-Basic-Auth" case that reintroduces the exact credential-sniffing ambiguity per-route
dispatch was meant to remove. Hardcoding `passthrough = false` for dispatcher-routed calls also
regresses UX for today's mixed-credential clients (a Basic-Auth header sent to an OIDC-routed path
now gets a generic "Missing Bearer token" 401 instead of dual-credential inspection).

## 9. Open questions carried into implementation

- Should `secure_with_basic_auth` take `&HttpBasicAuthConfig` (borrowed, matching
  `setup_basic_auth`'s existing pattern) or an owned value? Borrowed, for consistency — noted here
  as a decision, not left ambiguous.
- Should `setup_metrics()` warn (not just silently allow) if both `[http.basic_auth]` and
  `[http.metrics_basic_auth]` are configured with what looks like duplicated credentials, or is
  silent independence acceptable? Leaning toward silent — duplication may be intentional (e.g. the
  same operator both scrapes metrics and calls the API with a shared credential store) — but flag
  for review during Phase 1.
- `BrowserRedirectConfig.skip_paths` (§2.6's gotcha, `browser_redirect.rs:22-26`) is a separate,
  uncoordinated path list from anything this design introduces. It doesn't need to change for
  Phase 1: per §2.1's registration-order rule, `setup_browser_login_redirect`'s `route_layer`
  (position 2) only wraps routes registered before it, i.e. `setup_protected_files`'s routes —
  `/metrics`, merged in later at position 20, is never wrapped by it and so never redirects. This
  should still get an explicit regression test (rather than being assumed safe) once
  `secure_with_oidc` (Phase 2) makes browser-redirect-adjacent routes a realistic target for this
  mechanism.
