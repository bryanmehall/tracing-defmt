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
            let space_idx = line.find(" parent=").or_else(|| line.find(' ')).unwrap();
            let ctx_str = &line[4..space_idx];

            // Expected format: 0000000000000000000000000000000C:000000000000000C
            let mut split = ctx_str.split(':');
            let _trace_id = split.next().unwrap();
            let span_id = split.next().unwrap();
            let payload = &line[space_idx + 1..];

            if payload.starts_with("parent=") {
                let parent_space_idx = payload.find(' ').unwrap();
                let _parent_id = &payload[7..parent_space_idx];
                let event_str = &payload[parent_space_idx + 1..];

                if event_str.starts_with("span_enter: ") {
                    let name = &event_str["span_enter: ".len()..];
                    active_spans.insert(span_id.to_string(), name.to_string());
                } else if event_str.starts_with("span_exit: ") {
                    active_spans.remove(span_id);
                } else {
                    // Fallback
                    if let Some(span_name) = active_spans.get(span_id) {
                        output.push((span_name.clone(), payload.to_string()));
                    }
                }
            } else {
                // It's a standard log message
                if let Some(span_name) = active_spans.get(span_id) {
                    output.push((span_name.clone(), payload.to_string()));
                } else {
                    // Stateless Recovery Span logic
                    if ctx_str.len() == 32 + 1 + 16 {
                        active_spans.insert(span_id.to_string(), "recovery_span".to_string());
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
        "ctx=0000000000000000000000000000000c:000000000000000c an orphaned log from missing span"
            .to_string(),
        "ctx=0000000000000000000000000000000c:000000000000000c another log in the recovered span"
            .to_string(),
    ];

    let output = process_logs(&logs);

    assert_eq!(
        output,
        vec![
            (
                "recovery_span".to_string(),
                "an orphaned log from missing span".to_string()
            ),
            (
                "recovery_span".to_string(),
                "another log in the recovered span".to_string()
            ),
        ]
    );
}

#[test]
fn test_nested_span_reconstruction() {
    let logs = vec![
        "ctx=00000000000000000000000000000001:0000000000000001 parent=0000000000000000 span_enter: root_task".to_string(),
        "ctx=00000000000000000000000000000001:0000000000000001 root task started".to_string(),
        // Nested async child span
        "ctx=00000000000000000000000000000001:0000000000000002 parent=0000000000000001 span_enter: nested_async".to_string(),
        "ctx=00000000000000000000000000000001:0000000000000002 deep log message".to_string(),
        "ctx=00000000000000000000000000000001:0000000000000002 parent=0000000000000001 span_exit: nested_async".to_string(),
        // Manual span inside root task
        "ctx=00000000000000000000000000000001:0000000000000003 parent=0000000000000001 span_enter: manual_span".to_string(),
        "ctx=00000000000000000000000000000001:0000000000000003 manual span message".to_string(),
        "ctx=00000000000000000000000000000001:0000000000000003 parent=0000000000000001 span_exit: manual_span".to_string(),
        "ctx=00000000000000000000000000000001:0000000000000001 parent=0000000000000000 span_exit: root_task".to_string(),
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
        "ctx=0000000000000000000000000000000A:000000000000000A parent=0000000000000000 span_enter: task_a".to_string(),
        "ctx=0000000000000000000000000000000B:000000000000000B parent=0000000000000000 span_enter: task_b".to_string(),
        // Logs are jumbled chronologically!
        "ctx=0000000000000000000000000000000B:000000000000000B hello from task b".to_string(),
        "ctx=0000000000000000000000000000000A:000000000000000A hello from task a".to_string(),
        "ctx=0000000000000000000000000000000B:000000000000000B parent=0000000000000000 span_exit: task_b".to_string(),
        "ctx=0000000000000000000000000000000A:000000000000000A parent=0000000000000000 span_exit: task_a".to_string(),
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
