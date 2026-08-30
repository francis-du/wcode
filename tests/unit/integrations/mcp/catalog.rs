use super::*;

#[test]
fn catalog_is_bounded_and_has_no_secret_or_hook_payloads() {
    let prompt = prompt_get("wcode-implement", Some(&json!({"goal":"x".repeat(2_000)}))).unwrap();
    let text = prompt["messages"][0]["content"]["text"].as_str().unwrap();
    assert!(text.len() < 5_000);
    assert!(!SECURITY_RESOURCE.contains("API_KEY="));
    assert_eq!(resources_list()["resources"].as_array().unwrap().len(), 3);
    let scopes = resource_read("wcode://runtime/product-scopes").unwrap();
    let scopes_text = scopes["contents"][0]["text"].as_str().unwrap();
    assert!(scopes_text.contains("Software Graph (`graph`)"));
    assert!(scopes_text.contains("software_context.scopes"));
    assert_eq!(prompts_list()["prompts"].as_array().unwrap().len(), 3);
}
