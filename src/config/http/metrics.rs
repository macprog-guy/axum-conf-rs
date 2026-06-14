use serde::Deserialize;

/// How a [`MetricBucketsConfig`] entry matches recorded metric names.
///
/// Mirrors `metrics_exporter_prometheus::Matcher`. The default is [`Full`]
/// (exact name match).
///
/// In TOML the value is lowercase (`full`, `prefix`, `suffix`).
///
/// [`Full`]: MetricMatch::Full
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MetricMatch {
    /// Match a metric by its exact, full name.
    #[default]
    Full,
    /// Match every metric whose name starts with the configured string.
    Prefix,
    /// Match every metric whose name ends with the configured string.
    Suffix,
}

/// Per-metric Prometheus histogram bucket override.
///
/// By default `metrics-exporter-prometheus` renders histograms recorded through
/// the global `metrics` facade as Prometheus **summaries** (`{quantile=…}`),
/// which cannot be aggregated across replicas. Configuring explicit `le` upper
/// bounds turns a metric into a true bucketed **histogram**
/// (`_bucket{le=…}` / `_sum` / `_count`).
///
/// # Metric names are NOT package-prefixed
///
/// Metrics you record yourself via `metrics::histogram!("my_metric")` are stored
/// under their **raw** name. The `axum_conf_` prefix only applies to the
/// framework's own built-in HTTP series, so `metric` must be the raw recorded
/// name (e.g. `cdx_pictse_convert_acquire_wait_seconds`, *not*
/// `axum_conf_cdx_pictse_…`). Names are Prometheus-sanitized (`-` → `_`) when
/// matched.
///
/// # Examples
///
/// In TOML configuration:
/// ```toml
/// [[http.metrics_buckets]]
/// metric  = "cdx_pictse_convert_acquire_wait_seconds"
/// match   = "full"   # optional; defaults to "full"
/// buckets = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0]
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct MetricBucketsConfig {
    /// Raw recorded metric name (or the prefix/suffix when `match` is set
    /// accordingly). See the type-level note: facade metrics are not prefixed.
    pub metric: String,

    /// How `metric` is matched against recorded names. Defaults to
    /// [`MetricMatch::Full`] (exact match).
    #[serde(default)]
    pub r#match: MetricMatch,

    /// Ascending `le` upper bounds for the histogram. An empty list causes the
    /// entry to be ignored (and logged) at setup time, leaving that metric with
    /// its default (summary) rendering.
    pub buckets: Vec<f64>,
}

#[cfg(feature = "metrics")]
impl MetricBucketsConfig {
    /// Maps this entry's `metric` + `match` to the exporter's `Matcher`.
    pub(crate) fn to_matcher(&self) -> axum_prometheus::metrics_exporter_prometheus::Matcher {
        use axum_prometheus::metrics_exporter_prometheus::Matcher;
        let name = self.metric.clone();
        match self.r#match {
            MetricMatch::Full => Matcher::Full(name),
            MetricMatch::Prefix => Matcher::Prefix(name),
            MetricMatch::Suffix => Matcher::Suffix(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;

    const BASE: &str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
"#;

    #[test]
    fn metrics_buckets_default_when_absent() {
        let config: Config = BASE.parse().unwrap();
        assert!(config.http.metrics_buckets.is_empty());
        assert!(config.http.metrics_global_labels.is_empty());
        assert!(config.http.metrics_idle_timeout.is_none());
        assert!(config.http.metrics_upkeep_timeout.is_none());
    }

    #[test]
    fn metrics_buckets_entry_match_defaults_to_full() {
        let toml = format!(
            "{BASE}\n[[http.metrics_buckets]]\nmetric = \"widget_seconds\"\nbuckets = [0.01, 0.1, 1.0]\n"
        );
        let config: Config = toml.parse().unwrap();
        assert_eq!(config.http.metrics_buckets.len(), 1);
        let entry = &config.http.metrics_buckets[0];
        assert_eq!(entry.metric, "widget_seconds");
        assert_eq!(entry.r#match, super::MetricMatch::Full);
        assert_eq!(entry.buckets, vec![0.01, 0.1, 1.0]);
    }

    #[test]
    fn metrics_buckets_explicit_match_variants() {
        let toml = format!(
            "{BASE}\n\
             [[http.metrics_buckets]]\nmetric = \"a\"\nmatch = \"prefix\"\nbuckets = [1.0]\n\
             [[http.metrics_buckets]]\nmetric = \"b\"\nmatch = \"suffix\"\nbuckets = [2.0]\n"
        );
        let config: Config = toml.parse().unwrap();
        assert_eq!(config.http.metrics_buckets.len(), 2);
        assert_eq!(config.http.metrics_buckets[0].r#match, super::MetricMatch::Prefix);
        assert_eq!(config.http.metrics_buckets[1].r#match, super::MetricMatch::Suffix);
    }

    #[test]
    fn metrics_global_labels_and_timeouts_round_trip() {
        let toml = format!(
            "{BASE}\n\
             metrics_idle_timeout = \"5m\"\n\
             metrics_upkeep_timeout = \"10s\"\n\
             [http.metrics_global_labels]\n\
             service = \"widget\"\n\
             tier = \"prod\"\n"
        );
        let config: Config = toml.parse().unwrap();
        assert_eq!(
            config.http.metrics_global_labels.get("service").map(String::as_str),
            Some("widget")
        );
        assert_eq!(
            config.http.metrics_global_labels.get("tier").map(String::as_str),
            Some("prod")
        );
        assert_eq!(
            config.http.metrics_idle_timeout,
            Some(std::time::Duration::from_secs(300))
        );
        assert_eq!(
            config.http.metrics_upkeep_timeout,
            Some(std::time::Duration::from_secs(10))
        );
    }

    #[test]
    fn metrics_buckets_empty_list_parses() {
        // An empty bucket list is accepted by deserialization; it is skipped
        // (and logged) at setup time rather than rejected at parse time.
        let toml = format!(
            "{BASE}\n[[http.metrics_buckets]]\nmetric = \"empty\"\nbuckets = []\n"
        );
        let config: Config = toml.parse().unwrap();
        assert_eq!(config.http.metrics_buckets.len(), 1);
        assert!(config.http.metrics_buckets[0].buckets.is_empty());
    }

    #[cfg(feature = "metrics")]
    #[test]
    fn to_matcher_maps_each_variant() {
        use super::{MetricBucketsConfig, MetricMatch};
        use axum_prometheus::metrics_exporter_prometheus::Matcher;

        let full = MetricBucketsConfig {
            metric: "exact".into(),
            r#match: MetricMatch::Full,
            buckets: vec![1.0],
        };
        let prefix = MetricBucketsConfig {
            metric: "pre".into(),
            r#match: MetricMatch::Prefix,
            buckets: vec![1.0],
        };
        let suffix = MetricBucketsConfig {
            metric: "suf".into(),
            r#match: MetricMatch::Suffix,
            buckets: vec![1.0],
        };

        assert!(matches!(full.to_matcher(), Matcher::Full(n) if n == "exact"));
        assert!(matches!(prefix.to_matcher(), Matcher::Prefix(n) if n == "pre"));
        assert!(matches!(suffix.to_matcher(), Matcher::Suffix(n) if n == "suf"));
    }
}
