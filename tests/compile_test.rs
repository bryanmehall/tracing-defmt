use tracing_defmt as tracing;

#[test]
fn test_macros_compile() {
    tracing::trace!("trace log");
    tracing::debug!("debug log");
    tracing::info!("info log");
    tracing::warn!("warn log");
    tracing::error!("error log");
}

#[test]
fn test_args() {
    let x = 42;
    tracing::info!("value: {}", x);
    // Key-value pairs (trailing)
    tracing::info!("value", x = x);
    // Key-value pairs (mixed/leading - supported by our macro parser)
    tracing::info!(y = x, "value");
}

#[tracing::instrument]
fn instrumented_fn(x: u32) {
    tracing::info!("inside instrumented function");
}

#[test]
fn test_instrument() {
    instrumented_fn(123);
}

#[test]
fn test_spans() {
    // Spans are currently dummy implementations, but should compile
    let span = tracing::info_span!("my_span");
    let _enter = span.enter();
    tracing::info!("in span");
}

#[test]
fn test_fields_wrappers() {
    struct NoDefmt;

    impl core::fmt::Display for NoDefmt {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "NoDefmt(Display)")
        }
    }

    impl core::fmt::Debug for NoDefmt {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "NoDefmt(Debug)")
        }
    }

    let n = NoDefmt;
    // Test field::display wrapper
    tracing::info!(val = tracing::field::display(&n), "testing display wrapper");
    // Test field::debug wrapper
    tracing::info!(val = tracing::field::debug(&n), "testing debug wrapper");
}

// Stubs to satisfy the linker when running tests on host
#[unsafe(no_mangle)]
fn _defmt_acquire() {}

#[unsafe(no_mangle)]
fn _defmt_release() {}

#[unsafe(no_mangle)]
fn _defmt_write(_bytes: &[u8]) {}

#[unsafe(no_mangle)]
fn _defmt_timestamp(_fmt: tracing::defmt::Formatter<'_>) {}
