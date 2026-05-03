# tracing-defmt

![CI](https://github.com/bryanmehall/tracing-defmt/actions/workflows/ci.yml/badge.svg)

A **syntax-compatible** facade for [tracing](https://github.com/tokio-rs/tracing) that outputs directly to [defmt](https://github.com/knurling-rs/defmt), with **built-in OpenTelemetry distributed context propagation**.

Export just the trace and log data with [defmt](https://github.com/knurling-rs/defmt) and reconstruct the full traces on a host or on a connected server to export as OpenTelemetry for production observability.

## Overview

`tracing` is the de-facto standard for instrumentation in the Rust ecosystem. `defmt` is the gold standard for high-efficiency logging on embedded devices.

However, using `tracing` with a subscriber on embedded systems often forces a compromise:
1.  **Type Erasure**: `tracing` erases types into `dyn Value`, forcing the subscriber to use `fmt::Debug`.
2.  **Formatting on Device**: To log these erased values, one must typically use `defmt::Debug2Format`, which performs formatting on the device, negating `defmt`'s bandwidth and size savings.

`tracing-defmt` resolves this by providing macros that **look** like `tracing` macros but **expand** to `defmt` macros at compile time. 

More importantly, it provides a built-in OpenTelemetry Trace Context registry that seamlessly handles asynchronous task concurrency (like `embassy` or `rtic`), guaranteeing that interleaved logs on the device are always perfectly reconstructed on the host.

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
tracing-defmt = "0.1"
defmt = "0.3"
```

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

When using an async executor like Embassy, `tracing-defmt` tracks OpenTelemetry contexts precisely across `await` yields. Even if the executor interleaves dozens of tasks on a single core, logs are strictly bound to their correct trace hierarchy.

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

let span = tracing::span!(Level::INFO, "my_manual_span");
let _enter = span.enter();
tracing::info!("I am safely inside my_manual_span's OpenTelemetry context!");
```

### OpenTelemetry Interoperability

Because `tracing-defmt` natively generates and manages standard W3C 16-byte Trace IDs and 8-byte Span IDs on the microcontroller, you can directly interface with external APIs and cloud services. 

If your embedded device makes HTTP requests (or MQTT publishes), you can extract the active context to propagate traces end-to-end:

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

### Host vs Embedded Usage

Since `defmt` is designed for embedded targets and requires a global logger, running `tracing-defmt` code directly on a host machine (e.g. `cargo test`) usually produces no visible output.

To get the best of both worlds—**standard tracing logs on host** and **efficient defmt logs on embedded**—you should use conditional compilation in your `Cargo.toml`.

```toml
[features]
default = ["std"]
std = ["dep:tracing"]
embedded = ["dep:tracing-defmt", "dep:defmt"]

[dependencies]
tracing = { version = "0.1", optional = true }
tracing-defmt = { version = "0.1", optional = true }
defmt = { version = "0.3", optional = true }
```

```rust
#[cfg(feature = "std")]
use tracing;

#[cfg(feature = "embedded")]
use tracing_defmt as tracing;

#[tracing::instrument]
fn process_data(data: &[u8]) {
    tracing::info!("Processing {} bytes", data.len());
}
```

## Features & Limitations

- **TraceId Entropy**: You can optionally call `tracing_defmt::context::init(seed)` with a hardware-specific random seed (e.g., from a TRNG or Flash UID) on boot. This seeds the upper 8 bytes of the generated W3C Trace IDs, ensuring global uniqueness and preventing trace collisions across device reboots in your cloud backend.
- **ISR Pollution Prevention**: If you enable the `cortex-m` Cargo feature, `tracing-defmt` will automatically read the ARM `IPSR` register. If a hardware interrupt fires and logs an event, it will cleanly bypass the active application task's Trace Context, preventing lower-level hardware events from falsely polluting higher-level HTTP/MQTT spans.
- **Macros**: `trace!`, `debug!`, `info!`, `warn!`, `error!` map directly to their `defmt` counterparts.
- **Attributes**: `#[instrument]` is supported. Arguments must implement `defmt::Format`.
- **Fields**:
    - `tracing::field::display(x)` is supported via a wrapper that uses `defmt::Display2Format`.
    - `tracing::field::debug(x)` is supported via a wrapper that uses `defmt::Debug2Format`.
- **Spans**: `span!` macros (`info_span!`, etc.) accurately manage a lock-free global registry of OpenTelemetry Trace Contexts. You can safely `.enter()` spans and the context propagates across standard `.await` execution boundaries flawlessly.
- **Events**: `event!` macro maps to the corresponding log level macro.

## Decoding on the Host

Because `tracing-defmt` injects W3C context directly into the binary stream, host decoders are completely stateless. 
See `examples/host_trace_reconstructor.rs` for a complete example of how to parse these logs from stdin and route them directly into the standard `opentelemetry_sdk` for export to Jaeger, Zipkin, or Datadog.

## Testing

To run the test suite (which validates context propagation, async future instrumentation, and API compatibility):

```bash
cargo test
```