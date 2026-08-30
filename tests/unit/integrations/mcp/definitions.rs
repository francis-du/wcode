use super::*;
use std::collections::HashSet;

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
            assert_eq!(workspace["description"], "Workspace ID; omit for default.");
        }
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
    assert!(names.contains("verify_project"));
    assert!(names.contains("apply_file_edits"));
}
