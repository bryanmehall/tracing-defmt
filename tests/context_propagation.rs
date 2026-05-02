use opentelemetry::propagation::{Extractor, Injector, TextMapPropagator};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use std::collections::HashMap;

// =========================================================================
// Mocks for standard Tokio/OpenTelemetry Carrier
// =========================================================================

struct HeaderInjector<'a>(&'a mut HashMap<String, String>);

impl<'a> Injector for HeaderInjector<'a> {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key.to_string(), value);
    }
}

struct HeaderExtractor<'a>(&'a HashMap<String, String>);

impl<'a> Extractor for HeaderExtractor<'a> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|s| s.as_str())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}

// =========================================================================
// Mock of the future `tracing-defmt` Context API (for no_std use)
// =========================================================================

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DefmtTraceContext {
    pub trace_id: [u8; 16],
    pub span_id: [u8; 8],
    pub trace_flags: u8,
}

impl DefmtTraceContext {
    // Generates a new context (mocked)
    pub fn new() -> Self {
        Self {
            trace_id: [1; 16],
            span_id: [2; 8],
            trace_flags: 1,
        }
    }

    // Creates a child context (same trace_id, new span_id)
    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id,
            span_id: [3; 8], // New mock span ID
            trace_flags: self.trace_flags,
        }
    }

    // Export to W3C format (e.g. to send over MQTT/CoAP to host)
    pub fn to_traceparent(&self) -> String {
        format!(
            "00-{}-{}-01",
            hex::encode(self.trace_id),
            hex::encode(self.span_id)
        )
    }

    // Import from W3C format
    pub fn from_traceparent(header: &str) -> Option<Self> {
        let parts: Vec<&str> = header.split('-').collect();
        if parts.len() != 4 {
            return None;
        }
        let mut trace_id = [0u8; 16];
        hex::decode_to_slice(parts[1], &mut trace_id).ok()?;

        let mut span_id = [0u8; 8];
        hex::decode_to_slice(parts[2], &mut span_id).ok()?;

        Some(Self {
            trace_id,
            span_id,
            trace_flags: 1,
        })
    }
}

// =========================================================================
// Tests
// =========================================================================

// Test 1: Defmt -> Tokio
#[test]
fn test_defmt_to_tokio_propagation() {
    // 1. Defmt device creates a root context
    let device_ctx = DefmtTraceContext::new();

    // 2. Device injects it into a "message" (e.g. CoAP or HTTP header)
    let mut message_headers = HashMap::new();
    message_headers.insert("traceparent".to_string(), device_ctx.to_traceparent());

    // 3. Tokio Server receives the message and extracts the context
    let propagator = TraceContextPropagator::new();
    let extractor = HeaderExtractor(&message_headers);
    let otel_context = propagator.extract(&extractor);

    // 4. Assert Tokio Server properly extracted the TraceId
    // We expect the TraceId to match what the defmt device generated
    let span_ref = opentelemetry::trace::TraceContextExt::span(&otel_context);
    let extracted_trace_id = span_ref.span_context().trace_id().to_bytes();

    assert_eq!(extracted_trace_id, device_ctx.trace_id);
}

// Test 2: Tokio -> Defmt
#[test]
fn test_tokio_to_defmt_propagation() {
    use opentelemetry::trace::{SpanContext, SpanId, TraceFlags, TraceId, TraceState};

    // 1. Tokio Server generates a trace context (mocking active span context)
    let tokio_span_context = SpanContext::new(
        TraceId::from_bytes([5; 16]),
        SpanId::from_bytes([6; 8]),
        TraceFlags::SAMPLED,
        false,
        TraceState::default(),
    );

    use opentelemetry::trace::TraceContextExt;
    let otel_context =
        opentelemetry::Context::new().with_remote_span_context(tokio_span_context.clone());

    // 2. Tokio Server injects it into a message
    let mut message_headers = HashMap::new();
    let propagator = TraceContextPropagator::new();
    let mut injector = HeaderInjector(&mut message_headers);
    propagator.inject_context(&otel_context, &mut injector);

    let traceparent_header = message_headers
        .get("traceparent")
        .expect("Should have traceparent injected by Tokio");

    // 3. Defmt device receives the message and parses the W3C header
    let device_ctx = DefmtTraceContext::from_traceparent(traceparent_header)
        .expect("Defmt should parse traceparent");

    // 4. Assert Defmt device extracted the correct IDs
    assert_eq!(
        device_ctx.trace_id,
        tokio_span_context.trace_id().to_bytes()
    );
    assert_eq!(device_ctx.span_id, tokio_span_context.span_id().to_bytes());
}

// Test 3: Defmt -> Defmt
#[test]
fn test_defmt_to_defmt_propagation() {
    // 1. Node A creates a trace
    let node_a_ctx = DefmtTraceContext::new();

    // 2. Node A sends a W3C traceparent header over a raw protocol (e.g. CAN bus, UART)
    let wire_format = node_a_ctx.to_traceparent();

    // 3. Node B receives it and constructs a child context
    let node_b_parent_ctx =
        DefmtTraceContext::from_traceparent(&wire_format).expect("Node B should parse wire format");

    let node_b_child_ctx = node_b_parent_ctx.child();

    // 4. Assert they share the same trace ID, but have distinct span IDs
    assert_eq!(node_b_child_ctx.trace_id, node_a_ctx.trace_id);
    assert_ne!(node_b_child_ctx.span_id, node_a_ctx.span_id);
}
