//! This example demonstrates how to reconstruct OpenTelemetry traces on the host
//! from `defmt` logs that were instrumented with `tracing-defmt`.
//!
//! To run this example:
//! 1.  Instrument your embedded application with `#[tracing::instrument]`.
//! 2.  Pipe the output of `defmt-print` (or `probe-run`) into this tool.
//!     `cargo run --example host_trace_reconstructor`
//!
//! Implementation Note:
//! This tool uses a stateless `HashMap` approach. Because `tracing-defmt` now injects
//! W3C OpenTelemetry Trace Contexts into every log (`ctx=TID:SID parent=PID`), this
//! reconstructor is 100% resilient to concurrent tasks interleaving logs out of order.

use opentelemetry::trace::TracerProvider as _; // Import trait for .tracer()
use opentelemetry_sdk::trace::TracerProvider;
use opentelemetry_stdout::SpanExporter;
use std::collections::HashMap;
use std::io::{self, BufRead};
use tracing::{Level, info};
use tracing_subscriber::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize OpenTelemetry pipeline
    // We use the stdout exporter for demonstration.
    let exporter = SpanExporter::default();
    let provider = TracerProvider::builder()
        .with_simple_exporter(exporter)
        .build();
    let tracer = provider.tracer("host_trace_reconstructor");

    // Create a tracing subscriber with the OpenTelemetry layer
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
    let subscriber = tracing_subscriber::registry()
        .with(telemetry)
        // We can also add a fmt layer to see logs on stderr
        .with(tracing_subscriber::fmt::layer().with_writer(io::stderr));

    tracing::subscriber::set_global_default(subscriber)?;

    eprintln!("Listening for defmt logs on stdin...");

    // 2. Read logs from stdin
    let stdin = io::stdin();
    let lines = stdin.lock().lines();

    // 3. Process the logs statelessly
    process_logs(lines);

    // Ensure all spans are exported
    opentelemetry::global::shutdown_tracer_provider();

    Ok(())
}

/// Processes log lines statelessly using W3C Trace Context markers.
fn process_logs<I>(lines: I)
where
    I: Iterator<Item = Result<String, io::Error>>,
{
    // Maps a Span ID (hex string) to an active host tracing Span
    let mut active_spans: HashMap<String, tracing::Span> = HashMap::new();

    for line in lines {
        let Ok(line) = line else { continue };
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        // Check if the log frame contains a tracing-defmt context
        // Format: "ctx=TRACE_ID:SPAN_ID parent=PARENT_ID <payload>"
        if line.starts_with("ctx=") {
            let space_idx = line.find(' ').unwrap_or(line.len());
            let ctx_str = &line[4..space_idx];
            let mut split = ctx_str.split(':');

            // In a real production decoder, we would map the extracted 16-byte Trace ID
            // directly into the opentelemetry SDK's global context map.
            let _trace_id = split.next().unwrap();
            let span_id = split.next().unwrap();
            let payload = &line[space_idx + 1..];

            // Does it have a parent definition?
            if payload.starts_with("parent=") {
                let parent_space_idx = payload.find(' ').unwrap_or(payload.len());
                let _parent_id = &payload[7..parent_space_idx];
                let event_str = &payload[parent_space_idx + 1..];

                if event_str.starts_with("span_enter: ") {
                    let content = &event_str["span_enter: ".len()..];

                    // Parse "function_name(arg=val, ...)" or just "function_name"
                    let (name, args) = if let Some(idx) = content.find('(') {
                        let end = content.len().saturating_sub(1);
                        (&content[..idx], &content[idx + 1..end])
                    } else {
                        (content, "")
                    };

                    // For demonstration, we simply map it to an INFO span.
                    // A true OTel bridge would use dynamic names and propagate the parent context cleanly.
                    let span =
                        tracing::span!(Level::INFO, "device_span", function = name, args = args);
                    active_spans.insert(span_id.to_string(), span);
                } else if event_str.starts_with("span_exit: ") {
                    // Dropping the span from the map fires the `Drop` guard on the host,
                    // signaling to the OpenTelemetry subscriber that the span is closed.
                    active_spans.remove(span_id);
                }
            } else {
                // It is a standard log event, not a span boundary.
                // Reconstruct the execution environment by checking if its Span ID matches an active span.
                if let Some(span) = active_spans.get(span_id) {
                    let _guard = span.enter();
                    info!(target: "device_log", "{}", payload);
                } else {
                    // Orphaned log (perhaps the span_enter packet was dropped over the UART/TCP link)
                    info!(target: "device_log", "{}", payload);
                }
            }
        } else {
            // Uninstrumented legacy defmt log
            info!(target: "device_log", "{}", line);
        }
    }
}
