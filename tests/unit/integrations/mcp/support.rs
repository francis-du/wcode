use super::*;

pub(super) fn modern_headers(method: &str, name: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "mcp-protocol-version",
        MODERN_PROTOCOL_VERSION.parse().unwrap(),
    );
    headers.insert("mcp-method", method.parse().unwrap());
    if let Some(name) = name {
        headers.insert("mcp-name", name.parse().unwrap());
    }
    headers
}

pub(super) fn modern_request(method: &str, params: Value) -> Value {
    let mut request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": {
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": MODERN_PROTOCOL_VERSION,
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }
    });
    if let Some(object) = params.as_object() {
        for (key, value) in object {
            request["params"][key] = value.clone();
        }
    }
    request
}

pub(super) fn task_capable(mut request: Value) -> Value {
    request["params"]["_meta"]["io.modelcontextprotocol/clientCapabilities"] = json!({
        "extensions": {(TASK_EXTENSION_ID): {}}
    });
    request
}
