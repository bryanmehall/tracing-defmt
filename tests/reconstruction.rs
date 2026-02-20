use tracing::Level;
use tracing_fluent_assertions::{AssertionRegistry, AssertionsLayer};
use tracing_subscriber::{Registry, layer::SubscriberExt};

// Re-implementation of the logic from examples/host_trace_reconstructor.rs
// adjusted to read from a slice of strings.
fn process_logs(logs: &[String]) {
    let mut iter = logs.iter();
    process_scope(&mut iter);
}

fn process_scope<'a, I>(lines: &mut I)
where
    I: Iterator<Item = &'a String>,
{
    while let Some(line) = lines.next() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(msg_start) = line.find("span_enter: ") {
            let content = &line[msg_start + "span_enter: ".len()..];
            let (name, args) = if let Some(idx) = content.find('(') {
                let end = content.len().saturating_sub(1);
                (&content[..idx], &content[idx + 1..end])
            } else {
                (content, "")
            };

            // Use match to provide static names for expected spans
            let span = match name {
                "my_function" => tracing::span!(Level::INFO, "my_function", args = args),
                "nested_call" => tracing::span!(Level::INFO, "nested_call", args = args),
                _ => tracing::span!(Level::INFO, "unknown", function = name, args = args),
            };

            let _guard = span.enter();
            process_scope(lines);
        } else if let Some(msg_start) = line.find("span_exit: ") {
            let _name = &line[msg_start + "span_exit: ".len()..];
            return;
        } else {
            tracing::info!(target: "device_log", "{}", line);
        }
    }
}

#[test]
fn test_nested_span_reconstruction() {
    let assertion_registry = AssertionRegistry::default();
    let layer = AssertionsLayer::new(&assertion_registry);
    let subscriber = Registry::default().with(layer);

    // Build assertions BEFORE execution
    let my_fn_assertion = assertion_registry
        .build()
        .with_name("my_function")
        .was_entered()
        .was_closed()
        .finalize();

    let nested_call_assertion = assertion_registry
        .build()
        .with_name("nested_call")
        .was_entered()
        .was_closed()
        .finalize();

    let logs = vec![
        "span_enter: my_function(x=10, y=20)".to_string(),
        "Entered my_function with x=10, y=20".to_string(),
        "span_enter: nested_call(value=30)".to_string(),
        "Inside nested_call with value=30".to_string(),
        "Very verbose info from nested call".to_string(),
        "span_exit: nested_call".to_string(),
        "This is a warning inside the function".to_string(),
        "span_exit: my_function".to_string(),
    ];

    tracing::subscriber::with_default(subscriber, || {
        process_logs(&logs);
    });

    // Assertions
    my_fn_assertion.assert();
    nested_call_assertion.assert();
}
