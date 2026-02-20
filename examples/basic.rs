use tracing_defmt as tracing;

#[tracing::instrument]
fn nested_call(value: u8) {
    tracing::info!("Inside nested_call with value={}", value);
    tracing::trace!("Very verbose info from nested call");
}

#[tracing::instrument]
fn my_function(x: u8, y: u8) {
    tracing::info!("Entered my_function with x={}, y={}", x, y);
    nested_call(x + y);
    tracing::warn!("This is a warning inside the function");
}

fn main() {
    // This example mimics how you would use the crate in an embedded project.
    // Note: When running on host, defmt logs are encoded and won't appear on stdout
    // without a decoder (like probe-run or defmt-print).

    let x = 10;
    let y = 20;

    tracing::info!("Starting application...");
    my_function(x, y);
    tracing::error!("An error occurred (simulated)");
}

// Stubs to satisfy the linker when running examples on host
#[unsafe(no_mangle)]
fn _defmt_acquire() {}

#[unsafe(no_mangle)]
fn _defmt_release() {}

#[unsafe(no_mangle)]
fn _defmt_write(_bytes: &[u8]) {}

#[unsafe(no_mangle)]
fn _defmt_timestamp(_fmt: tracing::defmt::Formatter<'_>) {}
