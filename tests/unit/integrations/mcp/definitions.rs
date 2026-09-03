use super::*;
use std::collections::HashSet;

fn assert_no_model_tuning_args(value: &Value, tool_name: &str) {
    match value {
        Value::Object(object) => {
            if let Some(properties) = object.get("properties").and_then(Value::as_object) {
                for key in MODEL_HIDDEN_TUNING_ARGS {
                    assert!(
                        !properties.contains_key(*key),
                        "tool {tool_name} exposes model tuning argument {key}"
                    );
                }
            }
            for child in object.values() {
                assert_no_model_tuning_args(child, tool_name);
            }
        }
        Value::Array(items) => {
            for child in items {
                assert_no_model_tuning_args(child, tool_name);
            }
        }
        _ => {}
    }
}

#[test]
fn tool_catalog_is_deterministic_compact_and_unique() {
    let first = tools();
    let second = tools();
    assert_eq!(first, second);

    let bytes = serde_json::to_vec(&first).unwrap().len();
    assert!(bytes <= 60_000, "tool catalog is {bytes} bytes");

    let mut names = HashSet::new();
    for tool in &first {
        let name = tool["name"].as_str().unwrap();
        assert!(names.insert(name), "duplicate tool name: {name}");
        let description = tool["description"].as_str().unwrap();
        assert!(
            description.chars().count() <= MAX_TOOL_DESCRIPTION_CHARS,
            "tool {name} description is too long"
        );
        if let Some(workspace) = tool["inputSchema"]["properties"].get("workspace") {
            assert_eq!(
                workspace["description"],
                "Only pass when switching away from the default Workspace."
            );
        }
        assert!(
            !serde_json::to_string(&tool["inputSchema"])
                .unwrap()
                .contains("\"default\":"),
            "tool {name} must not advertise model-visible default arguments"
        );
        assert_no_model_tuning_args(&tool["inputSchema"], name);
    }
    let agent_context = first
        .iter()
        .find(|tool| tool["name"] == "agent_context")
        .unwrap();
    assert!(agent_context["inputSchema"]["properties"]["budget"]
        .get("default")
        .is_none());
    assert!(
        agent_context["inputSchema"]["properties"]["budget"]["description"]
            .as_str()
            .unwrap()
            .contains("adaptive")
    );
    assert!(names.contains("agent_context"));
    assert!(names.contains("semantic_navigation"));
    assert!(names.contains("verify_project"));
    assert!(names.contains("apply_file_edits"));
}
