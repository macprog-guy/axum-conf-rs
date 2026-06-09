//! Canonical middleware application order — the single source of truth.
//!
//! [`setup_middleware`](crate::FluentRouter::setup_middleware) installs middleware
//! innermost-first; the **last layer added is the outermost** and runs *first* on
//! an incoming request. [`MIDDLEWARE_ORDER`] mirrors that call sequence exactly.
//! Two kinds of guard keep documentation and behavior honest:
//!
//! * [`tests::claude_md_table_matches_generated`] regenerates the Markdown table
//!   from [`MIDDLEWARE_ORDER`] and asserts `CLAUDE.md` embeds it verbatim, so the
//!   project's middleware table can never silently drift from this list.
//! * the behavioral tests drive real requests through the full stack and assert
//!   the observable ordering guarantees (panic recovery is outermost, the health
//!   probes are reachable without auth, every response carries a request id, …).

/// One step in the middleware application order.
struct Step {
    /// 1-based application position (`1` = innermost, applied first).
    pos: u16,
    /// The `setup_*` method that installs this step.
    setup: &'static str,
    /// What the step is responsible for.
    role: &'static str,
    /// Cargo feature that activates it (`None` = core or purely config-driven).
    feature: Option<&'static str>,
}

/// The canonical order in which `setup_middleware` applies layers, innermost
/// (position 1) to outermost. Reading top-to-bottom is "added order"; on an
/// incoming request the layers execute bottom-to-top (outermost first).
const MIDDLEWARE_ORDER: &[Step] = &[
    Step {
        pos: 1,
        setup: "setup_protected_files",
        role: "Protected static files (added before auth so the auth `route_layer` covers them)",
        feature: None,
    },
    Step {
        pos: 2,
        setup: "setup_browser_login_redirect",
        role: "Redirect unauthenticated browsers to the login route",
        feature: Some("keycloak"),
    },
    Step {
        pos: 3,
        setup: "setup_oidc",
        role: "OIDC authentication (bearer JWT and/or auth-code identity)",
        feature: Some("keycloak"),
    },
    Step {
        pos: 4,
        setup: "setup_basic_auth",
        role: "HTTP Basic Auth and API-key authentication",
        feature: Some("basic-auth"),
    },
    Step {
        pos: 5,
        setup: "setup_proxy_oidc",
        role: "Reverse-proxy header authentication (fail-closed in production)",
        feature: None,
    },
    Step {
        pos: 6,
        setup: "setup_public_files",
        role: "Public static files (added after auth so they need no credentials)",
        feature: None,
    },
    Step {
        pos: 7,
        setup: "setup_oidc_routes",
        role: "OIDC login / callback / logout routes",
        feature: Some("keycloak"),
    },
    Step {
        pos: 8,
        setup: "setup_user_span",
        role: "Record the authenticated username on the tracing span",
        feature: None,
    },
    Step {
        pos: 9,
        setup: "setup_session_handling",
        role: "Session cookie store (wraps the auth layers)",
        feature: Some("session"),
    },
    Step {
        pos: 10,
        setup: "setup_deduplication",
        role: "Request deduplication by request id",
        feature: Some("deduplication"),
    },
    Step {
        pos: 11,
        setup: "setup_concurrency_limit",
        role: "Max concurrent in-flight requests",
        feature: Some("concurrency-limit"),
    },
    Step {
        pos: 12,
        setup: "setup_max_payload_size",
        role: "Request body size limit",
        feature: Some("payload-limit"),
    },
    Step {
        pos: 13,
        setup: "setup_compression",
        role: "Response compression / request decompression",
        feature: Some("compression"),
    },
    Step {
        pos: 14,
        setup: "setup_path_normalization",
        role: "Trailing-slash path normalization",
        feature: Some("path-normalization"),
    },
    Step {
        pos: 15,
        setup: "setup_sensitive_headers",
        role: "Mark sensitive headers for redaction in logs",
        feature: Some("sensitive-headers"),
    },
    Step {
        pos: 16,
        setup: "setup_api_versioning",
        role: "Extract the API version from path / header / query",
        feature: Some("api-versioning"),
    },
    Step {
        pos: 17,
        setup: "setup_cors",
        role: "CORS preflight handling and response headers",
        feature: Some("cors"),
    },
    Step {
        pos: 18,
        setup: "setup_helmet",
        role: "Security headers (Helmet)",
        feature: Some("security-headers"),
    },
    Step {
        pos: 19,
        setup: "setup_logging",
        role: "Request / response logging",
        feature: None,
    },
    Step {
        pos: 20,
        setup: "setup_metrics",
        role: "Prometheus metrics layer and the `/metrics` endpoint",
        feature: Some("metrics"),
    },
    Step {
        pos: 21,
        setup: "setup_readiness",
        role: "Readiness probe endpoint (benefits from timeout / rate limiting)",
        feature: None,
    },
    Step {
        pos: 22,
        setup: "setup_timeout",
        role: "Request timeout boundary",
        feature: None,
    },
    Step {
        pos: 23,
        setup: "setup_rate_limiting",
        role: "Per-IP rate limiting (rejects excess load early)",
        feature: Some("rate-limiting"),
    },
    Step {
        pos: 24,
        setup: "setup_request_id",
        role: "Generate / propagate the `x-request-id` header (early, for tracing)",
        feature: None,
    },
    Step {
        pos: 25,
        setup: "setup_liveness",
        role: "Liveness probe endpoint (always reachable, very early)",
        feature: None,
    },
    Step {
        pos: 26,
        setup: "setup_catch_panic",
        role: "Panic recovery — catches panics from every inner layer (outermost)",
        feature: None,
    },
    Step {
        pos: 27,
        setup: "setup_fallback_files",
        role: "Fallback static files (must be installed last)",
        feature: None,
    },
];

/// Renders [`MIDDLEWARE_ORDER`] as a GitHub-flavored Markdown table. This is the
/// exact text expected to appear (between the sentinel comments) in `CLAUDE.md`.
fn render_order_table() -> String {
    let mut out =
        String::from("| # | Setup step | Responsibility | Feature |\n| --: | --- | --- | --- |");
    for step in MIDDLEWARE_ORDER {
        let feature = match step.feature {
            Some(f) => format!("`{f}`"),
            None => "—".to_string(),
        };
        out.push_str(&format!(
            "\n| {} | `{}` | {} | {} |",
            step.pos, step.setup, step.role, feature
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FluentRouter;
    use crate::fluent::tests::{create_test_config, prepare_config_for_test};
    use axum::{body::Body, http::Request, routing::get};
    use tower::ServiceExt;

    /// Sentinel comments that fence the generated table inside `CLAUDE.md`.
    const BEGIN: &str = "<!-- BEGIN GENERATED: middleware-order -->";
    const END: &str = "<!-- END GENERATED: middleware-order -->";

    #[test]
    fn order_positions_are_contiguous_and_setups_unique() {
        let mut seen = std::collections::HashSet::new();
        for (idx, step) in MIDDLEWARE_ORDER.iter().enumerate() {
            assert_eq!(
                step.pos as usize,
                idx + 1,
                "positions must be 1-based and contiguous (offending step: {})",
                step.setup
            );
            assert!(
                seen.insert(step.setup),
                "duplicate setup step in MIDDLEWARE_ORDER: {}",
                step.setup
            );
        }
    }

    /// Doc-sync guard: the canonical table generated from [`MIDDLEWARE_ORDER`]
    /// must match, byte-for-byte, the table embedded between the sentinel
    /// comments in `CLAUDE.md`. If this fails, copy the printed table over the
    /// region between the sentinels in `CLAUDE.md`.
    #[test]
    fn claude_md_table_matches_generated() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/CLAUDE.md");
        let md =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));

        let begin = md
            .find(BEGIN)
            .unwrap_or_else(|| panic!("CLAUDE.md is missing the `{BEGIN}` sentinel"));
        let region_start = begin
            + md[begin..]
                .find('\n')
                .expect("a newline must follow the BEGIN sentinel")
            + 1;
        let region_end = region_start
            + md[region_start..]
                .find(END)
                .unwrap_or_else(|| panic!("CLAUDE.md is missing the `{END}` sentinel"));

        let embedded = md[region_start..region_end].trim();
        let expected = render_order_table();
        assert_eq!(
            embedded, expected,
            "\nCLAUDE.md middleware table is out of sync with MIDDLEWARE_ORDER.\n\
             Replace the region between the sentinels with:\n\n{expected}\n"
        );
    }

    /// Catch-panic is outer to the request pipeline (not merely wrapping the leaf
    /// handler). A handler panic is converted to a `500` by the outermost layer;
    /// and because the request-id layer (position 24) is *inner* to catch-panic
    /// (26), the panic unwinds past it and the synthesized 500 carries **no**
    /// `x-request-id` — whereas a normal response does (see
    /// `application_route_traverses_request_id_and_security_layers`). That
    /// asymmetry distinguishes "catch-panic is outermost" from "catch-panic merely
    /// wraps the handler": were catch-panic to regress inward of request-id, the
    /// 500 would flow back out through it and gain the header, failing this test.
    #[tokio::test]
    async fn panic_recovery_is_outer_to_the_request_pipeline() {
        let config = prepare_config_for_test(create_test_config());
        let app = FluentRouter::without_state(config)
            .expect("router builds")
            .route(
                "/boom",
                get(|| async {
                    panic!("boom");
                    #[allow(unreachable_code)]
                    ""
                }),
            )
            .setup_middleware()
            .await
            .expect("middleware sets up")
            .into_inner();

        let resp = app
            .oneshot(Request::builder().uri("/boom").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            500,
            "CatchPanic (outermost) must convert a handler panic into a 500"
        );
        assert!(
            !resp.headers().contains_key("x-request-id"),
            "the panic-recovery 500 must not carry a request id — the request-id layer (24) is \
             inner to catch-panic (26), so it cannot stamp the outermost-synthesized response"
        );
    }

    /// The liveness and readiness probes are wired as endpoints by
    /// `setup_middleware` (validates positions 21 and 25 are reachable). Note the
    /// liveness endpoint is added *outer* to the request-id and Helmet layers
    /// (positions 24 and 18) precisely so it short-circuits as cheaply as
    /// possible — which is why those response headers are asserted on a normal
    /// application route below, not on `/live`.
    #[tokio::test]
    async fn health_probes_are_reachable() {
        let config = prepare_config_for_test(create_test_config());
        let live = config.http.liveness_route.clone();
        let ready = config.http.readiness_route.clone();

        let app = FluentRouter::without_state(config)
            .expect("router builds")
            .setup_middleware()
            .await
            .expect("middleware sets up")
            .into_inner();

        for route in [live.as_str(), ready.as_str()] {
            let resp = app
                .clone()
                .oneshot(Request::builder().uri(route).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_ne!(
                resp.status(),
                404,
                "health probe {route} must be wired by setup_middleware"
            );
        }
    }

    /// A normal application route traverses the full stack, so the request-id
    /// layer (position 24) stamps the response and, when enabled, Helmet
    /// (position 18) adds security headers (validates those layers wrap ordinary
    /// routes, unlike the short-circuiting health endpoints above).
    #[tokio::test]
    async fn application_route_traverses_request_id_and_security_layers() {
        let config = prepare_config_for_test(create_test_config());

        let app = FluentRouter::without_state(config)
            .expect("router builds")
            .route("/app", get(|| async { "ok" }))
            .setup_middleware()
            .await
            .expect("middleware sets up")
            .into_inner();

        // A request with no id: the request-id layer must *generate* one and
        // propagate it onto the response.
        let resp = app
            .clone()
            .oneshot(Request::builder().uri("/app").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            200,
            "the application route should respond 200"
        );
        assert!(
            resp.headers().contains_key("x-request-id"),
            "request-id layer (24) must stamp a generated id on every application response"
        );

        #[cfg(feature = "security-headers")]
        {
            let headers = resp.headers();
            assert!(
                headers.contains_key("x-content-type-options")
                    || headers.contains_key("content-security-policy")
                    || headers.contains_key("x-frame-options"),
                "Helmet (18) must apply a security header to application responses"
            );
        }

        // A request with a client-supplied id: it must be echoed unchanged.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/app")
                    .header("x-request-id", "order-probe-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok()),
            Some("order-probe-123"),
            "a client-supplied request id must be preserved on the response"
        );
    }
}
