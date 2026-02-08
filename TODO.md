# Production IoT Transport & Trace Reconstruction

This document outlines the steps required to transition from local debugging (via RTT/USB) to a production-ready IoT observability pipeline using `tracing-defmt`.

## 1. Device-Side: Custom Transport Implementation

To send logs over a network (MQTT, TCP, BLE, etc.) instead of RTT, you must implement a custom `defmt::Logger`.

- [ ] **Define Transport Protocol**:
    - Decide on a framing strategy (e.g., length-prefixed, COBS) to delineate log packets.
    - Ensure reliability (e.g., use TCP or MQTT QoS 1) if losing logs is unacceptable.
- [ ] **Implement `defmt::Logger`**:
    - Create a struct that implements `defmt::Logger`.
    - In `acquire()`, reserve a buffer.
    - In `write()`, copy bytes to the buffer.
    - In `release()`, flush the buffer to your transport (e.g., `mqtt_client.publish(...)`).
- [ ] **Versioning**:
    - Embed a unique firmware version ID (e.g., git hash) in the binary.
    - Send this ID as a "handshake" or header in the log stream so the server knows which ELF file to use for decoding.

## 2. Server-Side: "Bridge" Service

Build a Rust service that acts as an intermediary between your devices and the OpenTelemetry Collector.

- [ ] **Symbol Management**:
    - Create a storage system (S3, local disk) to hold firmware ELF files, keyed by version ID.
    - Implement a lookup mechanism to fetch the correct `defmt::Table` for a connected device.
- [ ] **Decoder Integration**:
    - Use the `defmt-decoder` crate to decode raw binary frames received from the device into log strings.
    - Handle decoding errors gracefully (e.g., dropped packets).
- [ ] **Trace Reconstruction**:
    - Adapt the logic from `examples/host_trace_reconstructor.rs` into a library function.
    - Process the decoded log strings to reconstruct the trace hierarchy (`span_enter` -> `span_exit`).
    - Use `opentelemetry` SDK to generate and export spans.
- [ ] **OTLP Export**:
    - Configure the service to export traces via OTLP (gRPC/HTTP) to your OpenTelemetry Collector.

## 3. DevOps / CI Pipeline

- [ ] **Automated Symbol Upload**:
    - Update your CI/CD pipeline to automatically upload the ELF file (with debug symbols) to your symbol storage whenever a release build is created.
    - Ensure the version ID in the firmware matches the key used for storage.

## Resources

- [defmt-decoder crate](https://crates.io/crates/defmt-decoder)
- [OpenTelemetry OTLP Exporter](https://crates.io/crates/opentelemetry-otlp)