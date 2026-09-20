use super::*;

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
