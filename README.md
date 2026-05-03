# tracing-defmt

![CI](https://github.com/bryanmehall/tracing-defmt/actions/workflows/ci.yml/badge.svg)

A syntax-compatible version of [tracing](https://github.com/tokio-rs/tracing) that outputs to [defmt](https://github.com/knurling-rs/defmt), for no_std environments. `tracing-defmt` includes support for OpenTelemetry context propagation.


## Usage

You can import the crate as `tracing` to minimize code changes across shared host/device libraries.

```rust
use tracing_defmt as tracing;

fn main() {
    let x = 42;
    tracing::info!("Hello world! x={}", x);
    tracing::warn!("Something might be wrong...");
}
```

### Async Task Concurrency (`#[instrument]`)

The `#[instrument]` attribute is fully supported for both synchronous and `async` functions.

When using an async executor like Embassy, `tracing-defmt` tracks OpenTelemetry contexts  across `await` yields. 

```rust
use tracing_defmt as tracing;

#[tracing::instrument]
async fn my_task(sensor_id: u8) {
    // This defmt log automatically carries the W3C OpenTelemetry Trace ID!
    tracing::debug!("Inside my_task, reading sensor");
    
    // If Embassy pauses this task and polls another one, the active Trace Context 
    // is instantly swapped. Concurrency logs will never be jumbled on the host.
    Timer::after_secs(1).await;
}
```

### Manual Spans

You can also use manual `span!` RAII guards exactly like the host `tracing` crate:

```rust
use tracing_defmt as tracing;
use tracing_defmt::Level;

let span = tracing::span!(Level::INFO, "task span");
let _enter = span.enter();
tracing::info!("info event inside task span");
```

### OpenTelemetry Interoperability

`tracing-defmt` natively generates and manages standard W3C 16-byte Trace IDs and 8-byte Span IDs on the microcontroller, to directly interface with external APIs and cloud services. 

```rust
use tracing_defmt as tracing;

#[tracing::instrument]
async fn send_telemetry_to_cloud() {
    // 1. Get the current automatically generated OpenTelemetry Context
    if let Some(ctx) = tracing_defmt::context::get_active() {
        // 2. Format it into a standard W3C header
        let traceparent = format!("00-{}-{}-01", hex::encode(ctx.trace_id), hex::encode(ctx.span_id));
        
        // 3. Send it to your cloud service! Your cloud traces will now seamlessly link 
        // back to the deep `defmt` execution logs of your microcontroller.
        // http_client.set_header("traceparent", traceparent);
    }
}
```

## Features & Limitations

- **TraceId Entropy**: You can optionally call `tracing_defmt::context::init(seed)` with a hardware-specific random seed (e.g., from a TRNG or Flash UID) on boot. This seeds the upper 8 bytes of the generated W3C Trace IDs, ensuring global uniqueness and preventing trace collisions across device reboots in your cloud backend.
- **ISR Pollution Prevention**: The `cortex-m` Cargo feature forces a read of  the ARM `IPSR` register. If a hardware interrupt fires and logs an event, it will cleanly bypass the active application task's Trace Context, preventing lower-level hardware events from falsely polluting higher-level spans.
- **Macros**: `trace!`, `debug!`, `info!`, `warn!`, `error!` map directly to their `defmt` counterparts.
- **Attributes**: `#[instrument]` is supported. Arguments must implement `defmt::Format`.
- **Fields**:
    - `tracing::field::display(x)` is supported via a wrapper that uses `defmt::Display2Format`.
    - `tracing::field::debug(x)` is supported via a wrapper that uses `defmt::Debug2Format`.
- **Spans**: `span!` macros (`info_span!`, etc.) accurately manage a lock-free global registry of OpenTelemetry Trace Contexts. You can safely `.enter()` spans and the context propagates across standard `.await` execution boundaries flawlessly.
- **Events**: `event!` macro maps to the corresponding log level macro.

## Decoding on the Host

`tracing-defmt` injects W3C context directly into the binary stream, host decoders are completely stateless. 
See `examples/host_trace_reconstructor.rs` for a complete example of how to parse these logs from stdin and route them directly into the standard `opentelemetry_sdk` for export to any OpenTelemetry compatible trace database.
