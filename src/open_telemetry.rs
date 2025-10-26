use opentelemetry::trace::TracerProvider;
use opentelemetry::{KeyValue, global as otel_global};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{WithExportConfig, new_exporter};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::{
    Resource,
    logs::LoggerProvider as SdkLoggerProvider,
    trace::{self, TracerProvider as SdkTracerProvider},
};
use opentelemetry_semantic_conventions::resource::{SERVICE_NAME, SERVICE_VERSION};
use tracing_subscriber::{filter::FilterFn, prelude::*};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_tracing(
    traces_grpc_endpoint: &str,
    logs_http_endpoint: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let resource = Resource::new(vec![
        KeyValue::new(SERVICE_NAME, "peillute"),
        KeyValue::new(SERVICE_VERSION, "0.1.0"),
        KeyValue::new("deployment.environment", "development"),
    ]);

    let trace_exporter = new_exporter()
        .tonic()
        .with_endpoint(traces_grpc_endpoint)
        .build_span_exporter()?;

    let tracer_provider = SdkTracerProvider::builder()
        .with_config(trace::Config::default().with_resource(resource.clone()))
        .with_batch_exporter(trace_exporter, opentelemetry_sdk::runtime::Tokio)
        .build();

    let tracer = tracer_provider.tracer("peillute");

    let logs_exporter = new_exporter()
        .http()
        .with_endpoint(logs_http_endpoint)
        .with_timeout(std::time::Duration::from_secs(10))
        .build_log_exporter()?;

    let logger_provider = SdkLoggerProvider::builder()
        .with_batch_exporter(logs_exporter, opentelemetry_sdk::runtime::Tokio)
        .with_resource(resource)
        .build();

    let logger_provider_static: &'static SdkLoggerProvider = Box::leak(Box::new(logger_provider));
    let otel_log_layer = OpenTelemetryTracingBridge::new(logger_provider_static);

    otel_global::set_text_map_propagator(TraceContextPropagator::new());
    otel_global::set_tracer_provider(tracer_provider);

    let only_mine = FilterFn::new(|meta| {
        let t = meta.target();
        t.starts_with("peillute")
    });

    tracing_subscriber::registry()
        .with(only_mine)
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(true),
        )
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(otel_log_layer)
        .init();

    Ok(())
}
