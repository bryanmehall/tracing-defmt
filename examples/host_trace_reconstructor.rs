//! This example demonstrates how to reconstruct OpenTelemetry traces on the host
//! from `defmt` logs that were instrumented with `tracing-defmt`.
//!
//! To run this example:
//! 1.  Instrument your embedded application with `#[tracing::instrument]`.
//! 2.  Pipe the output of `defmt-print` (or `probe-run`) into this tool.
//!     `cargo run --example host_trace_reconstructor`
//!
//! Implementation Note:
//! This tool uses a recursive function to process the log stream. This approach allows us
//! to use standard `tracing` RAII guards (`span.enter()`) naturally, as the host's
//! call stack mimics the embedded device's call stack. This ensures full compatibility
//! with `tracing-subscriber` layers like `tracing-opentelemetry`.

use opentelemetry::trace::TracerProvider as _; // Import trait for .tracer()
use opentelemetry_sdk::trace::TracerProvider;
use opentelemetry_stdout::SpanExporter;
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
    let mut lines = stdin.lock().lines();

    // 3. Process logs recursively
    process_scope(&mut lines);

    // Ensure all spans are exported
    opentelemetry::global::shutdown_tracer_provider();

    Ok(())
}

/// Recursively processes log lines to reconstruct the span hierarchy.
fn process_scope<I>(lines: &mut I)
where
    I: Iterator<Item = Result<String, io::Error>>,
{
    while let Some(Ok(line)) = lines.next() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(msg_start) = line.find("span_enter: ") {
            // Found a new span start
            let content = &line[msg_start + "span_enter: ".len()..];

            // Parse "function_name(arg=val, ...)" or just "function_name"
            let (name, args) = if let Some(idx) = content.find('(') {
                let end = content.len().saturating_sub(1); // strip trailing ')'
                (&content[..idx], &content[idx + 1..end])
            } else {
                (content, "")
            };

            // Create a new tracing span
            // Note: In a real tool, you might want to parse 'args' into typed fields.
            // Here we just attach the raw string.
            let span = tracing::span!(Level::INFO, "device_span", function = name, args = args);

            // Enter the span (RAII guard)
            let _guard = span.enter();

            // Recurse to process lines within this span
            process_scope(lines);

            // When process_scope returns (due to exit or EOF), _guard is dropped, closing the span.
        } else if let Some(msg_start) = line.find("span_exit: ") {
            // Found end of current span
            let _name = &line[msg_start + "span_exit: ".len()..];
            // We could verify 'name' matches the current span, but for simplicity we assume strict nesting.
            // Returning from here drops the guard in the caller, closing the span.
            return;
        } else {
            // Regular log line
            // It is recorded as an event within the current span context
            info!(target: "device_log", "{}", line);
        }
    }
}
