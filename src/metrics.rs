use lazy_static::lazy_static;

#[cfg(feature = "server")]
pub struct MetricsHandles {
    pub meter: opentelemetry::metrics::Meter,
    pub req_counter: opentelemetry::metrics::Counter<u64>,
    pub err_counter: opentelemetry::metrics::Counter<u64>,
    pub latency_hist_ms: opentelemetry::metrics::Histogram<f64>,
    pub inflight: opentelemetry::metrics::UpDownCounter<i64>,
    pub queue_len: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub registry: prometheus::Registry,
    pub db_init_counter: opentelemetry::metrics::Counter<u64>,
}

lazy_static! {
    pub static ref PROM_REQ_COUNTER: prometheus::Counter = prometheus::Counter::with_opts(
        prometheus::Opts::new("peillute_requests_total", "Total des requêtes traitées")
    )
    .unwrap();
    pub static ref PROM_ERR_COUNTER: prometheus::Counter =
        prometheus::Counter::with_opts(prometheus::Opts::new(
            "peillute_requests_errors_total",
            "Total des requêtes en erreur"
        ))
        .unwrap();
    pub static ref PROM_LATENCY_HIST: prometheus::Histogram = prometheus::Histogram::with_opts(
        prometheus::HistogramOpts::new("peillute_request_duration_ms", "Durée des requêtes (ms)")
            .buckets(vec![
                1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0
            ])
    )
    .unwrap();
    pub static ref PROM_INFLIGHT: prometheus::Gauge = prometheus::Gauge::with_opts(
        prometheus::Opts::new("peillute_inflight", "Nombre de requêtes en cours")
    )
    .unwrap();
    pub static ref PROM_DB_USER_CREATED: prometheus::Counter =
        prometheus::Counter::with_opts(prometheus::Opts::new(
            "peillute_db_user_created_total",
            "Nombre d'utilisateurs créés"
        ))
        .unwrap();
    pub static ref PROM_DB_TRANSACTION_CREATED: prometheus::Counter =
        prometheus::Counter::with_opts(prometheus::Opts::new(
            "peillute_db_transaction_created_total",
            "Nombre de transactions créées"
        ))
        .unwrap();
}

#[cfg(feature = "server")]
pub fn init_metrics() -> MetricsHandles {
    use opentelemetry::metrics::MeterProvider;
    let resource = opentelemetry_sdk::Resource::new(vec![
        opentelemetry::KeyValue::new(
            opentelemetry_semantic_conventions::resource::SERVICE_NAME,
            "peillute",
        ),
        opentelemetry::KeyValue::new(
            opentelemetry_semantic_conventions::resource::SERVICE_VERSION,
            "0.1.0",
        ),
        opentelemetry::KeyValue::new("deployment.environment", "development"),
    ]);

    let registry = prometheus::Registry::new();

    registry
        .register(Box::new(PROM_REQ_COUNTER.clone()))
        .unwrap();
    registry
        .register(Box::new(PROM_ERR_COUNTER.clone()))
        .unwrap();
    registry
        .register(Box::new(PROM_LATENCY_HIST.clone()))
        .unwrap();
    registry.register(Box::new(PROM_INFLIGHT.clone())).unwrap();
    registry
        .register(Box::new(PROM_DB_USER_CREATED.clone()))
        .unwrap();
    registry
        .register(Box::new(PROM_DB_TRANSACTION_CREATED.clone()))
        .unwrap();

    let provider = opentelemetry_sdk::metrics::SdkMeterProvider::builder()
        .with_resource(resource)
        .build();

    opentelemetry::global::set_meter_provider(provider.clone());
    let meter = provider.meter("peillute");

    let req_counter = meter
        .u64_counter("peillute_requests_total")
        .with_description("Total des requêtes traitées")
        .init();
    let err_counter = meter
        .u64_counter("peillute_requests_errors_total")
        .with_description("Total des requêtes en erreur")
        .init();
    let latency_hist_ms = meter
        .f64_histogram("peillute_request_duration_ms")
        .with_description("Durée des requêtes (ms)")
        .init();
    let inflight = meter
        .i64_up_down_counter("peillute_inflight")
        .with_description("Nombre de requêtes en cours")
        .init();

    let db_init_counter = meter
        .u64_counter("peillute_db_init_count")
        .with_description("Nombre d'initialisations de la base de données")
        .init();

    let queue_len = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let q_clone = queue_len.clone();
    meter
        .f64_observable_gauge("peillute_queue_len")
        .with_description("Taille de la file interne")
        .with_callback(move |obs| {
            let v = q_clone.load(std::sync::atomic::Ordering::Relaxed) as f64;
            obs.observe(v, &[opentelemetry::KeyValue::new("component", "queue")]);
        })
        .init();

    MetricsHandles {
        meter,
        req_counter,
        err_counter,
        latency_hist_ms,
        inflight,
        queue_len,
        registry,
        db_init_counter,
    }
}

#[cfg(feature = "server")]
pub fn record_request(attrs: &[opentelemetry::KeyValue]) {
    let provider = opentelemetry::global::meter_provider();
    let meter = provider.meter("peillute");
    let counter = meter.u64_counter("peillute_requests_total").init();
    counter.add(1, attrs);
    PROM_REQ_COUNTER.inc();
}

#[cfg(feature = "server")]
pub fn record_error(attrs: &[opentelemetry::KeyValue]) {
    let provider = opentelemetry::global::meter_provider();
    let meter = provider.meter("peillute");
    let counter = meter.u64_counter("peillute_requests_errors_total").init();
    counter.add(1, attrs);
    PROM_ERR_COUNTER.inc();
}

#[cfg(feature = "server")]
pub fn record_latency(ms: f64, attrs: &[opentelemetry::KeyValue]) {
    let provider = opentelemetry::global::meter_provider();
    let meter = provider.meter("peillute");
    let histogram = meter.f64_histogram("peillute_request_duration_ms").init();
    histogram.record(ms, attrs);
    PROM_LATENCY_HIST.observe(ms);
}

#[cfg(feature = "server")]
pub fn record_inflight_delta(delta: i64, attrs: &[opentelemetry::KeyValue]) {
    let provider = opentelemetry::global::meter_provider();
    let meter = provider.meter("peillute");
    let counter = meter.i64_up_down_counter("peillute_inflight").init();
    counter.add(delta, attrs);
    if delta > 0 {
        for _ in 0..delta {
            PROM_INFLIGHT.inc();
        }
    } else {
        for _ in 0..-delta {
            PROM_INFLIGHT.dec();
        }
    }
}

#[cfg(feature = "server")]
pub struct OpGuardSimple {
    start: std::time::Instant,
    attrs: Vec<opentelemetry::KeyValue>,
    ok: bool,
}

#[cfg(feature = "server")]
impl OpGuardSimple {
    pub fn new(component: &str, operation: &str) -> Self {
        let attrs = vec![
            opentelemetry::KeyValue::new("component", component.to_string()),
            opentelemetry::KeyValue::new("operation", operation.to_string()),
        ];
        record_inflight_delta(1, &attrs);
        Self {
            start: std::time::Instant::now(),
            attrs,
            ok: true,
        }
    }

    pub fn mark_error(&mut self) {
        self.ok = false;
    }
}

#[cfg(feature = "server")]
impl Drop for OpGuardSimple {
    fn drop(&mut self) {
        record_inflight_delta(-1, &self.attrs);
        record_request(&self.attrs);
        if !self.ok {
            record_error(&self.attrs);
        }
        let ms = self.start.elapsed().as_secs_f64() * 1000.0;
        record_latency(ms, &self.attrs);
    }
}

#[cfg(feature = "server")]
pub struct ReqTimer(std::time::Instant);

#[cfg(feature = "server")]
impl ReqTimer {
    pub fn start() -> Self {
        Self(std::time::Instant::now())
    }
    pub fn stop_and_record(&self, handles: &MetricsHandles, attrs: &[opentelemetry::KeyValue]) {
        let ms = self.0.elapsed().as_secs_f64() * 1000.0;
        handles.latency_hist_ms.record(ms, attrs);
    }
}

#[cfg(feature = "server")]
pub async fn metrics_handler(registry: prometheus::Registry) -> axum::response::Response {
    use prometheus::Encoder;
    let metric_families = registry.gather();
    let mut buf = Vec::new();
    prometheus::TextEncoder::new()
        .encode(&metric_families, &mut buf)
        .unwrap();
    let body = String::from_utf8(buf).unwrap();

    axum::response::Response::builder()
        .status(200)
        .header("Content-Type", "text/plain; version=0.0.4")
        .body(axum::body::Body::from(body))
        .unwrap()
}

#[cfg(feature = "server")]
pub struct OpGuard {
    start: std::time::Instant,
    handles: std::sync::Arc<MetricsHandles>,
    attrs: Vec<opentelemetry::KeyValue>,
    ok: bool,
}

#[cfg(feature = "server")]
impl OpGuard {
    pub fn new(
        handles: std::sync::Arc<MetricsHandles>,
        attrs: impl Into<Vec<opentelemetry::KeyValue>>,
    ) -> Self {
        let attrs = attrs.into();
        handles.inflight.add(1, &attrs);
        Self {
            start: std::time::Instant::now(),
            handles,
            attrs,
            ok: true,
        }
    }

    pub fn mark_error(&mut self) {
        self.ok = false;
    }
}

#[cfg(feature = "server")]
impl Drop for OpGuard {
    fn drop(&mut self) {
        self.handles.inflight.add(-1, &self.attrs);
        self.handles.req_counter.add(1, &self.attrs);
        if !self.ok {
            self.handles.err_counter.add(1, &self.attrs);
        }
        let ms = self.start.elapsed().as_secs_f64() * 1000.0;
        self.handles.latency_hist_ms.record(ms, &self.attrs);
    }
}
