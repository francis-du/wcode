use crate::workspace::Workspace;
use serde::Serialize;
use std::{collections::BTreeMap, fs};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(super) struct Identity {
    pub path: String,
    pub symbol: String,
}

impl Identity {
    pub fn new(path: &str, symbol: &str) -> Self {
        Self {
            path: path.into(),
            symbol: symbol.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Gold {
    pub identity: Identity,
    // Authored from fixture source, never from the system under test.
    pub fragment: String,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Case {
    pub id: String,
    pub language: String,
    pub category: String,
    pub query: String,
    pub files: BTreeMap<String, String>,
    pub required: Vec<Gold>,
    pub useful: Vec<Identity>,
    pub writable: bool,
    pub no_answer: bool,
}

impl Case {
    pub fn instantiate(&self) -> (tempfile::TempDir, Workspace) {
        let root = tempfile::tempdir().unwrap();
        for (path, content) in &self.files {
            let target = root.path().join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, content).unwrap();
        }
        let workspace = Workspace::new(root.path(), self.writable, false).unwrap();
        (root, workspace)
    }
}

fn add_noise(case: &mut Case, extension: &str, count: usize) {
    for index in 0..count {
        let source = match extension {
            "go" => format!("package fixture\nfunc unrelated_{index}() int {{ return {index} }}\n"),
            "ts" => format!("export function unrelated_{index}(): number {{ return {index}; }}\n"),
            "py" => format!("def unrelated_{index}():\n    return {index}\n"),
            _ => format!("pub fn unrelated_{index}() -> usize {{ {index} }}\n"),
        };
        case.files
            .insert(format!("src/noise_{index:04}.{extension}"), source);
    }
}

pub(super) fn base_case(language: &str) -> Case {
    let (extension, manifest, config, guard, refresh, observe) = match language {
        "go" => (
            "go", "go.mod", "module fitness_fixture\n\ngo 1.20\n",
            "func cleanup_if_owner(old uint64, current uint64) bool {\n    return old == current\n}\n",
            "func refresh_session(old uint64, current uint64) bool {\n    return cleanup_if_owner(old, current)\n}\n",
            "func observe_epoch(epoch uint64) bool {\n    return refresh_session(epoch, epoch)\n}\n",
        ),
        "typescript" => (
            "ts", "package.json", "{\"name\":\"fitness-fixture\",\"private\":true}\n",
            "export function cleanup_if_owner(old: number, current: number): boolean {\n    return old === current;\n}\n",
            "export function refresh_session(old: number, current: number): boolean {\n    return cleanup_if_owner(old, current);\n}\n",
            "export function observe_epoch(epoch: number): boolean {\n    return refresh_session(epoch, epoch);\n}\n",
        ),
        "python" => (
            "py", "pyproject.toml", "[project]\nname='fitness-fixture'\nversion='0.1.0'\n",
            "def cleanup_if_owner(old, current):\n    return old == current\n",
            "def refresh_session(old, current):\n    return cleanup_if_owner(old, current)\n",
            "def observe_epoch(epoch):\n    return refresh_session(epoch, epoch)\n",
        ),
        "rust" => (
            "rs", "Cargo.toml", "[package]\nname='fitness-fixture'\nversion='0.1.0'\nedition='2021'\n",
            "pub fn cleanup_if_owner(old: u64, current: u64) -> bool {\n    old == current\n}\n",
            "pub fn refresh_session(old: u64, current: u64) -> bool {\n    cleanup_if_owner(old, current)\n}\n",
            "pub fn observe_epoch(epoch: u64) -> bool {\n    refresh_session(epoch, epoch)\n}\n",
        ),
        _ => panic!("unknown fixture language"),
    };
    let path = format!("src/session.{extension}");
    let mut files = BTreeMap::from([(manifest.into(), config.into())]);
    let prefix = if language == "go" {
        "package fixture\n\n"
    } else {
        ""
    };
    files.insert(
        path.clone(),
        format!("{prefix}{guard}\n{refresh}\n{observe}"),
    );
    if language == "rust" {
        files.insert(
            "src/lib.rs".into(),
            "mod session;\npub use session::*;\n".into(),
        );
        files.insert("tests/session.rs".into(), "use fitness_fixture::cleanup_if_owner;\n#[test]\nfn replacement_keeps_new_owner() {\n    assert!(!cleanup_if_owner(1, 2));\n    assert!(cleanup_if_owner(2, 2));\n}\n".into());
    }
    let mut case = Case {
        id: format!("{language}-exact-pair"),
        language: language.into(),
        category: "explicit-symbols".into(),
        query: "inspect cleanup_if_owner refresh_session".into(),
        files,
        required: vec![
            Gold {
                identity: Identity::new(&path, "cleanup_if_owner"),
                fragment: guard.into(),
            },
            Gold {
                identity: Identity::new(&path, "refresh_session"),
                fragment: refresh.into(),
            },
        ],
        useful: vec![Identity::new(&path, "observe_epoch")],
        writable: true,
        no_answer: false,
    };
    add_noise(&mut case, extension, 24);
    case
}

fn language_variants(exact: &Case) -> Vec<Case> {
    let language = exact.language.as_str();
    let path = exact.required[0].identity.path.clone();
    let mut variants = Vec::new();
    for (suffix, category, query, required) in [
        (
            "cleanup-only",
            "single-target",
            "inspect cleanup_if_owner",
            vec![0],
        ),
        (
            "refresh-only",
            "single-target",
            "inspect refresh_session",
            vec![1],
        ),
        (
            "ownership-natural",
            "natural-language",
            "Locate the stale ownership guard used before cleanup",
            vec![0],
        ),
        (
            "refresh-natural",
            "natural-language",
            "Locate the session refresh path that invokes the ownership cleanup check",
            vec![1],
        ),
        (
            "call-chain",
            "relationship-context",
            "Trace observe_epoch through refresh_session to cleanup_if_owner",
            vec![0, 1],
        ),
        (
            "reverse-explicit",
            "explicit-symbols",
            "inspect refresh_session cleanup_if_owner",
            vec![0, 1],
        ),
        (
            "path-qualified-cleanup",
            "path-anchor",
            "inspect the cleanup_if_owner definition in the session source",
            vec![0],
        ),
        (
            "impact-context",
            "impact-context",
            "What session source is affected if cleanup_if_owner changes?",
            vec![0, 1],
        ),
    ] {
        let mut case = exact.clone();
        case.id = format!("{language}-{suffix}");
        case.category = category.into();
        case.query = query.into();
        case.required = required
            .into_iter()
            .map(|index| exact.required[index].clone())
            .collect();
        case.useful = vec![Identity::new(&path, "observe_epoch")];
        variants.push(case);
    }
    variants
}

fn rust_mutation_variants(base: &Case) -> Vec<Case> {
    let replacements = [
        ("self-owner", "old == old"),
        ("always-false", "false"),
        ("inverted-owner", "old != current"),
        ("zero-owner", "old == 0"),
        ("current-zero", "current == 0"),
        ("ordered-owner", "old <= current"),
        ("strict-order", "old < current"),
        ("bit-owner", "old & 1 == current & 1"),
        ("offset-owner", "old.wrapping_add(1) == current"),
    ];
    replacements
        .into_iter()
        .map(|(suffix, replacement)| {
            let mut case = base.clone();
            case.id = format!("rust-mutation-{suffix}");
            case.category = "bug-relevant-evidence".into();
            case.query =
                "Review cleanup_if_owner and its caller for stale ownership cleanup behavior"
                    .into();
            let text = case.files.get_mut("src/session.rs").unwrap();
            *text = text.replace("old == current", replacement);
            case.required[0].fragment = case.required[0]
                .fragment
                .replace("old == current", replacement);
            case
        })
        .collect()
}

pub(super) fn corpus() -> Vec<Case> {
    let mut cases = Vec::new();
    for language in ["rust", "go", "typescript", "python"] {
        let exact = base_case(language);
        let mut narrative = exact.clone();
        narrative.id = format!("{language}-symptom-only");
        narrative.category = "natural-language".into();
        narrative.query = "Find the ownership check that prevents an old session from cleaning up its replacement".into();
        let mut callers = exact.clone();
        callers.id = format!("{language}-caller-context");
        callers.category = "relationship-context".into();
        callers.query = "Find callers of cleanup_if_owner and provide the relevant source".into();
        let variants = language_variants(&exact);
        cases.extend([exact, narrative, callers]);
        cases.extend(variants);
    }
    let mut duplicate = base_case("rust");
    duplicate.id = "rust-same-name-path-anchor".into();
    duplicate.category = "identity-disambiguation".into();
    duplicate.query = "src/session.rs:1".into();
    duplicate.required.truncate(1);
    duplicate.files.insert(
        "src/other.rs".into(),
        "pub fn cleanup_if_owner() -> bool { false }\n".into(),
    );
    cases.push(duplicate);

    let mut four = base_case("rust");
    four.id = "rust-four-explicit-targets".into();
    four.query = "feature_entry batch_worker parse_request finish_job".into();
    four.required.clear();
    four.useful.clear();
    for name in four.query.split_whitespace() {
        let path = format!("src/{name}.rs");
        let source = format!("pub fn {name}() -> usize {{\n    42\n}}\n");
        four.files.insert(path.clone(), source.clone());
        four.required.push(Gold {
            identity: Identity::new(&path, name),
            fragment: source,
        });
    }
    cases.push(four);

    let mut unicode = base_case("rust");
    unicode.id = "rust-unicode-crlf-anchor".into();
    unicode.category = "source-fidelity".into();
    unicode.query = "修复：src/源码.rs:2，保留原文".into();
    let source = "pub fn unicode_guard() -> bool {\r\n    let message = \"你好🚀\";\r\n    !message.is_empty()\r\n}\r\n";
    unicode
        .files
        .insert("src/源码.rs".into(), format!("// 原文\r\n{source}"));
    unicode.required = vec![Gold {
        identity: Identity::new("src/源码.rs", "unicode_guard"),
        fragment: source.into(),
    }];
    unicode.useful.clear();
    cases.push(unicode);

    let mut missing = base_case("rust");
    missing.id = "rust-missing-anchor".into();
    missing.category = "no-answer".into();
    missing.query = "src/does_not_exist.rs:7".into();
    missing.required.clear();
    missing.useful.clear();
    missing.no_answer = true;
    cases.push(missing);

    let mut readonly = base_case("rust");
    readonly.id = "rust-read-only".into();
    readonly.category = "read-only".into();
    readonly.writable = false;
    cases.push(readonly);

    let mut mutation = base_case("rust");
    mutation.id = "rust-removed-ownership-guard".into();
    mutation.category = "bug-relevant-evidence".into();
    mutation.query =
        "Review cleanup_if_owner and refresh_session for stale ownership cleanup".into();
    let text = mutation.files.get_mut("src/session.rs").unwrap();
    *text = text.replace("old == current", "true");
    mutation.required[0].fragment = mutation.required[0]
        .fragment
        .replace("old == current", "true");
    cases.push(mutation);
    cases.extend(rust_mutation_variants(&base_case("rust")));

    let mut large = base_case("rust");
    large.id = "rust-640-distractors".into();
    large.category = "scan-pressure".into();
    large.query = "Find callers of cleanup_if_owner and refresh_session".into();
    add_noise(&mut large, "rs", 640);
    cases.push(large);
    cases
}
