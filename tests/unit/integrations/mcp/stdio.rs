use super::*;

#[test]
fn protocol_selection_supports_modern_stdio_and_legacy_sessions() {
    let modern = json!({
        "jsonrpc":"2.0",
        "id":1,
        "method":"tools/list",
        "params":{"_meta":{
            "io.modelcontextprotocol/protocolVersion": MODERN_PROTOCOL_VERSION,
            "io.modelcontextprotocol/clientCapabilities": {}
        }}
    });
    assert_eq!(
        protocol_for_message(&modern, DEFAULT_LEGACY_PROTOCOL),
        MODERN_PROTOCOL_VERSION
    );
    let discover = json!({"jsonrpc":"2.0","id":2,"method":"server/discover"});
    assert_eq!(
        protocol_for_message(&discover, DEFAULT_LEGACY_PROTOCOL),
        MODERN_PROTOCOL_VERSION
    );
    let legacy = json!({"jsonrpc":"2.0","id":3,"method":"tools/list","params":{}});
    assert_eq!(protocol_for_message(&legacy, "2025-06-18"), "2025-06-18");
}
