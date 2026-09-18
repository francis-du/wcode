//! All partitions reuse the primary aggregation, including failed attempts.
use super::{percent, summarize_selected, Row};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn build(rows: &[Row]) -> Value {
    let mut result = serde_json::Map::new();
    for dimension in ["language", "category", "target_count"] {
        let mut groups = BTreeMap::<&str, Vec<&Row>>::new();
        for row in rows {
            let key = match dimension {
                "language" => row.language.as_str(),
                "category" => row.category.as_str(),
                _ => match row.required_count {
                    0 => "none",
                    1 => "single",
                    _ => "multiple",
                },
            };
            groups.entry(key).or_default().push(row);
        }
        result.insert(
            dimension.into(),
            Value::Object(
                groups
                    .into_iter()
                    .map(|(key, rows)| (key.into(), json!(summarize_selected(rows))))
                    .collect(),
            ),
        );
    }
    let mut misses = Vec::new();
    for row in rows.iter().filter(|row| row.required_count > 0) {
        let mut identities = BTreeSet::new();
        let mut bodies = BTreeSet::new();
        let mut shas = BTreeSet::new();
        let mut writes = BTreeSet::new();
        let mut errors = 0;
        let mut raw_bytes = None;
        for sample in &row.samples {
            if let Some(score) = &sample.score {
                let detail = &score.delivery;
                identities.extend(detail.missing_identities.iter().cloned());
                bodies.extend(detail.identified_without_complete_body.iter().cloned());
                shas.extend(detail.missing_current_sha.iter().cloned());
                writes.extend(detail.unavailable_write_inputs.iter().cloned());
                raw_bytes = detail.required_source_bytes;
            } else {
                errors += 1;
            }
        }
        if !identities.is_empty()
            || !bodies.is_empty()
            || !shas.is_empty()
            || !writes.is_empty()
            || errors > 0
        {
            misses.push(json!({"case":row.case_id,"language":row.language,"category":row.category,
                "budget":row.budget,"phase":row.phase,"writable":row.writable,
                "attempts":row.samples.len(),"query_errors":errors,
                "missing_identities":identities,"identified_without_complete_body":bodies,
                "missing_current_sha":shas,"unavailable_write_inputs":writes,
                "raw_required_source_bytes":raw_bytes,
                "raw_source_exceeds_budget":raw_bytes.map(|n| n > row.budget.saturating_mul(4)),
                "scope":"union of observed gaps across repeats; per-attempt detail retained in rows"}));
        }
    }
    result.insert("misses".into(), json!(misses));
    Value::Object(result)
}

pub(super) fn markdown(breakdown: &Value) -> String {
    let mut text = String::new();
    for dimension in ["language", "category", "target_count"] {
        let Some(groups) = breakdown[dimension].as_object() else {
            continue;
        };
        text.push_str(&format!("\n## Breakdown: {dimension}\n\n| Group | Budget | Phase | Attempts | Recall | Full body | Current SHA | Edit inputs | Gold byte density | Errors |\n| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n"));
        for (name, groups) in groups {
            for group in groups.as_array().into_iter().flatten() {
                text.push_str(&format!(
                    "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                    name,
                    group["budget"],
                    group["phase"].as_str().unwrap_or("?"),
                    group["attempts"],
                    percent(&group["required_recall"]),
                    percent(&group["complete_body_recall"]),
                    percent(&group["fresh_sha_recall"]),
                    percent(&group["all_required_edit_inputs_rate"]),
                    percent(&group["complete_gold_density"]),
                    group["errors"]
                ));
            }
        }
    }
    text.push_str("\n## 1K cold delivery gaps\n\nMissing identities describe final delivery, not proven internal search failures. Counts are unions across repeats. Raw source bytes are only a necessary lower bound: being below 4,000 bytes does not prove the whole response fits.\n\n| Case | Missing identities | Identified without full body | Missing SHA | Raw Gold bytes | Query errors |\n| --- | --- | --- | ---: | ---: | ---: |\n");
    for row in breakdown["misses"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["budget"] == 1000 && row["phase"] == "cold")
    {
        let names = |key: &str| {
            row[key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|id| id["symbol"].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        text.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            row["case"].as_str().unwrap_or("?"),
            names("missing_identities"),
            names("identified_without_complete_body"),
            row["missing_current_sha"].as_array().map_or(0, Vec::len),
            row["raw_required_source_bytes"],
            row["query_errors"]
        ));
    }
    text
}

#[test]
#[ignore = "explicit single-sample diagnostic snapshot; saves all rows without model calls"]
fn engineering_fitness_diagnostic_snapshot() {
    let report = super::collect(1).unwrap();
    let paths = super::persist(&report).unwrap();
    println!(
        "FITNESS_SNAPSHOT {}",
        json!({"json":paths.0,"markdown":paths.1,
        "metadata":report.metadata,"summary":report.summary})
    );
    let misses: Vec<_> = report.breakdown["misses"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["budget"] == 1000 && row["phase"] == "cold")
        .collect();
    println!("FITNESS_1K_GAPS {}", json!(misses));
    assert!(report.controls.iter().all(|control| control.passed));
    assert!(report
        .rows
        .iter()
        .flat_map(|row| &row.samples)
        .all(super::valid_observation));
}
