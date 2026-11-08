//! Concurrent `install_metrics()` calls must install the recorder exactly once.
#![cfg(feature = "metrics")]

use axum_conf::Config;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_installs_share_one_recorder() {
    let mut config = Config::new();
    config.http.with_metrics = true;

    let tasks: Vec<_> = (0..16)
        .map(|_| {
            let config = config.clone();
            tokio::spawn(async move { config.install_metrics().expect("enabled").1 })
        })
        .collect();

    let mut handles = Vec::new();
    for task in tasks {
        handles.push(task.await.expect("no panic from a double install"));
    }

    metrics::counter!("concurrent_install_total").increment(1);
    for handle in handles {
        assert!(handle.render().contains("concurrent_install_total 1"));
    }
}
