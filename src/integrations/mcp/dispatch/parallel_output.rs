use super::*;

pub(crate) fn parallel_item_from_response(
    id: String,
    tool: String,
    mut response: Value,
) -> (Value, usize) {
    let is_error = response
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let result = response
        .get_mut("structuredContent")
        .map(std::mem::take)
        .unwrap_or(Value::Null);
    let fixed_bytes = serialized_size(&json!({
        "id": &id,
        "tool": &tool,
        "ok": !is_error,
        "result": null,
    }))
    .saturating_sub(4);
    let item = json!({
        "id": id,
        "tool": tool,
        "ok": !is_error,
        "result": result,
    });
    let bytes = serialized_size(&item);
    let result_bytes = bytes.saturating_sub(fixed_bytes);
    if result_bytes > MAX_PARALLEL_FANOUT_ITEM_BYTES {
        let id = item["id"].as_str().unwrap_or("unknown").to_owned();
        let tool = item["tool"].as_str().unwrap_or("unknown").to_owned();
        let item = parallel_item_error(
            id,
            tool,
            format!(
                "child result is {result_bytes}B, above the {}B fan-out item limit; use line bounds or a smaller result limit",
                MAX_PARALLEL_FANOUT_ITEM_BYTES
            ),
        );
        let bytes = serialized_size(&item);
        return (item, bytes);
    }
    (item, bytes)
}

pub(super) fn serialized_size(value: &Value) -> usize {
    struct ByteCounter(usize);

    impl std::io::Write for ByteCounter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(buffer.len());
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut counter = ByteCounter(0);
    if serde_json::to_writer(&mut counter, value).is_ok() {
        counter.0
    } else {
        0
    }
}

pub(super) fn parallel_item_error(
    id: impl Into<String>,
    tool: impl Into<String>,
    error: impl Into<String>,
) -> Value {
    json!({
        "id": id.into(),
        "tool": tool.into(),
        "ok": false,
        "error": error.into(),
    })
}
