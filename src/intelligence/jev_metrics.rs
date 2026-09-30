use super::*;

#[cfg(not(test))]
#[derive(Clone, Debug, Default)]
pub(crate) struct JevCallMetrics {
    pub(crate) request_bytes: u64,
    pub(crate) response_bytes: u64,
    pub(crate) elapsed_ms: u64,
    pub(crate) input_tokens: Option<u64>,
    pub(crate) output_tokens: Option<u64>,
    pub(crate) total_tokens: Option<u64>,
}

#[cfg(not(test))]
impl JevCallMetrics {
    pub(crate) fn as_json(&self) -> Value {
        json!({
            "request_bytes": self.request_bytes,
            "response_bytes": self.response_bytes,
            "elapsed_ms": self.elapsed_ms,
            "tokens": {
                "input": self.input_tokens,
                "output": self.output_tokens,
                "total": self.total_tokens,
                "source": provider_token_source(
                    self.input_tokens,
                    self.output_tokens,
                    self.total_tokens,
                )
            }
        })
    }
}

#[cfg(not(test))]
pub(crate) struct JevEvaluation {
    pub(crate) batch: DecisionBatch,
    pub(crate) metrics: JevCallMetrics,
}

fn provider_token_source(
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    total_tokens: Option<u64>,
) -> &'static str {
    match (
        input_tokens.is_some(),
        output_tokens.is_some(),
        total_tokens.is_some(),
    ) {
        (true, true, true) => "provider_reported",
        (false, false, false) => "unavailable",
        _ => "partial_provider_reported",
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct ProviderUsage {
    pub(super) input_tokens: Option<u64>,
    pub(super) output_tokens: Option<u64>,
    pub(super) total_tokens: Option<u64>,
}

pub(super) fn provider_usage(value: &Value) -> ProviderUsage {
    let usage = [
        value.get("usage"),
        value.pointer("/meta/usage"),
        value.pointer("/metadata/usage"),
    ]
    .into_iter()
    .flatten()
    .find(|usage| {
        [
            "input_tokens",
            "prompt_tokens",
            "output_tokens",
            "completion_tokens",
            "total_tokens",
        ]
        .iter()
        .any(|name| usage.get(*name).and_then(Value::as_u64).is_some())
    });
    let Some(usage) = usage else {
        return ProviderUsage::default();
    };
    let token = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| usage.get(*name).and_then(Value::as_u64))
    };
    let mut parsed = ProviderUsage {
        input_tokens: token(&["input_tokens", "prompt_tokens"]),
        output_tokens: token(&["output_tokens", "completion_tokens"]),
        total_tokens: token(&["total_tokens"]),
    };
    if parsed.total_tokens.is_none() {
        parsed.total_tokens = parsed
            .input_tokens
            .zip(parsed.output_tokens)
            .and_then(|(input, output)| input.checked_add(output));
    }
    parsed
}

#[cfg(test)]
#[path = "../../tests/unit/intelligence/jev_metrics.rs"]
mod tests;
