use super::corpus::{base_case, Case, Gold, Identity};
use super::delivery::inspect;
use super::scoring::digest;
use serde_json::{json, Value};

fn pack(case: &Case) -> Value {
    let sources: Vec<_> = case
        .required
        .iter()
        .map(|gold| {
            let text = &case.files[&gold.identity.path];
            let fragment = gold.fragment.trim_end_matches(['\r', '\n']);
            let offset = text.find(fragment).unwrap();
            let start = text[..offset].bytes().filter(|b| *b == b'\n').count() + 1;
            json!({"path":gold.identity.path,"qualified_name":gold.identity.symbol,
            "sha256":digest(text.as_bytes()), "body":{"content":fragment,
            "start_line":start,"end_line":start+fragment.lines().count()-1,"redacted":false}})
        })
        .collect();
    let files: Vec<_> = case
        .files
        .iter()
        .map(|(path, text)| {
            json!({
                "path":path,"sha256":digest(text.as_bytes()),"readonly":false
            })
        })
        .collect();
    json!({"budget":1000,"hot_source":sources,"files":files,"project":{"write_enabled":true}})
}

#[test]
fn engineering_fitness_delivery_body_states_partition_required_targets() {
    let mut case = base_case("rust");
    case.required.truncate(1);
    for mode in [
        "complete",
        "partial",
        "absent",
        "stale",
        "redacted",
        "wrong-lines",
        "invented",
    ] {
        let mut output = pack(&case);
        match mode {
            "partial" => {
                let text = output["hot_source"][0]["body"]["content"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                output["hot_source"][0]["body"]["content"] =
                    json!(format!("{}\n", text.lines().next().unwrap()));
                output["hot_source"][0]["body"]["end_line"] =
                    output["hot_source"][0]["body"]["start_line"].clone();
                output["hot_source"][0]["body"]["truncated"] = json!(true);
            }
            "absent" => output["hot_source"] = json!([]),
            "stale" => output["hot_source"][0]["sha256"] = json!("f".repeat(64)),
            "redacted" => output["hot_source"][0]["body"]["redacted"] = json!(true),
            "wrong-lines" => output["hot_source"][0]["body"]["start_line"] = json!(u64::MAX),
            "invented" => output["hot_source"][0]["body"]["content"] = json!("fabricated"),
            _ => {}
        }
        let result = super::scoring::score(&output, &case);
        let detail = result.delivery;
        assert_eq!(
            result.complete_body_hits
                + detail.absent_bodies.len()
                + detail.partial_original_bodies.len()
                + detail.unusable_bodies.len(),
            1,
            "{mode}"
        );
        assert_eq!(
            detail.partial_original_bodies.len(),
            usize::from(mode == "partial"),
            "{mode}"
        );
        assert_eq!(
            detail.absent_bodies.len(),
            usize::from(mode == "absent"),
            "{mode}"
        );
        assert_eq!(
            detail.unusable_bodies.len(),
            usize::from(matches!(
                mode,
                "stale" | "redacted" | "wrong-lines" | "invented"
            )),
            "{mode}"
        );
    }
}

#[test]
fn engineering_fitness_delivery_duplicate_partial_does_not_hide_full_body() {
    let case = base_case("rust");
    let mut output = pack(&case);
    let mut broken = output["hot_source"][0].clone();
    broken["sha256"] = json!("0".repeat(64));
    output["hot_source"]
        .as_array_mut()
        .unwrap()
        .insert(0, broken);
    let detail = inspect(&output, &case);
    assert!(detail.absent_bodies.is_empty());
    assert!(detail.partial_original_bodies.is_empty());
    assert!(detail.unusable_bodies.is_empty());
    assert!(detail.identified_without_complete_body.is_empty());
}

#[test]
fn engineering_fitness_delivery_deduplicates_bytes_and_penalizes_repeated_payload() {
    let case = base_case("rust");
    let original = pack(&case);
    let before = inspect(&original, &case);
    let mut duplicated = original.clone();
    let body = duplicated["hot_source"][0].clone();
    duplicated["hot_source"].as_array_mut().unwrap().push(body);
    let after = inspect(&duplicated, &case);
    assert_eq!(
        before.complete_gold_source_bytes,
        after.complete_gold_source_bytes
    );
    assert!(before.complete_gold_density > after.complete_gold_density);
    let mut duplicate_gold = case.clone();
    duplicate_gold.required.push(case.required[0].clone());
    assert_eq!(
        inspect(&original, &duplicate_gold).required_source_bytes,
        before.required_source_bytes
    );
}

#[test]
fn engineering_fitness_delivery_uses_original_unicode_crlf_bytes_and_union() {
    let mut case = base_case("rust");
    let inner = "fn inner() {\r\n        let s = \"你好🚀\";\r\n    }";
    let outer = format!("fn outer() {{\r\n    {inner}\r\n}}\r\n");
    case.files.insert("src/nested.rs".into(), outer.clone());
    case.required = vec![
        Gold {
            identity: Identity::new("src/nested.rs", "outer"),
            fragment: outer.clone(),
        },
        Gold {
            identity: Identity::new("src/nested.rs", "outer::inner"),
            fragment: format!("    {inner}"),
        },
    ];
    let observed = inspect(&pack(&case), &case);
    let expected = outer.trim_end_matches(['\r', '\n']).len();
    assert_eq!(observed.required_source_bytes, Some(expected));
    assert_eq!(observed.complete_gold_source_bytes, Some(expected));
    assert!(expected > outer.trim_end_matches(['\r', '\n']).chars().count());
}

#[test]
fn engineering_fitness_delivery_rejects_stale_redacted_or_invented_body_credit() {
    let mut case = base_case("rust");
    case.required.truncate(1);
    for pointer in [
        "/hot_source/0/sha256",
        "/hot_source/0/body/redacted",
        "/hot_source/0/body/content",
        "/hot_source/0/body/start_line",
    ] {
        let mut output = pack(&case);
        let value = match pointer {
            "/hot_source/0/sha256" => json!("0".repeat(64)),
            "/hot_source/0/body/redacted" => json!(true),
            "/hot_source/0/body/start_line" => json!(0),
            _ => json!("made up source"),
        };
        *output.pointer_mut(pointer).unwrap() = value;
        let result = inspect(&output, &case);
        assert_eq!(result.complete_gold_source_bytes, Some(0), "{pointer}");
        assert_eq!(result.identified_without_complete_body.len(), 1);
    }
}

#[test]
fn engineering_fitness_delivery_separates_missing_identity_body_sha_and_permissions() {
    let case = base_case("rust");
    let output = json!({"targets":[{"path":"src/session.rs","qualified_name":"cleanup_if_owner"}]});
    let result = inspect(&output, &case);
    assert_eq!(
        result.missing_identities,
        vec![case.required[1].identity.clone()]
    );
    assert_eq!(
        result.identified_without_complete_body,
        vec![case.required[0].identity.clone()]
    );
    assert_eq!(result.missing_current_sha.len(), 2);
    let mut readonly = case.clone();
    readonly.writable = false;
    let result = inspect(&pack(&case), &readonly);
    assert!(result.missing_identities.is_empty());
    assert!(result.identified_without_complete_body.is_empty());
    assert_eq!(result.unavailable_write_inputs.len(), 2);
}

#[test]
fn engineering_fitness_delivery_raw_bound_is_not_a_feasibility_claim() {
    let mut case = base_case("rust");
    let text = format!(
        "fn large_body() {{\n{}\n}}",
        "    // long evidence\n".repeat(300)
    );
    case.files.insert("src/large.rs".into(), text.clone());
    case.required = vec![Gold {
        identity: Identity::new("src/large.rs", "large_body"),
        fragment: text,
    }];
    let result = inspect(&json!({}), &case);
    assert!(result.required_source_bytes.unwrap() > 4_000);
    case.required.clear();
    let result = inspect(&json!({}), &case);
    assert_eq!(result.required_source_bytes, None);
    assert_eq!(result.complete_gold_density, None);
}
