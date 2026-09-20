use super::*;

fn diagnostic(message: String) -> Value {
    json!({
        "range": {
            "start": {"line": 0, "character": 0},
            "end": {"line": 0, "character": 1}
        },
        "severity": 1,
        "code": "mock",
        "source": "wcode-test",
        "message": message,
        "data": {"fix": "bounded"}
    })
}

#[test]
fn published_diagnostics_are_bounded_and_mark_truncation() {
    let raw = (0..=MAX_LSP_DIAGNOSTICS_PER_DOCUMENT)
        .map(|index| diagnostic(format!("diagnostic-{index}")))
        .collect::<Vec<_>>();
    let published = compact_published_diagnostics(Some(7), &raw);
    assert_eq!(published.version, Some(7));
    assert_eq!(
        published.diagnostics.len(),
        MAX_LSP_DIAGNOSTICS_PER_DOCUMENT
    );
    assert!(published.truncated);
}

#[test]
fn published_diagnostics_bound_message_data_and_total_bytes() {
    let mut oversized = diagnostic("x".repeat(MAX_LSP_DIAGNOSTIC_MESSAGE_CHARS + 500));
    oversized["data"] = json!({"blob": "y".repeat(MAX_LSP_DIAGNOSTIC_DATA_BYTES + 500)});
    let published = compact_published_diagnostics(Some(3), &[oversized]);
    assert_eq!(published.version, Some(3));
    assert!(!published.truncated);
    assert_eq!(
        published.diagnostics[0]["message"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        MAX_LSP_DIAGNOSTIC_MESSAGE_CHARS
    );
    assert!(published.diagnostics[0].get("data").is_none());

    let many_large = (0..MAX_LSP_DIAGNOSTICS_PER_DOCUMENT)
        .map(|_| diagnostic("z".repeat(MAX_LSP_DIAGNOSTIC_MESSAGE_CHARS)))
        .collect::<Vec<_>>();
    let published = compact_published_diagnostics(Some(4), &many_large);
    assert!(published.truncated);
    assert!(published.diagnostics.len() < MAX_LSP_DIAGNOSTICS_PER_DOCUMENT);
}
