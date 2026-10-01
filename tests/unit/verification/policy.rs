use super::*;
use crate::design::{AcceptanceRule, DocsOnlyPolicy, PolicyPath, PolicySelector};
use std::collections::BTreeMap;

fn raw_hash(character: char) -> String {
    character.to_string().repeat(64)
}

fn identity(character: char) -> String {
    format!("sha256:{}", raw_hash(character))
}

fn requirements(level: PolicyLevel, check: &str) -> PolicyRequirements {
    PolicyRequirements {
        minimum_level: level,
        checks: vec![check.into()],
        stages: Vec::new(),
        reviewers: Vec::new(),
        human_approval: false,
        human_approval_min_risk: None,
    }
}

fn check(id: &str, level: &str, phase: u8, args: &[&str]) -> FrozenPolicyCheck {
    let args = args
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect::<Vec<_>>();
    FrozenPolicyCheck {
        binding: RequiredVerificationCheck::from_command(id, "cargo", &args, ".", "."),
        level: level.into(),
        phase,
        program: "cargo".into(),
        args,
        cwd: Some(".".into()),
        island: Some(".".into()),
    }
}

fn snapshot() -> PolicySnapshot {
    PolicySnapshot {
        schema_version: 1,
        workspace: "fixture".into(),
        root_digest: identity('a'),
        revision: Revision {
            code: identity('b'),
            design: Some(identity('c')),
        },
        policy: AcceptancePolicy {
            schema_version: 1,
            id: "local-policy".into(),
            version: 1,
            requirements: requirements(PolicyLevel::Full, "rust-test"),
            docs_only: Some(DocsOnlyPolicy {
                paths: vec![PolicyPath::Directory {
                    path: "docs".into(),
                }],
                require: requirements(PolicyLevel::Quick, "rust-format"),
            }),
            rules: vec![AcceptanceRule {
                id: "auth-boundary".into(),
                when: PolicySelector {
                    paths: Vec::new(),
                    components: vec!["component:auth".into()],
                    requirements: vec!["REQ-AUTH".into()],
                },
                require: requirements(PolicyLevel::Full, "security-test"),
            }],
        },
        mappings: PolicyPathMappings {
            complete: true,
            components: BTreeMap::from([("component:auth".into(), vec!["src/auth.rs".into()])]),
            requirements: BTreeMap::from([("REQ-AUTH".into(), vec!["src/auth.rs".into()])]),
        },
        checks: vec![
            check("rust-test", "full", 1, &["test", "--locked"]),
            check("rust-format", "quick", 0, &["fmt", "--check"]),
            check("security-test", "full", 1, &["test", "--test", "security"]),
        ],
        sources: vec![
            PolicySourceDigest {
                path: "Cargo.toml".into(),
                sha256: Some(raw_hash('a')),
            },
            PolicySourceDigest {
                path: ".wcode/project.yaml".into(),
                sha256: Some(raw_hash('b')),
            },
            PolicySourceDigest {
                path: "package.json".into(),
                sha256: None,
            },
        ],
    }
}

fn rejection(change: impl FnOnce(&mut PolicySnapshot), expected: &str) {
    let mut value = snapshot();
    change(&mut value);
    let error = value
        .validate()
        .expect_err("invalid snapshot must be rejected");
    assert!(format!("{error:#}").contains(expected), "{error:#}");
}

#[test]
fn policy_snapshot_is_bounded_roundtrippable_and_has_no_approval_claim() {
    let value = snapshot();
    value.validate().unwrap();
    let json = serde_json::to_value(&value).unwrap();
    assert_eq!(
        serde_json::from_value::<PolicySnapshot>(json.clone()).unwrap(),
        value
    );
    assert!(json.get("approved").is_none());
    assert!(json.get("producer").is_none());
    assert!(json.get("human_decision").is_none());
    let digest = value.digest().unwrap();
    assert!(sha256_identity(&digest));
    assert_eq!(value.digest().unwrap(), digest);
    let mut historical = value.clone();
    historical.revision.code = identity('d');
    historical.validate().unwrap();
    assert_ne!(historical.digest().unwrap(), digest);
}

#[test]
fn every_policy_branch_requires_its_exact_frozen_checks() {
    rejection(
        |value| {
            value.checks.remove(2);
        },
        "unavailable frozen check",
    );
    rejection(
        |value| {
            value
                .policy
                .docs_only
                .as_mut()
                .unwrap()
                .require
                .checks
                .push("unknown-doc-check".into());
        },
        "unavailable frozen check",
    );
    rejection(
        |value| {
            value.policy.rules[0]
                .require
                .checks
                .push("unknown-unmatched-check".into());
        },
        "unavailable frozen check",
    );
    rejection(
        |value| {
            value
                .checks
                .push(check("unused-check", "quick", 0, &["fmt", "--check"]));
        },
        "unreferenced frozen check",
    );
    rejection(
        |value| {
            value.checks.push(value.checks[0].clone());
        },
        "duplicate frozen check IDs",
    );
}

#[test]
fn frozen_command_signature_binds_program_arguments_cwd_and_island() {
    rejection(
        |value| {
            value.checks[0].program = "true".into();
        },
        "signature",
    );
    rejection(
        |value| {
            value.checks[0].args.push("--ignored".into());
        },
        "signature",
    );
    rejection(
        |value| {
            value.checks[0].cwd = Some("src".into());
        },
        "signature",
    );
    rejection(
        |value| {
            value.checks[0].island = Some("other-island".into());
        },
        "signature",
    );
    rejection(
        |value| {
            value.checks[0].binding.signature = identity('d');
        },
        "signature",
    );
    let mut value = snapshot();
    value.checks[0].island = Some("workspace".into());
    let current = &mut value.checks[0];
    current.binding = RequiredVerificationCheck::from_command(
        &current.binding.id,
        &current.program,
        &current.args,
        ".",
        "workspace",
    );
    value.validate().unwrap();
}

#[test]
fn frozen_execution_metadata_is_explicit_and_lexically_canonical() {
    rejection(
        |value| {
            value.checks[0].cwd = None;
        },
        "cwd is missing",
    );
    rejection(
        |value| {
            value.checks[0].island = None;
        },
        "island is missing",
    );
    for path in [
        "../child",
        "/tmp",
        "src//child",
        "src/./child",
        "C:\\child",
        "src/*",
    ] {
        rejection(
            |value| {
                value.checks[0].cwd = Some(path.into());
            },
            "not canonical",
        );
        rejection(
            |value| {
                value.checks[0].island = Some(path.into());
            },
            "not canonical",
        );
    }
    rejection(
        |value| {
            value.checks[0].phase = 4;
        },
        "invalid frozen",
    );
    rejection(
        |value| {
            value.checks[0].level = "fast".into();
        },
        "invalid frozen",
    );
    rejection(
        |value| {
            value.checks[0].program = "cargo test".into();
        },
        "invalid frozen",
    );
    rejection(
        |value| {
            value.checks[0].args = vec!["x".repeat(4097)];
        },
        "invalid frozen",
    );
    rejection(
        |value| {
            value.checks[0].args = vec!["--quiet".into(); 129];
        },
        "invalid frozen",
    );
    rejection(
        |value| {
            value.checks[0].args = vec!["x".repeat(4096); 17];
        },
        "invalid frozen",
    );
}

#[test]
fn policy_identity_requires_complete_lowercase_root_code_and_design_hashes() {
    rejection(
        |value| {
            value.schema_version = 0;
        },
        "complete root/code/Design",
    );
    rejection(
        |value| {
            value.workspace = " fixture".into();
        },
        "complete root/code/Design",
    );
    rejection(
        |value| {
            value.root_digest = identity('a').to_uppercase();
        },
        "complete root/code/Design",
    );
    rejection(
        |value| {
            value.revision.code = "sha256:partial".into();
        },
        "complete root/code/Design",
    );
    rejection(
        |value| {
            value.revision.code.push_str(":partial");
        },
        "complete root/code/Design",
    );
    rejection(
        |value| {
            value.revision.design = None;
        },
        "complete root/code/Design",
    );
    rejection(
        |value| {
            value.revision.design = Some("legacy".into());
        },
        "complete root/code/Design",
    );
}

#[test]
fn policy_mappings_require_complete_known_nonempty_file_scopes() {
    rejection(
        |value| {
            value.mappings.complete = false;
        },
        "mappings are incomplete",
    );
    rejection(
        |value| {
            value.mappings.components.clear();
        },
        "policy-design-mapping-unavailable",
    );
    rejection(
        |value| {
            value.mappings.requirements.clear();
        },
        "policy-design-mapping-unavailable",
    );
    rejection(
        |value| {
            value
                .mappings
                .components
                .get_mut("component:auth")
                .unwrap()
                .clear();
        },
        "file scope is missing",
    );
    rejection(
        |value| {
            value
                .mappings
                .components
                .get_mut("component:auth")
                .unwrap()
                .push("src/auth.rs".into());
        },
        "invalid or duplicate file path",
    );
    rejection(
        |value| {
            value
                .mappings
                .requirements
                .insert("REQ-AUTH".into(), vec!["../external.rs".into()]);
        },
        "invalid or duplicate file path",
    );
}

#[test]
fn missing_source_is_explicit_and_cannot_be_confused_with_a_present_definition() {
    let value = snapshot();
    value.validate().unwrap();
    let seal = value.source_seal_digest().unwrap();
    let mut changed = value.clone();
    changed.sources[2].sha256 = Some(raw_hash('d'));
    changed.validate().unwrap();
    assert_ne!(changed.source_seal_digest().unwrap(), seal);
    assert_ne!(changed.digest().unwrap(), value.digest().unwrap());
    rejection(
        |value| {
            for source in &mut value.sources {
                source.sha256 = None;
            }
        },
        "no captured source definition",
    );
    rejection(
        |value| {
            value.sources[0].sha256 = Some(identity('a'));
        },
        "complete native SHA256",
    );
    rejection(
        |value| {
            value.sources[0].sha256 = Some(raw_hash('a').to_uppercase());
        },
        "complete native SHA256",
    );
    rejection(
        |value| {
            value.sources[0].path = "../Cargo.toml".into();
        },
        "invalid or duplicate path",
    );
    rejection(
        |value| {
            value.sources.push(value.sources[0].clone());
        },
        "invalid or duplicate path",
    );
}

#[test]
fn source_seal_is_canonical_but_snapshot_freezes_exact_ordered_inputs() {
    let value = snapshot();
    let mut reordered = value.clone();
    reordered.sources.reverse();
    assert_eq!(
        value.source_seal_digest().unwrap(),
        reordered.source_seal_digest().unwrap()
    );
    assert_ne!(value.digest().unwrap(), reordered.digest().unwrap());
    let mut changed = value.clone();
    changed.root_digest = identity('d');
    assert_ne!(
        value.source_seal_digest().unwrap(),
        changed.source_seal_digest().unwrap()
    );
    let mut phase_changed = value.clone();
    phase_changed.checks[0].phase = 2;
    assert_ne!(value.digest().unwrap(), phase_changed.digest().unwrap());
    let mut policy_changed = value.clone();
    policy_changed.policy.version += 1;
    assert_ne!(value.digest().unwrap(), policy_changed.digest().unwrap());
}

#[test]
fn snapshot_rejects_count_overflow_without_truncating() {
    rejection(
        |value| {
            value.sources = (0..=MAX_SOURCE_PATHS)
                .map(|index| PolicySourceDigest {
                    path: format!("input-{index}.json"),
                    sha256: Some(raw_hash('a')),
                })
                .collect();
        },
        "count bound",
    );
    rejection(
        |value| {
            value.checks = vec![value.checks[0].clone(); MAX_FROZEN_CHECKS + 1];
        },
        "count bound",
    );
    rejection(
        |value| {
            value.mappings.components.insert(
                "component:auth".into(),
                (0..=MAX_MAPPING_PATHS)
                    .map(|index| format!("src/file-{index}.rs"))
                    .collect(),
            );
        },
        "count bound",
    );
    rejection(
        |value| {
            value.mappings.components = (0..=MAX_MAPPING_IDS)
                .map(|index| {
                    (
                        format!("component:{index}"),
                        vec![format!("src/file-{index}.rs")],
                    )
                })
                .collect();
        },
        "count bound",
    );
}

#[test]
fn snapshot_streaming_serialization_rejects_large_but_individually_valid_inputs() {
    rejection(
        |value| {
            value.sources = (0..3000)
                .map(|index| PolicySourceDigest {
                    path: format!("input-{index:04}/{}.toml", "x".repeat(950)),
                    sha256: Some(raw_hash('a')),
                })
                .collect();
        },
        "exceeds 2 MiB",
    );
}

#[test]
fn policy_snapshot_models_deny_unknown_fields_at_every_boundary() {
    let value = snapshot();
    for target in ["snapshot", "check", "source", "mappings"] {
        let mut json = serde_json::to_value(&value).unwrap();
        let object = match target {
            "snapshot" => json.as_object_mut().unwrap(),
            "check" => json["checks"][0].as_object_mut().unwrap(),
            "source" => json["sources"][0].as_object_mut().unwrap(),
            "mappings" => json["mappings"].as_object_mut().unwrap(),
            _ => unreachable!(),
        };
        object.insert("approved".into(), serde_json::Value::Bool(true));
        assert!(
            serde_json::from_value::<PolicySnapshot>(json).is_err(),
            "{target}"
        );
    }
}
