use std::collections::HashMap;

// A mock of the NEW stateless, OpenTelemetry-compatible host decoder.
// It translates parsed defmt strings into reconstructed hierarchical events.
fn process_logs(logs: &[String]) -> Vec<(String, String)> {
    // Maps a Span ID (hex string) to its active stack/name
    let mut active_spans: HashMap<String, String> = HashMap::new();
    let mut output = Vec::new();

    for line in logs {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with("ctx=") {
            let space_idx = line.find(' ').unwrap();
            let ctx_str = &line[4..space_idx];
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
                }
            } else {
                // It's a standard log message
                if let Some(span_name) = active_spans.get(span_id) {
                    output.push((span_name.clone(), payload.to_string()));
                } else {
                    output.push(("UNKNOWN".to_string(), payload.to_string()));
                }
            }
        }
    }
    output
}

#[test]
fn test_nested_span_reconstruction() {
    let logs = vec![
        "ctx=TID:SID_1 parent=PID_0 span_enter: root_task".to_string(),
        "ctx=TID:SID_1 root task started".to_string(),
        // Nested async child span
        "ctx=TID:SID_2 parent=SID_1 span_enter: nested_async".to_string(),
        "ctx=TID:SID_2 deep log message".to_string(),
        "ctx=TID:SID_2 parent=SID_1 span_exit: nested_async".to_string(),
        // Manual span inside root task
        "ctx=TID:SID_3 parent=SID_1 span_enter: manual_span".to_string(),
        "ctx=TID:SID_3 manual span message".to_string(),
        "ctx=TID:SID_3 parent=SID_1 span_exit: manual_span".to_string(),
        "ctx=TID:SID_1 parent=PID_0 span_exit: root_task".to_string(),
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
        "ctx=TID_A:SID_A parent=PID_0 span_enter: task_a".to_string(),
        "ctx=TID_B:SID_B parent=PID_0 span_enter: task_b".to_string(),
        // Logs are jumbled chronologically!
        "ctx=TID_B:SID_B hello from task b".to_string(),
        "ctx=TID_A:SID_A hello from task a".to_string(),
        "ctx=TID_B:SID_B parent=PID_0 span_exit: task_b".to_string(),
        "ctx=TID_A:SID_A parent=PID_0 span_exit: task_a".to_string(),
    ];

    let output = process_logs(&logs);

    // The stateless decoder perfectly attributes the jumbled logs
    // based purely on `ctx` extraction and mapping.
    assert_eq!(
        output,
        vec![
            ("task_b".to_string(), "hello from task b".to_string()),
            ("task_a".to_string(), "hello from task a".to_string()),
        ]
    );
}
