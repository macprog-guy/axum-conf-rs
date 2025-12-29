use crate::fluent::tests::{create_config_with_toml, create_test_router_with_static_files};
use crate::StaticDirRoute;
use axum::{body::Body, http::Request};
use tower::ServiceExt;

#[tokio::test]
async fn test_static_files_served_at_route() {
    let toml_dirs = r#"
[[http.directories]]
directory = "tests/test_static_files"
route = "/static"
    "#;

    let config = create_config_with_toml(toml_dirs);
    let app = create_test_router_with_static_files(config).await;

    // Test accessing index.html
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/static/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("Test Static File"));

    // Test accessing test.txt
    let response = app
        .oneshot(
            Request::builder()
                .uri("/static/test.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("This is a test file for static file serving."));
}

#[tokio::test]
async fn test_static_directory_with_index_html_auto_serving() {
    let toml_dirs = r#"
[[http.directories]]
directory = "tests/test_static_files"
route = "/static"
    "#;

    let config = create_config_with_toml(toml_dirs);
    let app = create_test_router_with_static_files(config).await;

    // Test accessing directory with trailing slash - should serve index.html
    let response = app
        .oneshot(
            Request::builder()
                .uri("/static/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("Test Static File"));
}

#[tokio::test]
async fn test_multiple_static_directories() {
    let toml_dirs = r#"
[[http.directories]]
directory = "tests/test_static_files"
route = "/assets"

[[http.directories]]
directory = "tests/test_fallback_files"
route = "/public"
    "#;

    let config = create_config_with_toml(toml_dirs);
    let app = create_test_router_with_static_files(config).await;

    // Test first directory
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/assets/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("Test Static File"));

    // Test second directory
    let response = app
        .oneshot(
            Request::builder()
                .uri("/public/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("Fallback Static File"));
}

#[tokio::test]
async fn test_fallback_static_directory() {
    let toml_dirs = r#"
[[http.directories]]
directory = "tests/test_fallback_files"
fallback = true
cache_max_age = 600
    "#;

    let config = create_config_with_toml(toml_dirs);
    let app = create_test_router_with_static_files(config).await;

    // Test accessing root - should serve index.html from fallback directory
    let response = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("Fallback Static File"));

    // Test accessing index.html directly through fallback
    let response = app
        .oneshot(
            Request::builder()
                .uri("/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("Fallback Static File"));
}

#[tokio::test]
async fn test_mixed_route_and_fallback_static_directories() {
    let toml_dirs = r#"
[[http.directories]]
directory = "tests/test_static_files"
route = "/static"

[[http.directories]]
directory = "tests/test_fallback_files"
fallback = true
    "#;

    let config = create_config_with_toml(toml_dirs);
    let app = create_test_router_with_static_files(config).await;

    // Test route-specific directory
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/static/test.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("This is a test file for static file serving."));

    // Test fallback directory
    let response = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("Fallback Static File"));

    // Test that application routes still work
    let response = app
        .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"test response");
}

#[tokio::test]
async fn test_static_directory_404_for_non_existent_file() {
    let toml_dirs = r#"
[[http.directories]]
directory = "tests/test_static_files"
route = "/static"
    "#;

    let config = create_config_with_toml(toml_dirs);
    let app = create_test_router_with_static_files(config).await;

    // Test accessing a non-existent file
    let response = app
        .oneshot(
            Request::builder()
                .uri("/static/non-existent.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn test_static_directory_config_parsing() {
    let toml_dirs = r#"
[[http.directories]]
directory = "tests/test_static_files"
route = "/static"

[[http.directories]]
directory = "tests/test_fallback_files"
fallback = true
    "#;

    let config = create_config_with_toml(toml_dirs);

    // Verify configuration was parsed correctly
    assert_eq!(config.http.directories.len(), 2);

    // First directory should be a route
    assert_eq!(
        config.http.directories[0].directory,
        "tests/test_static_files"
    );
    assert!(!config.http.directories[0].is_fallback());
    if let StaticDirRoute::Route(route) = &config.http.directories[0].route {
        assert_eq!(route, "/static");
    } else {
        panic!("Expected Route variant");
    }

    // Second directory should be a fallback
    assert_eq!(
        config.http.directories[1].directory,
        "tests/test_fallback_files"
    );
    assert!(config.http.directories[1].is_fallback());
}

#[tokio::test]
async fn test_no_static_directories_configured() {
    let toml_dirs = ""; // No directories configured

    let config = create_config_with_toml(toml_dirs);
    let app = create_test_router_with_static_files(config).await;

    // Test that application routes still work
    let response = app
        .clone()
        .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    // Test that non-existent routes return 404
    let response = app
        .oneshot(
            Request::builder()
                .uri("/non-existent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn test_static_files_respect_content_type() {
    let toml_dirs = r#"
[[http.directories]]
directory = "tests/test_static_files"
route = "/static"
    "#;

    let config = create_config_with_toml(toml_dirs);
    let app = create_test_router_with_static_files(config).await;

    // Test HTML file content-type
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/static/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let content_type = response
        .headers()
        .get("content-type")
        .expect("Content-Type header should be present");
    assert!(content_type.to_str().unwrap().contains("text/html"));

    // Test text file content-type
    let response = app
        .oneshot(
            Request::builder()
                .uri("/static/test.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let content_type = response
        .headers()
        .get("content-type")
        .expect("Content-Type header should be present");
    assert!(content_type.to_str().unwrap().contains("text/plain"));
}

#[tokio::test]
async fn test_static_files_with_cache_headers() {
    let toml_dirs = r#"
[[http.directories]]
directory = "tests/test_static_files"
route = "/static"
cache_max_age = 3600
    "#;

    let config = create_config_with_toml(toml_dirs);
    let app = create_test_router_with_static_files(config).await;

    // Test that Cache-Control header is present
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/static/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let cache_control = response
        .headers()
        .get("cache-control")
        .expect("Cache-Control header should be present");

    let cache_value = cache_control.to_str().unwrap();
    assert!(cache_value.contains("public"));
    assert!(cache_value.contains("max-age=3600"));
}

#[tokio::test]
async fn test_static_files_without_cache_headers() {
    let toml_dirs = r#"
[[http.directories]]
directory = "tests/test_static_files"
route = "/static"
    "#;

    let config = create_config_with_toml(toml_dirs);
    let app = create_test_router_with_static_files(config).await;

    // Test that Cache-Control header is NOT present when not configured
    let response = app
        .oneshot(
            Request::builder()
                .uri("/static/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    // Cache-Control should not be set
    assert!(response.headers().get("cache-control").is_none());
}

#[tokio::test]
async fn test_static_files_with_different_max_age_values() {
    // Test with max_age = 86400 (1 day)
    let toml_dirs = r#"
[[http.directories]]
directory = "tests/test_static_files"
route = "/static"
cache_max_age = 86400
    "#;

    let config = create_config_with_toml(toml_dirs);
    let app = create_test_router_with_static_files(config).await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/static/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let cache_control = response
        .headers()
        .get("cache-control")
        .expect("Cache-Control header should be present");

    let cache_value = cache_control.to_str().unwrap();
    assert!(cache_value.contains("public"));
    assert!(cache_value.contains("max-age=86400"));

    // Test with max_age = 0 (no caching)
    let toml_dirs_no_cache = r#"
[[http.directories]]
directory = "tests/test_static_files"
route = "/nocache"
cache_max_age = 0
    "#;

    let config_no_cache = create_config_with_toml(toml_dirs_no_cache);
    let app_no_cache = create_test_router_with_static_files(config_no_cache).await;

    let response_no_cache = app_no_cache
        .oneshot(
            Request::builder()
                .uri("/nocache/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response_no_cache.status(), 200);

    let cache_control_no_cache = response_no_cache
        .headers()
        .get("cache-control")
        .expect("Cache-Control header should be present even with max_age=0");

    let cache_value_no_cache = cache_control_no_cache.to_str().unwrap();
    assert!(cache_value_no_cache.contains("max-age=0"));
}

#[tokio::test]
async fn test_multiple_directories_with_different_cache_settings() {
    let toml_dirs = r#"
[[http.directories]]
directory = "tests/test_static_files"
route = "/short-cache"
cache_max_age = 300

[[http.directories]]
directory = "tests/test_fallback_files"
route = "/long-cache"
cache_max_age = 31536000
    "#;

    let config = create_config_with_toml(toml_dirs);
    let app = create_test_router_with_static_files(config).await;

    // Test short cache directory
    let response_short = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/short-cache/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response_short.status(), 200);

    let cache_control_short = response_short
        .headers()
        .get("cache-control")
        .expect("Cache-Control header should be present");

    let cache_value_short = cache_control_short.to_str().unwrap();
    assert!(cache_value_short.contains("max-age=300"));

    // Test long cache directory
    let response_long = app
        .oneshot(
            Request::builder()
                .uri("/long-cache/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response_long.status(), 200);

    let cache_control_long = response_long
        .headers()
        .get("cache-control")
        .expect("Cache-Control header should be present");

    let cache_value_long = cache_control_long.to_str().unwrap();
    assert!(cache_value_long.contains("max-age=31536000"));
}
