workspace "axum-conf" "A batteries-included Rust library for building production-ready Axum web services for Kubernetes." {

    model {

        # --- People ---------------------------------------------------------
        developer = person "Application Developer" {
            description "Builds production web services by composing routes and TOML configuration on top of axum-conf."
        }

        # --- The library being modelled -------------------------------------
        axumConf = softwareSystem "axum-conf" {
            description "Builder-pattern library that wraps axum::Router with a configurable middleware stack, health probes, auth, observability and resilience."
            tags "Library"

            # Containers ~ major crate modules / feature areas
            fluent = container "FluentRouter & Middleware Engine" {
                description "Builder-pattern wrapper around axum::Router that assembles the middleware stack in the correct order, runs the server and handles graceful shutdown."
                technology "Rust, axum, tower, tower-http"
                tags "Core"

                # Components ~ submodules of src/fluent
                router = component "FluentRouter" {
                    description "Core builder type. Holds state, config and the inner axum::Router; exposes route()/with_state()/with_readiness_check()."
                    technology "Rust (router.rs)"
                }
                builder = component "Middleware Orchestrator" {
                    description "setup_middleware() applies every layer innermost-to-outermost, then start() binds the listener and serves."
                    technology "Rust (builder.rs)"
                }
                auth = component "Authentication" {
                    description "OIDC bearer + auth-code flow, HTTP Basic/API key, proxy-OIDC headers and browser login redirect. Produces a unified AuthenticatedIdentity extractor."
                    technology "Rust (auth.rs, oidc_*, basic_auth, proxy_oidc)"
                }
                observability = component "Observability" {
                    description "Structured request logging with UUIDv7 correlation IDs, Prometheus metrics endpoint and OpenTelemetry/OTLP trace export."
                    technology "Rust (observability.rs, user_span.rs)"
                }
                request = component "Request Handling" {
                    description "Payload size limits, concurrency limiting, request deduplication and request-ID assignment."
                    technology "Rust (request.rs, dedup.rs)"
                }
                features = component "Feature Middleware" {
                    description "Routing, compression, CORS, security headers (Helmet), sessions, static files and liveness/readiness probes."
                    technology "Rust (features.rs)"
                }
                control = component "Traffic Control" {
                    description "Per-IP rate limiting, request timeout and outermost panic catching that returns 500 and keeps the server alive."
                    technology "Rust (control.rs)"
                }
                readiness = component "Readiness Hook" {
                    description "Composes app-supplied /ready checks with built-in database and circuit-breaker health, yielding 503 when not ready."
                    technology "Rust (readiness.rs)"
                }
                shutdown = component "Graceful Shutdown" {
                    description "Listens for SIGTERM, notifies subscribers and drains in-flight connections within the configured timeout."
                    technology "Rust (shutdown.rs)"
                }
            }

            config = container "Configuration" {
                description "Loads config/{RUST_ENV}.toml (or strings/builders), substitutes {{ ENV_VAR }} references and validates HTTP, database, OIDC, logging and middleware sections."
                technology "Rust, serde, toml, regex"
                tags "Core"
            }

            circuitBreaker = container "Circuit Breaker" {
                description "Per-target fail-fast resilience (closed/open/half-open state machine) for the database and external HTTP services."
                technology "Rust, dashmap (feature: circuit-breaker)"
                tags "Optional"
            }

            openapi = container "OpenAPI" {
                description "Generates an OpenAPI specification and serves a Scalar API reference UI."
                technology "Rust, utoipa, utoipa-scalar (feature: openapi)"
                tags "Optional"
            }
        }

        # --- External systems ----------------------------------------------
        runtime = softwareSystem "Tokio / Axum Runtime" {
            description "Async runtime and HTTP server hosting the service binary that embeds axum-conf."
            tags "External"
        }
        postgres = softwareSystem "PostgreSQL" {
            description "Relational database accessed through an sqlx connection pool."
            tags "External,Database"
        }
        keycloak = softwareSystem "Keycloak / OIDC Provider" {
            description "Identity provider issuing and signing JWTs; backs OIDC discovery, bearer validation and the authorization-code flow."
            tags "External"
        }
        proxy = softwareSystem "Authenticating Reverse Proxy" {
            description "oauth2-proxy / Nginx auth_request that performs OIDC and forwards identity via X-Auth-Request-* headers."
            tags "External"
        }
        otelCollector = softwareSystem "OpenTelemetry Collector" {
            description "Receives distributed traces over OTLP/gRPC."
            tags "External"
        }
        prometheus = softwareSystem "Prometheus" {
            description "Scrapes the /metrics endpoint for request counts and latencies."
            tags "External"
        }

        # --- Relationships: context ----------------------------------------
        developer -> axumConf "Composes routes & TOML config with" "Rust API"
        axumConf -> runtime "Runs on" "tokio::main"
        axumConf -> postgres "Pools connections to" "sqlx / TLS"
        axumConf -> keycloak "Validates JWTs & runs auth-code flow against" "OIDC / HTTPS"
        proxy -> axumConf "Forwards authenticated identity to" "HTTP headers"
        axumConf -> otelCollector "Exports traces to" "OTLP / gRPC"
        prometheus -> axumConf "Scrapes metrics from" "HTTP /metrics"

        # --- Relationships: container --------------------------------------
        developer -> fluent "Builds the router & middleware stack with" "Rust API"
        fluent -> config "Reads settings from"
        fluent -> circuitBreaker "Guards downstream calls via"
        fluent -> openapi "Mounts spec & docs UI from"
        config -> postgres "Builds connection pool to" "sqlx / TLS"
        circuitBreaker -> postgres "Wraps calls to"
        fluent -> runtime "Binds listener & serves on" "tokio"
        fluent -> keycloak "Authenticates against" "OIDC / HTTPS"
        proxy -> fluent "Sets identity headers on"
        fluent -> otelCollector "Exports traces to" "OTLP / gRPC"
        prometheus -> fluent "Scrapes /metrics from" "HTTP"

        # --- Relationships: component (within fluent) ----------------------
        developer -> router "Adds routes / state / readiness checks to" "Rust API"
        router -> config "Reads configuration from"
        router -> builder "Hands assembly to"
        builder -> auth "Layers"
        builder -> observability "Layers"
        builder -> request "Layers"
        builder -> features "Layers"
        builder -> control "Layers"
        builder -> readiness "Mounts"
        builder -> shutdown "Wires"
        auth -> keycloak "Validates JWTs / OIDC flow with" "HTTPS"
        proxy -> auth "Provides identity headers to"
        observability -> otelCollector "Exports traces to" "OTLP / gRPC"
        prometheus -> observability "Scrapes metrics from" "HTTP"
        readiness -> circuitBreaker "Checks health of"
        readiness -> config "Checks database health via"
        builder -> runtime "Serves on" "tokio"
    }

    views {

        systemContext axumConf "SystemContext" {
            include *
            autolayout lr
            description "Who uses axum-conf and the external systems a service built on it integrates with."
        }

        container axumConf "Containers" {
            include *
            autolayout lr
            description "Major crate modules of axum-conf and how they relate to external systems."
        }

        component fluent "Components" {
            include *
            autolayout lr
            description "Internal components of the FluentRouter & middleware engine."
        }

        styles {
            element "Person" {
                shape Person
                background #08427b
                color #ffffff
            }
            element "Software System" {
                background #1168bd
                color #ffffff
            }
            element "Library" {
                background #1168bd
                color #ffffff
            }
            element "External" {
                background #999999
                color #ffffff
            }
            element "Database" {
                shape Cylinder
            }
            element "Container" {
                background #438dd5
                color #ffffff
            }
            element "Core" {
                background #438dd5
                color #ffffff
            }
            element "Optional" {
                background #85bbf0
                color #000000
            }
            element "Component" {
                background #85bbf0
                color #000000
            }
        }
    }
}
