use super::*;
use crate::design::{load_design, ComponentDesign, Requirement};
use crate::workspace::Workspace;

fn requirements(checks: &[&str], level: PolicyLevel) -> PolicyRequirements {
    PolicyRequirements {
        minimum_level: level,
        checks: checks.iter().map(|value| (*value).into()).collect(),
        stages: Vec::new(),
        reviewers: Vec::new(),
        human_approval: false,
        human_approval_min_risk: None,
    }
}

fn policy() -> AcceptancePolicy {
    AcceptancePolicy {
        schema_version: 1,
        id: "team-baseline".into(),
        version: 1,
        requirements: requirements(&["rust-test"], PolicyLevel::Full),
        docs_only: Some(DocsOnlyPolicy {
            paths: vec![
                directory("docs"),
                PolicyPath::File {
                    path: "README.md".into(),
                },
            ],
            require: requirements(&["git-diff-check"], PolicyLevel::Quick),
        }),
        rules: Vec::new(),
    }
}

fn directory(path: &str) -> PolicyPath {
    PolicyPath::Directory { path: path.into() }
}

fn change(paths: &[&str]) -> PolicyChangeSet {
    PolicyChangeSet {
        paths: paths.iter().map(|value| (*value).into()).collect(),
        complete: true,
        executable_changes: Some(false),
        regular_file_changes: Some(true),
    }
}

fn select(policy: &AcceptancePolicy, change: &PolicyChangeSet) -> PolicySelection {
    policy
        .select(
            change,
            &PolicyPathMappings::default(),
            &requirements(&[], PolicyLevel::Quick),
            RiskLevel::Low,
        )
        .unwrap()
}

fn design() -> DesignState {
    let mut state = DesignState::default();
    let component: ComponentDesign = serde_yaml::from_str(
        "id: component:auth\nname: Auth\nimplementation:\n  - kind: file\n    path: src/auth.rs\n",
    )
    .unwrap();
    let requirement: Requirement =
        serde_yaml::from_str("id: REQ-AUTH\ntitle: Auth\nintent: Enforce access\n").unwrap();
    state.components.insert(component.id.clone(), component);
    state
        .requirements
        .insert(requirement.id.clone(), requirement);
    state
}

#[test]
fn project_policy_is_optional_and_unknown_commands_are_rejected() {
    let legacy: ProjectDesign = serde_yaml::from_str("name: demo\ndescription: legacy\n").unwrap();
    assert!(legacy.acceptance_policy.is_none());
    let body = serde_json::to_value(policy()).unwrap();
    for field in [
        "program",
        "argv",
        "shell",
        "authority",
        "trusted",
        "activated",
        "workspace",
    ] {
        let mut forged = body.clone();
        forged[field] = serde_json::json!(true);
        assert!(
            serde_json::from_value::<AcceptancePolicy>(forged).is_err(),
            "{field}"
        );
    }
    let mut forged = body;
    forged["default"]["command"] = serde_json::json!("echo success");
    assert!(serde_json::from_value::<AcceptancePolicy>(forged).is_err());
    let mut forged = serde_json::to_value(directory("docs")).unwrap();
    forged["exclude"] = serde_json::json!(["src/auth.rs"]);
    assert!(serde_json::from_value::<PolicyPath>(forged).is_err());
}

#[test]
fn policy_validation_rejects_bounds_paths_duplicates_and_unknown_design_refs() {
    assert!(policy().validate(&design()).is_empty());
    for path in [
        "../src",
        "/tmp",
        "C:/src",
        "src\\auth",
        "src/**",
        "src//auth",
        ".",
    ] {
        let mut draft = policy();
        draft.rules.push(AcceptanceRule {
            id: "auth".into(),
            when: PolicySelector {
                paths: vec![directory(path)],
                ..Default::default()
            },
            require: requirements(&[], PolicyLevel::Quick),
        });
        assert!(!draft.validate(&design()).is_empty(), "{path}");
    }
    let rule = AcceptanceRule {
        id: "auth".into(),
        when: PolicySelector {
            paths: vec![directory("src/auth")],
            ..Default::default()
        },
        require: requirements(&[], PolicyLevel::Quick),
    };
    let mut draft = policy();
    draft.rules = vec![rule.clone(), rule.clone()];
    assert!(draft
        .validate(&design())
        .contains(&"invalid-or-duplicate-policy-rule".into()));
    draft.rules = (0..33)
        .map(|i| AcceptanceRule {
            id: format!("rule-{i}"),
            ..rule.clone()
        })
        .collect();
    assert!(draft
        .validate(&design())
        .contains(&"policy-rule-bound".into()));
    draft.rules = vec![AcceptanceRule {
        when: PolicySelector {
            components: vec!["component:missing".into()],
            requirements: vec!["REQ-MISSING".into()],
            ..Default::default()
        },
        ..rule
    }];
    let errors = draft.validate(&design());
    assert!(errors.contains(&"unknown-policy-component".into()));
    assert!(errors.contains(&"unknown-policy-requirement".into()));
    draft.rules.clear();
    draft.requirements.checks = (0..33).map(|i| format!("check-{i}")).collect();
    assert!(draft
        .validate(&design())
        .contains(&"invalid-policy-checks".into()));
    draft.requirements.checks = vec!["rust-test".into(), "rust-test".into()];
    assert!(draft
        .validate(&design())
        .contains(&"invalid-policy-checks".into()));
    draft.requirements.checks.clear();
    assert!(draft
        .validate(&design())
        .contains(&"empty-required-policy-checks".into()));
}

#[test]
fn policy_rules_accumulate_order_independently_and_keep_risk_floor() {
    let mut draft = policy();
    draft.requirements.minimum_level = PolicyLevel::Quick;
    draft.rules = vec![
        AcceptanceRule {
            id: "auth".into(),
            when: PolicySelector {
                paths: vec![directory("src/auth")],
                ..Default::default()
            },
            require: PolicyRequirements {
                stages: vec![VerificationStage::Property],
                reviewers: vec![ReviewerRole::Security],
                human_approval_min_risk: Some(RiskLevel::High),
                ..requirements(&["auth-integration"], PolicyLevel::Full)
            },
        },
        AcceptanceRule {
            id: "payment".into(),
            when: PolicySelector {
                paths: vec![directory("src/payment")],
                ..Default::default()
            },
            require: PolicyRequirements {
                human_approval: true,
                ..requirements(&["payment-integration"], PolicyLevel::Quick)
            },
        },
    ];
    let floor = PolicyRequirements {
        stages: vec![VerificationStage::Mutation],
        reviewers: vec![ReviewerRole::Adversarial],
        ..requirements(&["rust-clippy"], PolicyLevel::Full)
    };
    let input = change(&["src/auth/token.rs", "src/payment/charge.rs"]);
    let first = draft
        .select(
            &input,
            &PolicyPathMappings::default(),
            &floor,
            RiskLevel::Medium,
        )
        .unwrap();
    draft.rules.reverse();
    let second = draft
        .select(
            &input,
            &PolicyPathMappings::default(),
            &floor,
            RiskLevel::Medium,
        )
        .unwrap();
    assert_eq!(first.requirements, second.requirements);
    assert_eq!(first.matched_rules, second.matched_rules);
    assert_eq!(first.requirements.minimum_level, PolicyLevel::Full);
    assert!(first.requirements.human_approval);
    assert_eq!(
        first.requirements.checks,
        [
            "auth-integration",
            "payment-integration",
            "rust-clippy",
            "rust-test"
        ]
    );
    assert_eq!(first.requirements.stages.len(), 2);
    assert_eq!(first.requirements.reviewers.len(), 2);
    let boundary = select(&draft, &change(&["src/authentic/main.rs"]));
    assert!(boundary.matched_rules.is_empty());
    assert!(!boundary.requirements.human_approval);
    let high = draft
        .select(
            &change(&["src/auth/token.rs"]),
            &PolicyPathMappings::default(),
            &requirements(&[], PolicyLevel::Quick),
            RiskLevel::High,
        )
        .unwrap();
    assert!(high.requirements.human_approval);

    // Union must reject exceeding a bound instead of silently dropping checks.
    draft.requirements.checks = (0..32).map(|i| format!("base-{i}")).collect();
    assert!(draft
        .select(
            &input,
            &PolicyPathMappings::default(),
            &floor,
            RiskLevel::High
        )
        .is_err());
}

#[test]
fn docs_only_requires_complete_old_and_new_markdown_paths_and_known_modes() {
    let draft = policy();
    let docs = select(&draft, &change(&["docs/guide.md", "README.md"]));
    assert!(docs.docs_only);
    assert_eq!(docs.requirements.minimum_level, PolicyLevel::Quick);
    for paths in [
        vec!["src/auth.rs", "docs/auth.md"], // both rename paths must be captured
        vec!["docs/guide.md", "src/lib.rs"],
        vec!["docs/guide.mdx"],
        vec!["docs/.github/workflows/build.md"],
        vec!["docs/.wcode/policy.md"],
        vec!["docs/.git/hooks/pre-commit.md"],
        vec![],
    ] {
        let selected = select(&draft, &change(&paths));
        assert!(!selected.docs_only, "{paths:?}");
        assert_eq!(selected.requirements.minimum_level, PolicyLevel::Full);
    }
    for name in [
        "AGENTS.md",
        "agents.md",
        "Agents.md",
        "CLAUDE.md",
        "claude.md",
        "Claude.md",
        "SKILL.md",
        "skill.md",
        "Skill.md",
    ] {
        for prefix in ["", "docs/nested/", "plugin/skills/wcode/"] {
            let path = format!("{prefix}{name}");
            let mut instruction_scope = policy();
            instruction_scope.docs_only.as_mut().unwrap().paths = if prefix.is_empty() {
                vec![PolicyPath::File { path: path.clone() }]
            } else {
                vec![directory(prefix.trim_end_matches('/'))]
            };
            let selected = select(&instruction_scope, &change(&[&path]));
            assert!(
                !selected.docs_only,
                "{path} is an instruction, even in an explicit docs scope"
            );
            assert_eq!(selected.requirements.minimum_level, PolicyLevel::Full);
            assert_eq!(selected.requirements.checks, ["rust-test"]);
        }
    }
    for modes in [None, Some(true)] {
        let input = PolicyChangeSet {
            executable_changes: modes,
            ..change(&["docs/guide.md"])
        };
        assert!(!select(&draft, &input).docs_only);
    }
    for modes in [None, Some(false)] {
        let input = PolicyChangeSet {
            regular_file_changes: modes,
            ..change(&["README.md"])
        };
        assert!(
            !select(&draft, &input).docs_only,
            "non-regular or unknown modes cannot lower checks"
        );
    }
    let input = PolicyChangeSet {
        complete: false,
        ..change(&["docs/guide.md"])
    };
    assert!(draft
        .select(
            &input,
            &PolicyPathMappings::default(),
            &requirements(&[], PolicyLevel::Quick),
            RiskLevel::Low
        )
        .is_err());
    let floor = PolicyRequirements {
        human_approval: true,
        ..requirements(&["security-review"], PolicyLevel::Full)
    };
    let selected = draft
        .select(
            &change(&["README.md"]),
            &PolicyPathMappings::default(),
            &floor,
            RiskLevel::High,
        )
        .unwrap();
    assert!(selected.docs_only);
    assert!(selected.requirements.human_approval);
    assert_eq!(selected.requirements.minimum_level, PolicyLevel::Full);
    assert!(selected
        .requirements
        .checks
        .contains(&"security-review".into()));
}

#[test]
fn policy_mapping_inputs_fail_closed_and_use_frozen_file_scopes() {
    let mut draft = policy();
    draft.rules.push(AcceptanceRule {
        id: "auth".into(),
        when: PolicySelector {
            components: vec!["component:auth".into()],
            requirements: vec!["REQ-AUTH".into()],
            ..Default::default()
        },
        require: requirements(&["auth-integration"], PolicyLevel::Full),
    });
    let mut frozen = PolicyPathMappings {
        complete: true,
        components: BTreeMap::from([("component:auth".into(), vec!["src/auth.rs".into()])]),
        requirements: BTreeMap::from([("REQ-AUTH".into(), vec!["src/token.rs".into()])]),
    };
    let floor = requirements(&[], PolicyLevel::Quick);
    let selected = draft
        .select(&change(&["src/token.rs"]), &frozen, &floor, RiskLevel::Low)
        .unwrap();
    assert_eq!(selected.matched_rules, ["auth"]);
    let mut candidate = design();
    candidate
        .components
        .get_mut("component:auth")
        .unwrap()
        .implementation
        .clear();
    assert!(draft.validate(&candidate).is_empty());
    // Matching depends on the explicit frozen scopes, not candidate remapping.
    assert_eq!(
        draft
            .select(&change(&["src/auth.rs"]), &frozen, &floor, RiskLevel::Low)
            .unwrap()
            .matched_rules,
        ["auth"]
    );
    assert!(draft
        .select(
            &change(&["src/authentic.rs"]),
            &frozen,
            &floor,
            RiskLevel::Low
        )
        .unwrap()
        .matched_rules
        .is_empty());
    frozen.complete = false;
    assert!(draft
        .select(&change(&["docs/guide.md"]), &frozen, &floor, RiskLevel::Low)
        .is_err());
    frozen.complete = true;
    frozen.components.clear();
    assert!(draft
        .select(&change(&["docs/guide.md"]), &frozen, &floor, RiskLevel::Low)
        .is_err());
    frozen
        .components
        .insert("component:auth".into(), Vec::new());
    assert!(draft
        .select(&change(&["docs/guide.md"]), &frozen, &floor, RiskLevel::Low)
        .is_err());
    frozen
        .components
        .insert("component:auth".into(), vec!["src/auth.rs".into(); 4096]);
    assert!(draft
        .select(&change(&["docs/guide.md"]), &frozen, &floor, RiskLevel::Low)
        .is_err());
}

#[test]
fn policy_digest_is_versioned_and_parsing_does_not_activate() {
    let draft = policy();
    let digest = draft.digest();
    let mut newer = draft.clone();
    newer.version += 1;
    assert_ne!(digest, newer.digest());
    let selected = select(&draft, &change(&["src/lib.rs"]));
    let json = serde_json::to_value(&selected).unwrap();
    for field in [
        "status",
        "ready",
        "approved",
        "activated",
        "actor",
        "executed",
    ] {
        assert!(json.get(field).is_none(), "{field}");
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".wcode")).unwrap();
    let project = ProjectDesign {
        schema_version: 1,
        name: "demo".into(),
        description: String::new(),
        acceptance_policy: Some(draft),
    };
    std::fs::write(
        dir.path().join(".wcode/project.yaml"),
        serde_yaml::to_string(&project).unwrap(),
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let load = load_design(&workspace).unwrap();
    assert_eq!(load.error_count(), 0, "{:?}", load.diagnostics);
    let loaded = load.state.project.unwrap().acceptance_policy.unwrap();
    assert_eq!(loaded.digest(), digest);

    let mut invalid = project;
    invalid
        .acceptance_policy
        .as_mut()
        .unwrap()
        .rules
        .push(AcceptanceRule {
            id: "unknown-design".into(),
            when: PolicySelector {
                components: vec!["component:missing".into()],
                ..Default::default()
            },
            require: requirements(&[], PolicyLevel::Quick),
        });
    std::fs::write(
        dir.path().join(".wcode/project.yaml"),
        serde_yaml::to_string(&invalid).unwrap(),
    )
    .unwrap();
    let load = load_design(&workspace).unwrap();
    assert!(load
        .diagnostics
        .iter()
        .any(|item| item.path == ".wcode/project.yaml" && item.code == "unknown-policy-component"));
}
