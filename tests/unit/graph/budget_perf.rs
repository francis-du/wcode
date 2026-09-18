use super::*;
use std::hint::black_box;
use std::time::Instant;

#[test]
#[ignore = "explicit release benchmark; elapsed time is not a CI threshold"]
fn graph_build_release_cost() {
    for symbols in [500, 5_000] {
        let root = tempfile::tempdir().unwrap();
        let source = (0..symbols)
            .map(|n| format!("pub fn item_{n:04}() -> usize {{ {n} }}\n"))
            .collect::<String>();
        fs::write(root.path().join("all.rs"), source).unwrap();
        let workspace = Workspace::new(root.path(), false, false).unwrap();
        let index = CodeIndex::new().unwrap();
        let mut timings = Vec::new();
        let mut reference = None;
        let mut result_bytes = 0;
        for iteration in 0..13 {
            let start = Instant::now();
            let graph = index
                .software_graph_from_paths(
                    &workspace,
                    vec!["all.rs".to_owned()],
                    false,
                    symbols,
                    &HashSet::new(),
                    &HashSet::new(),
                )
                .unwrap();
            black_box(&graph);
            let elapsed = start.elapsed().as_nanos();
            assert_eq!(graph.node_count, symbols + 1);
            assert_eq!(graph.edge_count, symbols);
            assert!(!graph.truncated);
            let serialized = serde_json::to_vec(&graph.graph).unwrap();
            result_bytes = serialized.len();
            if let Some(expected) = &reference {
                assert_eq!(&serialized, expected);
            } else {
                reference = Some(serialized);
            }
            if iteration >= 2 {
                timings.push(elapsed);
            }
        }
        timings.sort_unstable();
        println!(
            "{}",
            serde_json::json!({
                "stage": "warm-software-graph", "symbols": symbols, "samples": timings.len(),
                "p50_us": timings[timings.len() / 2] as f64 / 1000.0,
                "p95_us": timings[(timings.len() * 95).div_ceil(100) - 1] as f64 / 1000.0,
                "nodes": symbols + 1, "edges": symbols, "graph_bytes": result_bytes,
                "identical_outputs_across_samples": true
            })
        );
    }
}
