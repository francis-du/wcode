use super::*;

pub(super) fn jev_runtime_json(stats: &WorkspaceStats, now: Instant) -> Option<Value> {
    stats.jev_latest.as_ref().map(|jev| {
        let avg_elapsed_ms =
            (stats.jev_call_samples > 0).then(|| stats.jev_elapsed_ms / stats.jev_call_samples);

        let latest_provider_tokens = jev.call_input_tokens.is_some()
            || jev.call_output_tokens.is_some()
            || jev.call_total_tokens.is_some();
        let latest_input_tokens = jev
            .call_input_tokens
            .unwrap_or_else(|| jev.call_request_bytes.div_ceil(4));
        let latest_output_tokens = jev
            .call_output_tokens
            .unwrap_or_else(|| jev.call_response_bytes.div_ceil(4));
        let latest_total_tokens = jev
            .call_total_tokens
            .unwrap_or_else(|| latest_input_tokens.saturating_add(latest_output_tokens));
        let latest_token_source = if latest_provider_tokens {
            "provider_reported"
        } else if jev.call_request_bytes > 0 || jev.call_response_bytes > 0 {
            "byte_estimate"
        } else {
            "unavailable"
        };

        let aggregate_provider_complete =
            stats.jev_call_samples > 0 && stats.jev_token_observations == stats.jev_call_samples;
        let aggregate_input_tokens = if aggregate_provider_complete {
            stats.jev_input_tokens
        } else {
            stats.jev_request_bytes.div_ceil(4)
        };
        let aggregate_output_tokens = if aggregate_provider_complete {
            stats.jev_output_tokens
        } else {
            stats.jev_response_bytes.div_ceil(4)
        };
        let aggregate_total_tokens = if aggregate_provider_complete {
            stats.jev_total_tokens
        } else {
            aggregate_input_tokens.saturating_add(aggregate_output_tokens)
        };
        let aggregate_token_source = if stats.jev_call_samples == 0 {
            "unavailable"
        } else if aggregate_provider_complete {
            "provider_reported"
        } else if stats.jev_token_observations > 0 {
            "mixed_provider_and_byte_estimate"
        } else {
            "byte_estimate"
        };

        serde_json::json!({
            "provider": "jev",
            "checkpoint": jev.checkpoint,
            "status": jev.status,
            "model": jev.model,
            "authority": jev.authority,
            "question_set": {
                "id": jev.question_set_id,
                "version": jev.question_set_version,
            },
            "baseline_next_action": jev.baseline_next_action,
            "candidate_next_action": jev.candidate_next_action,
            "guidance": jev.guidance,
            "comparison": {
                "shared_signals": jev.shared_signals,
                "choice_disagreements": jev.choice_disagreements,
                "safety_policy_violations": jev.safety_policy_violations,
                "shape_mismatches": jev.shape_mismatches,
            },
            "call": {
                "request_bytes": jev.call_request_bytes,
                "response_bytes": jev.call_response_bytes,
                "elapsed_ms": jev.call_elapsed_ms,
                "tokens": {
                    "input": latest_input_tokens,
                    "output": latest_output_tokens,
                    "total": latest_total_tokens,
                    "source": latest_token_source,
                }
            },
            "calls": {
                "observed": stats.jev_observed,
                "successful": stats.jev_successful,
                "degraded": stats.jev_degraded,
                "disabled": stats.jev_disabled,
                "by_checkpoint": &stats.jev_checkpoints,
                "metered": stats.jev_call_samples,
                "request_bytes": stats.jev_request_bytes,
                "response_bytes": stats.jev_response_bytes,
                "elapsed_ms": stats.jev_elapsed_ms,
                "avg_elapsed_ms": avg_elapsed_ms,
                "tokens": {
                    "observations": stats.jev_token_observations,
                    "input": aggregate_input_tokens,
                    "output": aggregate_output_tokens,
                    "total": aggregate_total_tokens,
                    "source": aggregate_token_source,
                }
            },
            "observed_ago_ms": now.saturating_duration_since(jev.observed_at).as_millis(),
        })
    })
}
