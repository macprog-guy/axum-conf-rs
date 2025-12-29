//! Tests for OpenTelemetry middleware setup

#[tokio::test]
#[cfg(feature = "opentelemetry")]
async fn test_opentelemetry_initialization() {
    use crate::Config;
    use crate::FluentRouter;
    use crate::config::OpenTelemetryConfig;

    let config = Config::default().with_opentelemetry_config(
        OpenTelemetryConfig::new("http://localhost:4317").with_service_name("test-service"),
    );
    // Should successfully initialize (even if endpoint is not reachable)
    let result = FluentRouter::without_state(config);
    assert!(result.is_ok());

    if let Ok(router) = result {
        // Setup OpenTelemetry - this should not fail even if endpoint is unreachable
        // since it's fire-and-forget for OTLP
        let result = router.setup_opentelemetry();
        assert!(result.is_ok());
    }
}
