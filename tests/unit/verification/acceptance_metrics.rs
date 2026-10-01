use super::*;

fn record(at: u64, state: &str) -> Value {
    json!({"schema_version":1,"producer":"wcode/native-acceptance/v1","captured_at_ms":at,"state":state,"revision":{"code":"current"},"git":{"base_sha":"base","target_sha":"head"},"policy":{"generation":1},"checks":[],"actions":[]})
}
fn history(records: Vec<Value>) -> Value {
    json!({"authority":"historical_only","current_acceptance":false,"capacity":256,"records":records})
}
fn timed_record(at: u64, id: &str, duration: u64) -> Value {
    let mut value = record(at, "blocked");
    value["workspace"] = json!("workspace");
    value["policy"] = json!({"generation":1,"snapshot_digest":"snapshot"});
    value["git"]["binding"] = json!({"repository":"repository"});
    value["checks"] = json!([{"id":"check","signature":"signature","evidence_ids":[id]}]);
    value["evidence"] = json!([{"id":id,"authority":"native_verification","confidence":"deterministic",
        "verification_relation":"native_check","freshness":"current","revision":value["revision"],
        "execution_policy_binding":"project-policy/v1/1/snapshot","timestamp_ms":10,
        "execution_timing":{"source":"native_check_execution/v1","check_id":"check",
            "signature":"signature","level":"quick","phase":0,"execution_ms":duration}}]);
    value
}

#[test]
fn pilot_metrics_execution_timings_deduplicate_native_observations_and_keep_zero_samples() {
    let first = timed_record(100, "EV-time", 0);
    let mut repeat = first.clone();
    repeat["captured_at_ms"] = json!(150);
    repeat["state"] = json!("ready");
    let second = timed_record(200, "EV-second", 80);
    let result = summarize(&history(vec![first, repeat, second])).unwrap();
    let timing = &result["verification_duration"];
    assert_eq!(timing["available"], true);
    assert_eq!(timing["sample_count"], 2);
    assert_eq!(timing["samples_ms"], json!([0, 80]));
    assert_eq!(timing["p50_ms"], 0);
    assert_eq!(timing["p95_ms"], 80);
    assert_eq!(timing["historical_only"], true);
    assert_eq!(
        timing["measurement"],
        "per_check_execution_ms_not_run_wall_time"
    );
    for private in ["EV-time", "workspace", "signature", "repository"] {
        assert!(!timing.to_string().contains(private));
    }
}

#[test]
fn pilot_metrics_execution_timings_reject_conflicting_identity_and_untrusted_or_stale_samples() {
    let first = timed_record(100, "EV-conflict", 10);
    let second = timed_record(200, "EV-conflict", 20);
    let result = summarize(&history(vec![first, second])).unwrap();
    assert_eq!(result["verification_duration"]["available"], false);
    assert_eq!(result["verification_duration"]["conflicting_evidence"], 1);
    for variant in [
        "stale",
        "authority",
        "revision",
        "policy",
        "timestamp",
        "signature",
        "negative",
        "overflow",
        "source",
    ] {
        let mut value = timed_record(100, "EV-invalid", 10);
        let item = &mut value["evidence"][0];
        match variant {
            "stale" => item["freshness"] = json!("stale"),
            "authority" => item["authority"] = json!("self_reported"),
            "revision" => item["revision"] = json!({"code":"other"}),
            "policy" => item["execution_policy_binding"] = json!("project-policy/v1/2/snapshot"),
            "timestamp" => item["timestamp_ms"] = json!(101),
            "signature" => item["execution_timing"]["signature"] = json!("wrong"),
            "negative" => item["execution_timing"]["execution_ms"] = json!(-1),
            "overflow" => item["execution_timing"]["execution_ms"] = json!("18446744073709551616"),
            _ => item["execution_timing"]["source"] = json!("worker_claim"),
        }
        let result = summarize(&history(vec![value])).unwrap();
        assert_eq!(
            result["verification_duration"]["available"], false,
            "{variant}"
        );
        assert_eq!(
            result["verification_duration"]["sample_count"], 0,
            "{variant}"
        );
    }
    let legacy = summarize(&history(vec![record(100, "ready")])).unwrap();
    assert_eq!(legacy["verification_duration"]["available"], false);
    assert!(legacy["verification_duration"]["p50_ms"].is_null());
}

#[test]
fn pilot_metrics_count_observations_without_claiming_causal_savings() {
    let mut blocked = record(100, "blocked");
    blocked["checks"] = json!([{"required":true,"execution":"skipped"}]);
    blocked["summary"] = json!({"stale":1});
    blocked["actions"] = json!(["request_human_approval"]);
    let ready = record(150, "ready");
    let result = summarize(&history(vec![ready, blocked])).unwrap();
    assert_eq!(result["retained_records"], 2);
    assert_eq!(result["distinct_revision_policy_bindings"], 1);
    assert_eq!(result["observed_not_ready_to_ready_ms"], json!([50]));
    assert_eq!(result["records_with_missing_required_execution"], 1);
    assert_eq!(result["records_with_stale_evidence"], 1);
    assert_eq!(result["records_requesting_human_review"], 1);
    assert_eq!(result["uploads"], false);
    assert_eq!(result["exceptions"]["available"], false);
    assert!(result.get("saved_hours").is_none());
    assert!(result.get("saved_tokens").is_none());
}
#[test]
fn pilot_metrics_do_not_invent_lead_time_from_ready_only_or_a_different_revision() {
    assert_eq!(
        summarize(&history(vec![record(100, "ready")])).unwrap()["observed_not_ready_to_ready_ms"],
        json!([])
    );
    let mut ready = record(150, "ready");
    ready["policy"]["generation"] = json!(2);
    assert_eq!(
        summarize(&history(vec![record(100, "blocked"), ready])).unwrap()
            ["observed_not_ready_to_ready_ms"],
        json!([])
    );
}
#[test]
fn pilot_metrics_do_not_link_cross_workspace_or_worktree_commit_transitions() {
    for key in ["workspace", "git_binding", "policy_content"] {
        let mut before = record(100, "blocked");
        let mut after = record(150, "ready");
        match key {
            "workspace" => {
                before["workspace"] = json!("first");
                after["workspace"] = json!("second");
            }
            "git_binding" => {
                before["git"]["target_sha"] = Value::Null;
                after["git"]["target_sha"] = Value::Null;
                before["git"]["binding"] = json!({"head_sha":"first"});
                after["git"]["binding"] = json!({"head_sha":"second"});
            }
            _ => {
                before["policy"]["snapshot_fingerprint"] = json!("first");
                after["policy"]["snapshot_fingerprint"] = json!("second");
            }
        }
        let result = summarize(&history(vec![before, after])).unwrap();
        assert_eq!(result["distinct_revision_policy_bindings"], 2, "{key}");
        assert_eq!(result["observed_not_ready_to_ready_ms"], json!([]), "{key}");
    }
}
#[test]
fn pilot_metrics_reject_current_authority_bad_records_and_unbounded_data() {
    let mut current = history(vec![]);
    current["current_acceptance"] = json!(true);
    assert!(summarize(&current).is_err());
    assert!(summarize(&history(vec![record(0, "ready")])).is_err());
    assert!(summarize(&history(vec![record(1, "paid_ready")])).is_err());
    assert!(summarize(&history(vec![record(1, "ready"); 257])).is_err());
    let result = summarize(&history(vec![])).unwrap();
    assert_eq!(result["retained_records"], 0);
    assert!(result["window"]["first_capture_ms"].is_null());
}
