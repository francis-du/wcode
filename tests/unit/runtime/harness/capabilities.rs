use super::*;

#[test]
fn v080_capabilities_are_runtime_declared_and_bounded() {
    let capabilities = ToolHarness::new(8).unwrap().capabilities();

    let scan = &capabilities["repository_scanning"];
    assert_eq!(scan["gitignore"], true);
    assert_eq!(scan["dot_ignore"], true);
    assert_eq!(scan["generated_directories_pruned"], true);
    assert_eq!(scan["explicit_ignored_paths_queryable"], true);

    let observatory = &capabilities["observatory"];
    assert_eq!(observatory["cached_snapshots"], true);
    assert_eq!(observatory["background_single_flight_refresh"], true);
    assert_eq!(observatory["revision_stamped_cache"], true);
    assert_eq!(observatory["code_graph_lazy_loaded"], true);

    let twin = &capabilities["software_intelligence"]["engineering_digital_twin"];
    assert_eq!(twin["code_graph"], true);
    assert_eq!(twin["max_depth"], 4);
    assert_eq!(twin["max_nodes"], 240);
    assert_eq!(twin["history_navigation"], true);
    assert!(twin["modes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|mode| mode == "impact"));

    let decision = &capabilities["software_intelligence"]["decision_plane"];
    assert_eq!(decision["authority"], "advisory_only");
    assert_eq!(decision["shadow_ab"], true);
    assert_eq!(decision["independent_fitness_calibration"], true);
}
