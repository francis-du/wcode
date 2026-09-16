use super::*;
use serde_json::{json, Value};

struct AdversarialScenario {
    name: &'static str,
    mutate: fn(&mut Value),
    expected_failure: &'static str,
}

fn healthy_workspace() -> Value {
    json!({
        "workspace": "demo",
        "design": {"initialized": true, "valid": true},
        "traceability": {
            "requirement_to_component": {"percent": 100},
            "design_to_implementation": {"percent": 100},
            "acceptance_to_verification": {"percent": 100}
        },
        "product_scope_required": true,
        "scope_status": {
            "truncated": false,
            "source_files": 4,
            "mapped_files": 4,
            "unmapped_files": []
        },
        "conventions": {"truncated": false, "errors": 0}
    })
}

#[test]
fn healthy_release_state_passes_the_canonical_gate() {
    assert!(failures(&[healthy_workspace()]).is_empty());
}

#[test]
fn incomplete_release_state_cannot_pass() {
    assert!(
        !failures(&[]).is_empty(),
        "no audited Workspace is not a pass"
    );
    for pointer in [
        "/product_scope_required",
        "/scope_status/truncated",
        "/scope_status/unmapped_files",
        "/conventions/truncated",
        "/conventions/errors",
    ] {
        for invalid in [Value::Null, json!("unknown"), json!(-1)] {
            let mut workspace = healthy_workspace();
            *workspace.pointer_mut(pointer).unwrap() = invalid;
            assert!(
                !failures(&[workspace]).is_empty(),
                "missing or invalid {pointer} passed"
            );
        }
    }
}

#[test]
fn adversarial_release_state_mutants_fail_closed() {
    let scenarios = [
        AdversarialScenario {
            name: "design-uninitialized",
            mutate: |workspace| workspace["design"]["initialized"] = json!(false),
            expected_failure: "Design State is uninitialized",
        },
        AdversarialScenario {
            name: "design-invalid",
            mutate: |workspace| workspace["design"]["valid"] = json!(false),
            expected_failure: "Design State is invalid",
        },
        AdversarialScenario {
            name: "requirement-component-trace-regression",
            mutate: |workspace| {
                workspace["traceability"]["requirement_to_component"]["percent"] = json!(99)
            },
            expected_failure: "requirement→component traceability is incomplete",
        },
        AdversarialScenario {
            name: "implementation-trace-regression",
            mutate: |workspace| {
                workspace["traceability"]["design_to_implementation"]["percent"] = json!(99)
            },
            expected_failure: "design→implementation traceability is incomplete",
        },
        AdversarialScenario {
            name: "verification-trace-regression",
            mutate: |workspace| {
                workspace["traceability"]["acceptance_to_verification"]["percent"] = json!(99)
            },
            expected_failure: "acceptance→verification mapping traceability is incomplete",
        },
        AdversarialScenario {
            name: "scope-audit-truncated",
            mutate: |workspace| workspace["scope_status"]["truncated"] = json!(true),
            expected_failure: "Product Scope audit was truncated",
        },
        AdversarialScenario {
            name: "scope-mapping-gap",
            mutate: |workspace| workspace["scope_status"]["mapped_files"] = json!(3),
            expected_failure: "Product Scope source mapping is incomplete",
        },
        AdversarialScenario {
            name: "convention-audit-truncated",
            mutate: |workspace| workspace["conventions"]["truncated"] = json!(true),
            expected_failure: "Convention audit was truncated",
        },
        AdversarialScenario {
            name: "required-convention-error",
            mutate: |workspace| workspace["conventions"]["errors"] = json!(1),
            expected_failure: "Convention audit contains required-policy errors",
        },
    ];

    for scenario in scenarios {
        let mut workspace = healthy_workspace();
        (scenario.mutate)(&mut workspace);
        let result = failures(&[workspace]);
        assert!(
            result
                .iter()
                .any(|failure| failure.contains(scenario.expected_failure)),
            "scenario {} escaped canonical release gate: {:?}",
            scenario.name,
            result
        );
    }
}
