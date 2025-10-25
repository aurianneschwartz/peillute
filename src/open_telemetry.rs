use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::{
    trace::{TracerProvider as SdkTracerProvider, Config as TraceConfig},
    logs::LoggerProvider as SdkLoggerProvider,  
};
use opentelemetry_stdout::{SpanExporter, LogExporter};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_tracing() -> Result<(), Box<dyn std::error::Error>> {
    // TracerProvider 
    let trace_provider = SdkTracerProvider::builder()
        .with_simple_exporter(SpanExporter::default())
        .with_config(TraceConfig::default())
        .build();
    
    let tracer = trace_provider.tracer("peillute");

    // LoggerProvider 
    let log_provider = SdkLoggerProvider::builder()
        .with_simple_exporter(LogExporter::default())
        .build();  

    let otel_log_layer = OpenTelemetryTracingBridge::new(&log_provider);

    // Configuration
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(otel_log_layer)
        .init();

    Ok(())
}