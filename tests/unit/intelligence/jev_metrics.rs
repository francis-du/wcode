use super::*;

#[test]
fn provider_token_source_requires_a_complete_provider_triplet() {
    assert_eq!(
        provider_token_source(Some(80), Some(20), Some(100)),
        "provider_reported"
    );
    assert_eq!(
        provider_token_source(Some(80), None, None),
        "partial_provider_reported"
    );
    assert_eq!(
        provider_token_source(None, Some(20), None),
        "partial_provider_reported"
    );
    assert_eq!(provider_token_source(None, None, None), "unavailable");
}

#[test]
fn provider_usage_accepts_modern_and_compat_token_names() {
    assert_eq!(
        provider_usage(&json!({
            "usage":{"input_tokens":120,"output_tokens":30,"total_tokens":150}
        })),
        ProviderUsage {
            input_tokens: Some(120),
            output_tokens: Some(30),
            total_tokens: Some(150),
        }
    );
    assert_eq!(
        provider_usage(&json!({
            "meta":{"usage":{"prompt_tokens":80,"completion_tokens":20,"total_tokens":100}}
        })),
        ProviderUsage {
            input_tokens: Some(80),
            output_tokens: Some(20),
            total_tokens: Some(100),
        }
    );
    assert_eq!(provider_usage(&json!({})), ProviderUsage::default());
}

#[test]
fn provider_usage_derives_missing_total_only_from_complete_provider_components() {
    let complete = provider_usage(&json!({
        "usage":{"input_tokens":120,"output_tokens":30}
    }));
    assert_eq!(complete.total_tokens, Some(150));

    let partial = provider_usage(&json!({
        "usage":{"input_tokens":120}
    }));
    assert_eq!(partial.total_tokens, None);
}

#[test]
fn provider_usage_ignores_empty_outer_usage_when_nested_usage_has_tokens() {
    let usage = provider_usage(&json!({
        "usage":{},
        "meta":{"usage":{"prompt_tokens":80,"completion_tokens":20}}
    }));
    assert_eq!(usage.input_tokens, Some(80));
    assert_eq!(usage.output_tokens, Some(20));
    assert_eq!(usage.total_tokens, Some(100));
}
