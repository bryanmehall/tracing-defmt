# Refactoring Plan: Direct Binary Decoding for tracing-defmt

## Objective
Migrate from parsing textual `defmt` logs (via stdout) to directly decoding binary `defmt` frames on the host. This decouples the system from `probe-rs`'s text formatting and enables a robust, production-ready telemetry agent that can work with any transport (RTT, UART, UDP).

## Architecture

### 1. `tracing-defmt` (Device Crate)
*   **Role**: Minimal overhead instrumentation for `no_std` devices.
*   **Changes**:
    *   Simplify `#[instrument]` macro.
    *   Remove manual `file=`, `line=` formatting (since binary frames carry this metadata).
    *   Ensure `span_enter` and `span_exit` payloads are uniquely identifiable (perhaps using a specific bit/tag or just standardized strings).

### 2. `tracing-defmt-decoder` (New Host Library)
*   **Role**: Core logic for interpreting the binary stream and mapping it to OpenTelemetry traces.
*   **Dependencies**: `defmt-decoder`, `opentelemetry`, `tracing`.
*   **Key Components**:
    *   `Decoder`: Manages the `defmt` table (from ELF) and the state machine.
    *   `SpanTracker`: Reconstructs the call stack from `span_enter`/`span_exit` events.
    *   `OtelAdapter`: Converts decoded frames + span context into OpenTelemetry Spans and Events.
*   **API**:
    ```rust
    pub struct Tracer { ... }
    impl Tracer {
        pub fn new(elf_data: &[u8], otel_tracer: Box<dyn opentelemetry::trace::Tracer>) -> Self;
        pub fn feed(&mut self, data: &[u8]);
    }
    ```

### 3. `rp_pico/host` (Host Agent)
*   **Role**: The executable agent that connects the transport to the decoder.
*   **Changes**:
    *   Remove regex/stdin parsing logic.
    *   Add `probe-rs` dependency to read RTT directly.
    *   Load firmware ELF.
    *   Initialize `tracing-defmt-decoder`.
    *   Loop: Read RTT -> Feed Decoder.

## Implementation Steps

### Phase 1: Library Construction
1.  Initialize `tracing-defmt/decoder` crate.
2.  Implement `defmt-decoder` integration to parse raw frames.
3.  Implement state machine to track spans based on "span_enter" log messages.
    *   Use `defmt`'s `Frame::location()` to retrieve file/line info automatically.
4.  Connect to OpenTelemetry SDK to export spans.

### Phase 2: Device-Side Cleanup
1.  Update `tracing-defmt-macros` to emit clean `span_enter` messages without extra metadata text (reducing binary size).

### Phase 3: Integration
1.  Update `rp_pico/host` to depend on `tracing-defmt-decoder` and `probe-rs`.
2.  Implement RTT polling loop.
3.  Verify end-to-end tracing with Jaeger.

## Future Proofing
This design allows replacing `probe-rs` in `rp_pico/host` with a UART reader or UDP listener without changing the core decoding logic, making it suitable for deployed devices where a debugger is not attached.
