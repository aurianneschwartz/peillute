use lazy_static::lazy_static;
use opentelemetry::metrics::{Counter, Histogram, Meter, MeterProvider, UpDownCounter};
use opentelemetry::{KeyValue, global};
use opentelemetry_sdk::{Resource, metrics::SdkMeterProvider};
use opentelemetry_semantic_conventions::resource::{SERVICE_NAME, SERVICE_VERSION};
use prometheus::{
    Counter as PromCounter, Encoder, Gauge as PromGauge, Histogram as PromHistogram, Registry,
    TextEncoder,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{sync::Arc, time::Instant};

pub struct MetricsHandles {
    pub meter: Meter,
    pub req_counter: Counter<u64>,
    pub err_counter: Counter<u64>,
    pub latency_hist_ms: Histogram<f64>,
    pub inflight: UpDownCounter<i64>,
    pub queue_len: Arc<AtomicUsize>,
    pub registry: Registry,
    pub db_init_counter: Counter<u64>,
}

lazy_static! {
    pub static ref PROM_REQ_COUNTER: PromCounter = PromCounter::with_opts(prometheus::Opts::new(
        "peillute_requests_total",
        "Total des requêtes traitées"
    ))
    .unwrap();
    pub static ref PROM_ERR_COUNTER: PromCounter = PromCounter::with_opts(prometheus::Opts::new(
        "peillute_requests_errors_total",
        "Total des requêtes en erreur"
    ))
    .unwrap();
    pub static ref PROM_LATENCY_HIST: PromHistogram = PromHistogram::with_opts(
        prometheus::HistogramOpts::new("peillute_request_duration_ms", "Durée des requêtes (ms)")
            .buckets(vec![
                1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0
            ])
    )
    .unwrap();
    pub static ref PROM_INFLIGHT: PromGauge = PromGauge::with_opts(prometheus::Opts::new(
        "peillute_inflight",
        "Nombre de requêtes en cours"
    ))
    .unwrap();
    pub static ref PROM_DB_USER_CREATED: PromCounter =
        PromCounter::with_opts(prometheus::Opts::new(
            "peillute_db_user_created_total",
            "Nombre d'utilisateurs créés"
        ))
        .unwrap();
    pub static ref PROM_DB_TRANSACTION_CREATED: PromCounter =
        PromCounter::with_opts(prometheus::Opts::new(
            "peillute_db_transaction_created_total",
            "Nombre de transactions créées"
        ))
        .unwrap();
}

pub fn init_metrics() -> MetricsHandles {
    let resource = Resource::new(vec![
        KeyValue::new(SERVICE_NAME, "peillute"),
        KeyValue::new(SERVICE_VERSION, "0.1.0"),
        KeyValue::new("deployment.environment", "development"),
    ]);

    let registry = Registry::new();

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

    let provider = SdkMeterProvider::builder().with_resource(resource).build();

    global::set_meter_provider(provider.clone());
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

    let queue_len = Arc::new(AtomicUsize::new(0));
    let q_clone = queue_len.clone();
    meter
        .f64_observable_gauge("peillute_queue_len")
        .with_description("Taille de la file interne")
        .with_callback(move |obs| {
            let v = q_clone.load(Ordering::Relaxed) as f64;
            obs.observe(v, &[KeyValue::new("component", "queue")]);
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

pub fn record_request(attrs: &[KeyValue]) {
    let provider = global::meter_provider();
    let meter = provider.meter("peillute");
    let counter = meter.u64_counter("peillute_requests_total").init();
    counter.add(1, attrs);
    PROM_REQ_COUNTER.inc();
}

pub fn record_error(attrs: &[KeyValue]) {
    let provider = global::meter_provider();
    let meter = provider.meter("peillute");
    let counter = meter.u64_counter("peillute_requests_errors_total").init();
    counter.add(1, attrs);
    PROM_ERR_COUNTER.inc();
}

pub fn record_latency(ms: f64, attrs: &[KeyValue]) {
    let provider = global::meter_provider();
    let meter = provider.meter("peillute");
    let histogram = meter.f64_histogram("peillute_request_duration_ms").init();
    histogram.record(ms, attrs);
    PROM_LATENCY_HIST.observe(ms);
}

pub fn record_inflight_delta(delta: i64, attrs: &[KeyValue]) {
    let provider = global::meter_provider();
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

pub struct OpGuardSimple {
    start: Instant,
    attrs: Vec<KeyValue>,
    ok: bool,
}

impl OpGuardSimple {
    pub fn new(component: &str, operation: &str) -> Self {
        let attrs = vec![
            KeyValue::new("component", component.to_string()),
            KeyValue::new("operation", operation.to_string()),
        ];
        record_inflight_delta(1, &attrs);
        Self {
            start: Instant::now(),
            attrs,
            ok: true,
        }
    }

    pub fn mark_error(&mut self) {
        self.ok = false;
    }
}

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

pub struct ReqTimer(Instant);
impl ReqTimer {
    pub fn start() -> Self {
        Self(Instant::now())
    }
    pub fn stop_and_record(&self, handles: &MetricsHandles, attrs: &[KeyValue]) {
        let ms = self.0.elapsed().as_secs_f64() * 1000.0;
        handles.latency_hist_ms.record(ms, attrs);
    }
}

pub async fn metrics_handler(registry: Registry) -> axum::response::Response {
    let metric_families = registry.gather();
    let mut buf = Vec::new();
    TextEncoder::new()
        .encode(&metric_families, &mut buf)
        .unwrap();
    let body = String::from_utf8(buf).unwrap();

    axum::response::Response::builder()
        .status(200)
        .header("Content-Type", "text/plain; version=0.0.4")
        .body(axum::body::Body::from(body))
        .unwrap()
}

pub struct OpGuard {
    start: Instant,
    handles: Arc<MetricsHandles>,
    attrs: Vec<KeyValue>,
    ok: bool,
}

impl OpGuard {
    pub fn new(handles: Arc<MetricsHandles>, attrs: impl Into<Vec<KeyValue>>) -> Self {
        let attrs = attrs.into();
        handles.inflight.add(1, &attrs);
        Self {
            start: Instant::now(),
            handles,
            attrs,
            ok: true,
        }
    }

    pub fn mark_error(&mut self) {
        self.ok = false;
    }
}

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
