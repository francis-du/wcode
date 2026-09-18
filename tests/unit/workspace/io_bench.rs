use super::*;
use std::collections::BTreeSet;
use std::process::Command;
use std::time::Instant;

fn distribution(label: &str, mut samples: Vec<u128>) -> Value {
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let p95 = samples[(samples.len() * 95).div_ceil(100).saturating_sub(1)];
    json!({"name":label,"samples":samples.len(),"median_us":median,"p95_us":p95})
}

fn legacy_bounded_edits(content: &str, edits: &[TextEdit]) -> String {
    let mut starts = vec![0];
    starts.extend(
        content
            .bytes()
            .enumerate()
            .filter_map(|(i, c)| (c == b'\n' && i + 1 < content.len()).then_some(i + 1)),
    );
    let mut ranges = Vec::new();
    for (i, edit) in edits.iter().enumerate() {
        let from = starts[edit.start_line.unwrap() - 1];
        let to = starts
            .get(edit.end_line.unwrap())
            .copied()
            .unwrap_or(content.len());
        let hits = content[from..to]
            .match_indices(&edit.old_text)
            .collect::<Vec<_>>();
        assert_eq!(hits.len(), 1);
        ranges.push((from + hits[0].0, from + hits[0].0 + edit.old_text.len(), i));
    }
    ranges.sort_unstable_by_key(|r| r.0);
    let mut updated = content.to_owned();
    for (start, end, i) in ranges.into_iter().rev() {
        updated.replace_range(start..end, &edits[i].new_text);
    }
    updated
}

#[test]
#[ignore = "explicit release-profile microbenchmark; requires installed rg; not a model leaderboard"]
fn competitive_io_benchmark() {
    let dir = tempfile::tempdir().unwrap();
    let patterns = ["BUG_ALPHA", "BUG_BETA", "BUG_RARE"];
    for i in 0..768 {
        let mut source = "package demo\n".to_owned();
        if i % 3 == 0 {
            source.push_str("// BUG_ALPHA\n");
        }
        if i % 7 == 0 {
            source.push_str("// BUG_BETA\n");
        }
        if i == 767 {
            source.push_str("// BUG_RARE\n");
        }
        source.push_str(&"// unrelated source context\n".repeat(60));
        fs::write(dir.path().join(format!("file_{i:04}.go")), source).unwrap();
    }
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let query = request(&patterns, SearchMode::Regex, 2000);
    let execute_rg = || {
        Command::new("rg")
            .args([
                "--json",
                "--no-ignore",
                "-g",
                "*.go",
                "-e",
                patterns[0],
                "-e",
                patterns[1],
                "-e",
                patterns[2],
            ])
            .arg(dir.path())
            .output()
            .expect("install rg before running this explicit benchmark")
    };
    let baseline = execute_rg();
    assert!(baseline.status.success());
    let mut expected = BTreeSet::new();
    for line in String::from_utf8(baseline.stdout).unwrap().lines() {
        let value: Value = serde_json::from_str(line).unwrap();
        if value["type"] != "match" {
            continue;
        }
        let path = std::path::Path::new(value["data"]["path"]["text"].as_str().unwrap());
        let relative = path
            .strip_prefix(dir.path())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        expected.insert((relative, value["data"]["line_number"].as_u64().unwrap()));
    }
    let initial = report(&workspace, &query, false);
    let actual = initial["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["path"].as_str().unwrap().to_owned(),
                row["line"].as_u64().unwrap(),
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual, expected,
        "same-fixture union recall must equal ripgrep"
    );
    assert_eq!(expected.len(), 367);
    assert_eq!(initial["coverage_complete"], true);
    let mut batch = Vec::new();
    let mut sequential = Vec::new();
    let mut rg = Vec::new();
    for round in 0..15 {
        // Alternate execution order to reduce systematic warm-cache advantage.
        let order = if round % 2 == 0 { [0, 1, 2] } else { [2, 1, 0] };
        for lane in order {
            let start = Instant::now();
            match lane {
                0 => {
                    std::hint::black_box(
                        serde_json::to_vec(&report(&workspace, &query, false)).unwrap(),
                    );
                    batch.push(start.elapsed().as_micros());
                }
                1 => {
                    for pattern in patterns {
                        let single = request(&[pattern], SearchMode::Regex, 2000);
                        std::hint::black_box(
                            serde_json::to_vec(&report(&workspace, &single, false)).unwrap(),
                        );
                    }
                    sequential.push(start.elapsed().as_micros());
                }
                _ => {
                    let output = execute_rg();
                    assert!(output.status.success());
                    std::hint::black_box(output.stdout);
                    rg.push(start.elapsed().as_micros());
                }
            }
        }
    }
    let full_bytes = serde_json::to_vec(&initial).unwrap().len();
    let mut files_request = request(&patterns, SearchMode::Regex, 2000);
    files_request.output_mode = "files_with_matches".into();
    let files_bytes = serde_json::to_vec(&report(&workspace, &files_request, false))
        .unwrap()
        .len();
    let mut original = String::new();
    let mut edits = Vec::new();
    for i in 0..128 {
        let old = format!("change_{i:04}");
        original.push_str(&format!("{old}\n"));
        edits.push(TextEdit {
            old_text: old,
            new_text: format!("replacement_{i}_{}", "x".repeat(64)),
            start_line: Some(i + 1),
            end_line: Some(i + 1),
        });
    }
    original.push_str(&"unchanged content ".repeat(60_000));
    assert_eq!(
        legacy_bounded_edits(&original, &edits),
        apply_text_edits(&original, &edits).unwrap()
    );
    let mut old_edits = Vec::new();
    let mut stitched = Vec::new();
    for round in 0..31 {
        for lane in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
            let start = Instant::now();
            if lane == 0 {
                std::hint::black_box(legacy_bounded_edits(&original, &edits));
                old_edits.push(start.elapsed().as_micros());
            } else {
                std::hint::black_box(apply_text_edits(&original, &edits).unwrap());
                stitched.push(start.elapsed().as_micros());
            }
        }
    }
    println!(
        "COMPETITIVE_IO_BENCH {}",
        json!({
            "profile":if cfg!(debug_assertions) {"debug"} else {"release"},
            "os":std::env::consts::OS,"arch":std::env::consts::ARCH,
            "fixture_files":768,"matched_lines":expected.len(),"recall_equal_rg":true,
            "search_json_bytes":full_bytes,"files_only_json_bytes":files_bytes,
            "edit_source_bytes":original.len(),"edits":edits.len(),
            "measurements":[distribution("wcode_batch_3_patterns_json",batch),distribution("wcode_sequential_3_patterns_json",sequential),distribution("rg_process_3_patterns_json",rg),distribution("legacy_128_edits",old_edits),distribution("stitched_128_edits",stitched)],
            "caveats":"warm-cache local microbenchmark; wcode validates paths and emits SHA/coverage, rg includes process startup with a different JSON shape; no model generation or remote MCP latency measured"
        })
    );
}
