use super::*;
use std::hint::black_box;

fn percentiles_us(mut samples: Vec<u128>) -> (f64, f64) {
    samples.sort_unstable();
    (
        samples[samples.len() / 2] as f64 / 1_000.0,
        samples[(samples.len() * 95).div_ceil(100) - 1] as f64 / 1_000.0,
    )
}

#[test]
#[ignore = "opt-in release microbenchmark; timing is not a CI pass threshold"]
fn repo_rank_release_cost_comparison() {
    for count in [128, 6_000, 12_000] {
        let input = rank_candidates(count, 42);
        let mut full_samples = Vec::new();
        let mut top_samples = Vec::new();
        for iteration in 0..34 {
            // Identical buffers prepared outside the measured selection stages.
            let mut full = input.clone();
            let mut top = input.clone();
            let measure_full = |full: &mut Vec<RepoMapCandidate>| {
                let start = Instant::now();
                full.sort_by(compare_repo_candidates);
                full.truncate(16);
                black_box(&*full);
                start.elapsed().as_nanos()
            };
            let measure_top = |top: &mut Vec<RepoMapCandidate>| {
                let start = Instant::now();
                select_repo_candidates(top, 16);
                black_box(&*top);
                start.elapsed().as_nanos()
            };
            let (full_ns, top_ns) = if iteration % 2 == 0 {
                (measure_full(&mut full), measure_top(&mut top))
            } else {
                let top_ns = measure_top(&mut top);
                (measure_full(&mut full), top_ns)
            };
            assert_eq!(ranked_ids(&full), ranked_ids(&top));
            if iteration >= 3 {
                full_samples.push(full_ns);
                top_samples.push(top_ns);
            }
        }
        let (full_median, full_p95) = percentiles_us(full_samples);
        let (top_median, top_p95) = percentiles_us(top_samples);
        println!(
            "{}",
            json!({
                "stage": "candidate-selection", "candidates": count, "limit": 16,
                "samples": 31, "unit": "us", "full_sort_p50": full_median,
                "full_sort_p95": full_p95, "top_k_p50": top_median, "top_k_p95": top_p95,
                "ordered_ids_equal": true,
            })
        );
    }

    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    let source = (0..crate::harness::REPO_MAP_MAX_SYMBOLS.saturating_sub(1))
        .map(|index| format!("pub fn item_{index}() {{}}\n"))
        .collect::<String>();
    fs::write(root.path().join("src/lib.rs"), source).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let context = harness
        .software_context("demo", &workspace, "item_0", "inspect", 4_000, &[])
        .unwrap();
    let (base, _) = harness.repo_map_graph("demo", &workspace, ".").unwrap();
    assert!(
        !base.truncated && !base.scan_truncated,
        "no-supplement benchmark needs a complete base graph"
    );
    let mut clone_samples = Vec::new();
    let mut borrow_samples = Vec::new();
    for iteration in 0..34 {
        let start = Instant::now();
        let copied = black_box(base.as_ref()).clone();
        black_box(&copied);
        let clone_ns = start.elapsed().as_nanos();
        let start = Instant::now();
        let reused = augment_relationship_graph(
            &harness,
            "demo",
            &workspace,
            "item_0",
            &context,
            black_box(&base),
        )
        .unwrap();
        black_box(&reused);
        let borrow_ns = start.elapsed().as_nanos();
        assert!(std::ptr::eq(reused.as_ref(), base.as_ref()));
        assert_eq!(copied.node_count, reused.node_count);
        if iteration >= 3 {
            clone_samples.push(clone_ns);
            borrow_samples.push(borrow_ns);
        }
    }
    let (clone_median, clone_p95) = percentiles_us(clone_samples);
    let (borrow_median, borrow_p95) = percentiles_us(borrow_samples);
    println!(
        "{}",
        json!({
            "stage": "no-supplement-graph", "nodes": base.node_count, "samples": 31,
            "unit": "us", "clone_p50": clone_median, "clone_p95": clone_p95,
            "borrow_p50": borrow_median, "borrow_p95": borrow_p95,
            "borrowed_original_graph": true,
        })
    );
}
