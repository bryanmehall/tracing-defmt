use std::collections::HashMap;

// A mock of the NEW stateless, OpenTelemetry-compatible host decoder.
// It translates parsed defmt strings into reconstructed hierarchical events.
fn process_logs(logs: &[String]) -> Vec<(String, String)> {
    // Maps a Span ID (array string) to its active stack/name
    let mut active_spans: HashMap<String, String> = HashMap::new();
    let mut output = Vec::new();

    for line in logs {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with("ctx=") {
            let anchor = line.find("]:[").unwrap();
            let end_idx = line[anchor..].find("] ").unwrap() + anchor + 1;
            let ctx_str = &line[4..end_idx];

            // Expected format: [0, 1, ...]:[0, 1, ...]
            let mut split = ctx_str.split("]:[");
            let _trace_id = format!("{}]", split.next().unwrap());
            let span_id = format!("[{}", split.next().unwrap());
            let payload = &line[end_idx + 1..];

            if payload.starts_with("parent=") {
                let event_str = if let Some(idx) = payload.find(" span_enter: ") {
                    &payload[idx + 1..]
                } else if let Some(idx) = payload.find(" span_exit: ") {
                    &payload[idx + 1..]
                } else {
                    ""
                };

                if event_str.starts_with("span_enter: ") {
                    let name = &event_str["span_enter: ".len()..];
                    active_spans.insert(span_id.clone(), name.to_string());
                } else if event_str.starts_with("span_exit: ") {
                    active_spans.remove(&span_id);
                } else {
                    // Fallback just in case
                    if let Some(span_name) = active_spans.get(&span_id) {
                        output.push((span_name.clone(), payload.to_string()));
                    }
                }
            } else {
                // It's a standard log message
                if let Some(span_name) = active_spans.get(&span_id) {
                    output.push((span_name.clone(), payload.to_string()));
                } else {
                    // Stateless Recovery Span logic
                    if ctx_str.len() > 10 { // Simplified mock check for array string presence
                        active_spans.insert(span_id.clone(), "recovery_span".to_string());
                        output.push(("recovery_span".to_string(), payload.to_string()));
                    } else {
                        output.push(("UNKNOWN".to_string(), payload.to_string()));
                    }
                }
            }
        }
    }
    output
}

#[test]
fn test_stateless_recovery_span() {
    let logs = vec![
        "ctx=[0, 0, 0, 12]:[0, 0, 0, 12] an orphaned log from missing span".to_string(),
        "ctx=[0, 0, 0, 12]:[0, 0, 0, 12] another log in the recovered span".to_string(),
    ];

    let output = process_logs(&logs);

    assert_eq!(
        output,
        vec![
            ("recovery_span".to_string(), "an orphaned log from missing span".to_string()),
            ("recovery_span".to_string(), "another log in the recovered span".to_string()),
        ]
    );
}

#[test]
fn test_nested_span_reconstruction() {
    let logs = vec![
        "ctx=[0, 1]:[0, 1] parent=[0, 0] span_enter: root_task".to_string(),
        "ctx=[0, 1]:[0, 1] root task started".to_string(),
        // Nested async child span
        "ctx=[0, 1]:[0, 2] parent=[0, 1] span_enter: nested_async".to_string(),
        "ctx=[0, 1]:[0, 2] deep log message".to_string(),
        "ctx=[0, 1]:[0, 2] parent=[0, 1] span_exit: nested_async".to_string(),
        // Manual span inside root task
        "ctx=[0, 1]:[0, 3] parent=[0, 1] span_enter: manual_span".to_string(),
        "ctx=[0, 1]:[0, 3] manual span message".to_string(),
        "ctx=[0, 1]:[0, 3] parent=[0, 1] span_exit: manual_span".to_string(),
        "ctx=[0, 1]:[0, 1] parent=[0, 0] span_exit: root_task".to_string(),
    ];

    let output = process_logs(&logs);

    assert_eq!(
        output,
        vec![
            ("root_task".to_string(), "root task started".to_string()),
            ("nested_async".to_string(), "deep log message".to_string()),
            ("manual_span".to_string(), "manual span message".to_string()),
        ]
    );
}

#[test]
fn test_interleaved_concurrency_reconstruction() {
    let logs = vec![
        "ctx=[0, 10]:[0, 10] parent=[0, 0] span_enter: task_a".to_string(),
        "ctx=[0, 11]:[0, 11] parent=[0, 0] span_enter: task_b".to_string(),
        // Logs are jumbled chronologically!
        "ctx=[0, 11]:[0, 11] hello from task b".to_string(),
        "ctx=[0, 10]:[0, 10] hello from task a".to_string(),
        "ctx=[0, 11]:[0, 11] parent=[0, 0] span_exit: task_b".to_string(),
        "ctx=[0, 10]:[0, 10] parent=[0, 0] span_exit: task_a".to_string(),
    ];

    let output = process_logs(&logs);

    assert_eq!(
        output,
        vec![
            ("task_b".to_string(), "hello from task b".to_string()),
            ("task_a".to_string(), "hello from task a".to_string()),
        ]
    );
}
