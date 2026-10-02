use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use toml_edit::{DocumentMut, Item};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn document(path: &Path) -> Result<DocumentMut, String> {
    fs::read_to_string(path)
        .map_err(|error| error.to_string())?
        .parse()
        .map_err(|error: toml_edit::TomlError| error.to_string())
}

fn names(item: &Item) -> BTreeSet<String> {
    item.as_array()
        .into_iter()
        .flat_map(|array| array.iter())
        .map(|value| {
            value
                .as_str()
                .expect("architecture names must be strings")
                .to_owned()
        })
        .collect()
}

fn dependencies(doc: &DocumentMut) -> Vec<(&str, &str, &Item)> {
    let mut result = Vec::new();
    for kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = doc.get(kind).and_then(Item::as_table_like) {
            result.extend(table.iter().map(|(name, item)| (kind, name, item)));
        }
        if let Some(targets) = doc.get("target").and_then(Item::as_table_like) {
            for (_, target) in targets.iter() {
                if let Some(table) = target.get(kind).and_then(Item::as_table_like) {
                    result.extend(table.iter().map(|(name, item)| (kind, name, item)));
                }
            }
        }
    }
    result
}

fn value<'a>(item: &'a Item, name: &str) -> Option<&'a str> {
    item.get(name).and_then(Item::as_str)
}

fn check_entry(checkout: &Path, base: &Path, entry: &str) -> Result<(), String> {
    let checkout = checkout.canonicalize().map_err(|error| error.to_string())?;
    let path = base
        .join(entry)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !path.starts_with(&checkout) || !path.is_file() {
        return Err(format!("non-OSS compile target: {}", path.display()));
    }
    if path.extension().is_some_and(|extension| extension == "rs") {
        source_boundary(&fs::read_to_string(path).map_err(|error| error.to_string())?)?;
    }
    Ok(())
}

fn check_oss_graph(
    checkout: &Path,
    manifest: &Path,
    policy: &DocumentMut,
    seen: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let manifest = manifest.canonicalize().map_err(|error| error.to_string())?;
    let checkout = checkout.canonicalize().map_err(|error| error.to_string())?;
    if !manifest.starts_with(&checkout) {
        return Err(format!(
            "OSS dependency leaves OSS source: {}",
            manifest.display()
        ));
    }
    if !seen.insert(manifest.clone()) {
        return Ok(());
    }
    let doc = document(&manifest)?;
    if doc.get("patch").is_some() || doc.get("replace").is_some() {
        return Err("unreviewed dependency source override".into());
    }
    let base = manifest.parent().unwrap();
    for directory in ["src", "tests", "examples", "benches"] {
        if base.join(directory).is_dir() {
            let mut source_files = Vec::new();
            walk(&base.join(directory), &mut source_files);
            for path in source_files {
                if path.extension().is_some_and(|extension| extension == "rs") {
                    source_boundary(&fs::read_to_string(path).map_err(|error| error.to_string())?)?;
                }
            }
        }
    }
    if let Some(build) = doc["package"].get("build") {
        if let Some(path) = build.as_str() {
            check_entry(&checkout, base, path)?;
        } else if build.as_bool() == Some(true) {
            check_entry(&checkout, base, "build.rs")?;
        }
    } else if base.join("build.rs").exists() {
        check_entry(&checkout, base, "build.rs")?;
    }
    if let Some(path) = doc
        .get("lib")
        .and_then(|lib| lib.get("path"))
        .and_then(Item::as_str)
    {
        check_entry(&checkout, base, path)?;
    }
    for kind in ["bin", "example", "test", "bench"] {
        if let Some(targets) = doc.get(kind).and_then(Item::as_array_of_tables) {
            for target in targets {
                if let Some(path) = target.get("path").and_then(Item::as_str) {
                    check_entry(&checkout, base, path)?;
                }
            }
        }
    }
    let forbidden = names(&policy["commercial_packages"]);
    let package = doc["package"]["name"]
        .as_str()
        .ok_or("missing package name")?;
    if forbidden.contains(package) || doc["package"]["license"].as_str() != Some("Apache-2.0") {
        return Err(format!("non-OSS package: {package}"));
    }
    let forbidden_features = names(&policy["forbidden_oss_features"]);
    if let Some(features) = doc.get("features").and_then(Item::as_table_like) {
        for (name, _) in features.iter() {
            if forbidden_features
                .iter()
                .any(|banned| name == banned || name.starts_with(&format!("{banned}-")))
            {
                return Err(format!("commercial OSS feature: {name}"));
            }
        }
    }
    let workspace_doc = document(&checkout.join("Cargo.toml"))?;
    for (_, alias, original) in dependencies(&doc) {
        let inherited = original.get("workspace").and_then(Item::as_bool) == Some(true);
        let item = if inherited {
            workspace_doc["workspace"]["dependencies"]
                .get(alias)
                .ok_or_else(|| format!("missing workspace dependency: {alias}"))?
        } else {
            original
        };
        let dependency = value(item, "package").unwrap_or(alias);
        if forbidden.contains(dependency) {
            return Err(format!("commercial dependency: {dependency}"));
        }
        if item.get("git").is_some() || item.get("registry").is_some() {
            return Err(format!("unreviewed external source: {dependency}"));
        }
        if let Some(path) = value(item, "path") {
            let base = if inherited {
                checkout.as_path()
            } else {
                manifest.parent().unwrap()
            };
            check_oss_graph(&checkout, &base.join(path).join("Cargo.toml"), policy, seen)?;
        }
    }
    Ok(())
}

fn walk(path: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        assert!(
            !kind.is_symlink(),
            "source symlink: {}",
            entry.path().display()
        );
        if kind.is_dir() {
            walk(&entry.path(), files);
        } else if kind.is_file() {
            files.push(entry.path());
        }
    }
}

fn source_boundary(source: &str) -> Result<(), String> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).ok_or("Rust parse failed")?;
    if tree.root_node().has_error() {
        return Err("invalid Rust source".into());
    }
    let include_pattern = regex::Regex::new(
        r#"^(?:(?:r#)?[A-Za-z_]\w*\s*::\s*)*(?:r#)?include(?:_str|_bytes)?\s*!\s*\(\s*"[^"\\]*"\s*\)$"#,
    )
    .unwrap();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        let text = node
            .utf8_text(source.as_bytes())
            .map_err(|error| error.to_string())?;
        if matches!(node.kind(), "identifier" | "type_identifier")
            && matches!(
                text.trim_start_matches("r#"),
                "wcode_team" | "wcode_enterprise"
            )
        {
            return Err(format!("commercial crate reference: {text}"));
        }

        // Macro templates keep their invocations in token trees, not expanded
        // Rust AST nodes. Validate the same literal input contract there.
        if node.kind() == "token_tree" {
            let mut cursor = node.walk();
            let children = node.children(&mut cursor).collect::<Vec<_>>();
            for tokens in children.windows(3) {
                let name = tokens[0]
                    .utf8_text(source.as_bytes())
                    .map_err(|error| error.to_string())?;
                if matches!(
                    name.trim_start_matches("r#"),
                    "include" | "include_str" | "include_bytes"
                ) && tokens[1].utf8_text(source.as_bytes()).ok() == Some("!")
                    && tokens[2].kind() == "token_tree"
                {
                    let input = tokens[2]
                        .utf8_text(source.as_bytes())
                        .map_err(|error| error.to_string())?;
                    let invocation = format!("{name}!{input}");
                    let normalized = input.to_ascii_lowercase();
                    if !include_pattern.is_match(&invocation)
                        || normalized.contains("commercial/")
                        || normalized.contains("/commercial")
                    {
                        return Err(
                            "macro template compile input violates OSS source contract".into()
                        );
                    }
                }
            }
        }
        let include = node.kind() == "macro_invocation"
            && text
                .split('!')
                .next()
                .unwrap_or("")
                .trim()
                .rsplit("::")
                .next()
                .is_some_and(|name| {
                    matches!(
                        name.trim().trim_start_matches("r#"),
                        "include" | "include_str" | "include_bytes"
                    )
                });
        if include && !include_pattern.is_match(text) {
            return Err("dynamic compile input is not an explicit OSS source contract".into());
        }
        if matches!(node.kind(), "attribute_item" | "macro_invocation") {
            let compact = text
                .to_ascii_lowercase()
                .replace('\\', "/")
                .replace(char::is_whitespace, "");
            let path_attribute = node.kind() == "attribute_item" && compact.contains("path=");
            if path_attribute && text.contains('\\') {
                return Err("escaped compile path is not an explicit OSS source contract".into());
            }
            if (include || path_attribute)
                && (compact.contains("commercial/") || compact.contains("/commercial"))
            {
                return Err("commercial source inclusion".into());
            }
            for feature in ["team", "enterprise", "commercial", "cloud", "paid"] {
                if compact.contains(&format!("feature=\"{feature}\"")) {
                    return Err(format!("commercial compile feature: {feature}"));
                }
            }
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    Ok(())
}

#[test]
fn oss_dependency_graph_has_no_commercial_edges() {
    let checkout = root();
    let policy = document(&checkout.join(".wcode/architecture.toml")).unwrap();
    assert_eq!(policy["schema_version"].as_integer(), Some(1));
    check_oss_graph(
        &checkout,
        &checkout.join("Cargo.toml"),
        &policy,
        &mut BTreeSet::new(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--locked",
            "--offline",
            "--no-deps",
            "--format-version",
            "1",
        ])
        .current_dir(&checkout)
        .output()
        .unwrap();
    assert!(output.status.success(), "actual OSS cargo metadata failed");
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let forbidden = names(&policy["commercial_packages"]);
    let declared = names(&policy["oss_packages"]);
    let actual = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|package| package["name"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, declared);
    assert_eq!(
        metadata["workspace_members"].as_array().unwrap().len(),
        declared.len()
    );
    for package in metadata["packages"].as_array().unwrap() {
        assert!(!forbidden.contains(package["name"].as_str().unwrap()));
        assert_eq!(package["license"], "Apache-2.0");
        for target in package["targets"].as_array().unwrap() {
            check_entry(&checkout, &checkout, target["src_path"].as_str().unwrap()).unwrap();
        }
        for dependency in package["dependencies"].as_array().unwrap() {
            assert!(!forbidden.contains(dependency["name"].as_str().unwrap()));
        }
    }
}

#[test]
fn oss_source_and_logical_ownership_stay_independent() {
    let checkout = root();
    let policy = document(&checkout.join(".wcode/architecture.toml")).unwrap();
    let mut prefixes = Vec::new();
    for layer in ["core", "oss_product"] {
        for field in ["roots", "shared_runtime"] {
            if let Some(array) = policy["ownership"][layer]
                .get(field)
                .and_then(Item::as_array)
            {
                prefixes.extend(array.iter().map(|value| value.as_str().unwrap()));
            }
        }
    }
    let mut files = Vec::new();
    walk(&checkout.join("src"), &mut files);
    for path in files {
        let relative = path
            .strip_prefix(&checkout)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        assert!(
            prefixes.iter().any(|prefix| relative.starts_with(prefix)),
            "unowned OSS source: {relative}"
        );
        if path.extension().is_some_and(|extension| extension == "rs") {
            source_boundary(&fs::read_to_string(&path).unwrap())
                .unwrap_or_else(|error| panic!("{relative}: {error}"));
        }
    }
}

fn check_package_source(checkout: &Path, entry: &str) -> Result<(), String> {
    let checkout = checkout.canonicalize().map_err(|error| error.to_string())?;
    let source = checkout.join(entry);
    if source.exists() || source.is_symlink() {
        let canonical = source.canonicalize().map_err(|error| error.to_string())?;
        if !canonical.starts_with(&checkout) {
            return Err("package source escapes OSS ownership".into());
        }
    } else if !matches!(entry, "Cargo.toml.orig" | ".cargo_vcs_info.json") {
        return Err("package input is missing from the source checkout".into());
    }
    Ok(())
}

#[test]
fn oss_package_and_license_exclude_commercial_source() {
    let checkout = root();
    let manifest = document(&checkout.join("Cargo.toml")).unwrap();
    assert_eq!(manifest["package"]["license"].as_str(), Some("Apache-2.0"));
    let commercial = checkout.join("commercial");
    if commercial.exists() {
        let mut files = Vec::new();
        walk(&commercial, &mut files);
        let source_files = files
            .iter()
            .filter_map(|path| path.strip_prefix(&commercial).ok())
            .filter(|path| !path.starts_with("target"))
            .collect::<Vec<_>>();
        assert!(
            source_files.is_empty(),
            "commercial source files must live in a separate repository, never inside OSS wcode: {source_files:?}"
        );
    }
    let included = names(&manifest["package"]["include"]);
    assert!(!included.is_empty() && included.iter().all(|path| !path.contains("commercial")));
    let license = fs::read_to_string(checkout.join("LICENSE")).unwrap();
    assert!(license.contains("Apache License") && license.contains("Version 2.0, January 2004"));
    let output = Command::new(env!("CARGO"))
        .args([
            "package",
            "--list",
            "--locked",
            "--offline",
            "--allow-dirty",
        ])
        .current_dir(&checkout)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "actual OSS cargo package list failed (exit {:?}): {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let paths = String::from_utf8(output.stdout).unwrap();
    assert!(paths.lines().any(|path| path == "src/lib.rs"));
    for path in paths.lines() {
        check_package_source(&checkout, path).unwrap_or_else(|error| panic!("{path}: {error}"));
        assert!(
            !path
                .replace('\\', "/")
                .split('/')
                .any(|part| part == "commercial"),
            "proprietary package input: {path}"
        );
    }
    let release = fs::read_to_string(checkout.join(".github/workflows/release.yml")).unwrap();
    assert!(release.contains("set(found) == {expected_binary, \"README.md\", \"LICENSE\"}"));
}

fn fixture(checkout: &Path, relative: &str, body: &str) {
    let path = checkout.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

#[test]
fn compile_target_ownership_compares_canonical_checkout_and_source() {
    let temp = tempfile::tempdir().unwrap();
    let checkout = temp.path().join("oss");
    fixture(&checkout, "src/lib.rs", "pub fn normal() {}\n");
    fs::create_dir(checkout.join("nested")).unwrap();
    let alias = checkout.join("nested/..");
    let source = checkout.join("src/lib.rs").canonicalize().unwrap();
    check_entry(&alias, &checkout, source.to_str().unwrap()).unwrap();
    fixture(temp.path(), "external.rs", "pub fn external() {}\n");
    assert!(
        check_entry(&alias, &checkout, "../external.rs")
            .unwrap_err()
            .starts_with("non-OSS compile target:"),
        "normalizing checkout spelling must preserve the ownership boundary"
    );
}

fn fixture_policy() -> DocumentMut {
    "commercial_packages=['wcode-team','wcode-enterprise']\nforbidden_oss_features=['team','enterprise','commercial','cloud','paid']".parse().unwrap()
}

#[test]
fn reverse_dependency_alias_optional_target_build_and_transitive_paths_fail() {
    for dependency in [
        "[dependencies]\nrenamed={package='wcode-team', version='1', optional=true}",
        "[build-dependencies]\nalias={package='wcode-enterprise', version='1'}",
        "[dev-dependencies]\nalias={path='commercial/team'}",
        "[target.'cfg(windows)'.dependencies]\nalias={package='wcode-team', version='1'}",
        "[dependencies]\ninnocent={path='bridge'}",
        "[features]\nenterprise=[]",
        "[dependencies]\nprivate={git='https://example.invalid/private'}",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(
            temp.path(),
            "Cargo.toml",
            &format!("[package]\nname='wcode'\nlicense='Apache-2.0'\n{dependency}"),
        );
        fixture(temp.path(), "bridge/Cargo.toml", "[package]\nname='bridge'\nlicense='Apache-2.0'\n[dependencies]\nalias={path='../commercial/team'}");
        fixture(
            temp.path(),
            "commercial/team/Cargo.toml",
            "[package]\nname='wcode-team'\nlicense-file='../LICENSE'",
        );
        assert!(
            check_oss_graph(
                temp.path(),
                &temp.path().join("Cargo.toml"),
                &fixture_policy(),
                &mut BTreeSet::new()
            )
            .is_err(),
            "{dependency}"
        );
    }
}

#[test]
fn inherited_workspace_reverse_dependency_fails() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), "Cargo.toml", "[package]\nname='wcode'\nlicense='Apache-2.0'\n[dependencies]\nbridge={path='bridge'}\n[workspace.dependencies]\nsecret={package='wcode-team',version='1',optional=true}");
    fixture(
        temp.path(),
        "bridge/Cargo.toml",
        "[package]\nname='bridge'\nlicense='Apache-2.0'\n[dependencies]\nsecret.workspace=true",
    );
    assert!(check_oss_graph(
        temp.path(),
        &temp.path().join("Cargo.toml"),
        &fixture_policy(),
        &mut BTreeSet::new()
    )
    .is_err());
}

#[test]
fn source_include_and_feature_bypasses_fail_but_comments_do_not() {
    for source in [
        "use wcode_team::State;",
        "use wcode_enterprise as renamed;",
        "#[path = \"../commercial/team/src/lib.rs\"] mod hidden;",
        "include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/commercial/team/src/lib.rs\"));",
        "include_str!(\"../commercial/LICENSE\");",
        "r#include_str!(\"../commercial/LICENSE\");",
        "macro_rules! hidden { () => { include_str!(\"../COMMERCIAL/LICENSE\") }; } hidden!();",
        "std::r#include_str!(\"../commercial/LICENSE\");",
        "macro_rules! hidden { () => { r#include_bytes!(\"../commercial/LICENSE\") }; } hidden!();",
        "include!(concat!(\"../com\", \"mercial/team/src/lib.rs\"));",
        "include!(env!(\"PRIVATE_SOURCE\"));",
        "macro_rules! hidden { () => { include!(concat!(\"../com\", \"mercial/team/src/lib.rs\")) }; } hidden!();",
        "include ! (concat!(\"../com\", \"mercial/team/src/lib.rs\"));",
        "std::include_str ! (concat!(\"../com\", \"mercial/LICENSE\"));",
        "#[cfg(any(feature = \"enterprise\", test))] mod hidden {}",
        "#[cfg_attr(unix, path = \"../commercial/team/src/lib.rs\")] mod hidden;",
        r#"include_str!("../\x63ommercial/LICENSE");"#,
        r#"#[path="../\x63ommercial/team/src/lib.rs"] mod hidden;"#,
    ] {
        assert!(source_boundary(source).is_err(), "{source}");
    }
    source_boundary("// use wcode_team::State;\nfn normal() {}").unwrap();
    source_boundary("include_str!(\"../docs/assets/logo.svg\");").unwrap();
    source_boundary("std::r#include_str!(\"../docs/assets/logo.svg\");").unwrap();
}
#[test]
fn oss_path_dependency_cannot_escape_to_sibling_commercial_repository() {
    let temp = tempfile::tempdir().unwrap();
    let checkout = temp.path().join("wcode");
    let commercial = temp.path().join("wcode-commercial");
    fixture(
        &checkout,
        "Cargo.toml",
        "[package]\nname='wcode'\nlicense='Apache-2.0'\n[dependencies]\nteam={package='wcode-team',path='../wcode-commercial/team'}",
    );
    fixture(&checkout, "src/lib.rs", "pub fn normal() {}\n");
    fixture(
        &commercial,
        "team/Cargo.toml",
        "[package]\nname='wcode-team'\nversion='0.9.0'\nlicense='Proprietary'",
    );
    fixture(&commercial, "team/src/lib.rs", "pub fn commercial() {}\n");
    let error = check_oss_graph(
        &checkout,
        &checkout.join("Cargo.toml"),
        &fixture_policy(),
        &mut BTreeSet::new(),
    )
    .unwrap_err();
    assert!(error.contains("commercial dependency") || error.contains("leaves OSS source"));
}
#[test]
fn oss_patch_and_replace_overrides_fail_before_dependency_resolution() {
    for override_source in [
        "[patch.crates-io]\nbridge={path='commercial/bridge'}",
        "[replace]\n'bridge:1.0.0'={path='commercial/bridge'}",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(
            temp.path(),
            "Cargo.toml",
            &format!(
                "[package]\nname='wcode'\nversion='0.9.0'\nlicense='Apache-2.0'\n\
                 [dependencies]\nbridge='1.0.0'\n{override_source}"
            ),
        );
        fixture(
            temp.path(),
            "commercial/bridge/Cargo.toml",
            "[package]\nname='bridge'\nversion='1.0.0'\nlicense='Apache-2.0'",
        );
        fixture(temp.path(), "src/lib.rs", "pub fn normal() {}");
        fixture(
            temp.path(),
            "commercial/bridge/src/lib.rs",
            "pub fn hidden() {}",
        );
        let error = check_oss_graph(
            temp.path(),
            &temp.path().join("Cargo.toml"),
            &fixture_policy(),
            &mut BTreeSet::new(),
        )
        .unwrap_err();
        assert_eq!(
            error, "unreviewed dependency source override",
            "{override_source}"
        );
    }
}

#[test]
fn automatic_build_script_cannot_include_commercial_source() {
    let temp = tempfile::tempdir().unwrap();
    let manifest = "[package]\nname='wcode'\nversion='0.9.0'\nlicense='Apache-2.0'";
    fixture(temp.path(), "Cargo.toml", manifest);
    fixture(temp.path(), "src/lib.rs", "pub fn normal() {}");
    fixture(temp.path(), "commercial/LICENSE", "private-build-fixture");
    fixture(temp.path(), "build.rs", "fn main() {}");
    check_oss_graph(
        temp.path(),
        &temp.path().join("Cargo.toml"),
        &fixture_policy(),
        &mut BTreeSet::new(),
    )
    .unwrap();
    fixture(
        temp.path(),
        "build.rs",
        r#"fn main() { let _ = include_str!("commercial/LICENSE"); }"#,
    );
    let error = check_oss_graph(
        temp.path(),
        &temp.path().join("Cargo.toml"),
        &fixture_policy(),
        &mut BTreeSet::new(),
    )
    .unwrap_err();
    assert_eq!(error, "commercial source inclusion");
    // Cargo does not execute an automatic build script when explicitly disabled.
    fixture(
        temp.path(),
        "Cargo.toml",
        &format!("{manifest}\nbuild=false"),
    );
    check_oss_graph(
        temp.path(),
        &temp.path().join("Cargo.toml"),
        &fixture_policy(),
        &mut BTreeSet::new(),
    )
    .unwrap();
}

#[test]
fn explicit_external_compile_targets_fail_for_all_cargo_entry_kinds() {
    for entry in [
        "build='../wcode-commercial/team/src/lib.rs'",
        "[lib]\npath='../wcode-commercial/team/src/lib.rs'",
        "[[bin]]\nname='fixture'\npath='../wcode-commercial/team/src/lib.rs'",
        "[[example]]\nname='fixture'\npath='../wcode-commercial/team/src/lib.rs'",
        "[[test]]\nname='fixture'\npath='../wcode-commercial/team/src/lib.rs'",
        "[[bench]]\nname='fixture'\npath='../wcode-commercial/team/src/lib.rs'",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("wcode");
        fixture(
            &checkout,
            "Cargo.toml",
            &format!("[package]\nname='wcode'\nversion='0.9.0'\nlicense='Apache-2.0'\n{entry}"),
        );
        fixture(&checkout, "src/lib.rs", "pub fn normal() {}");
        fixture(
            temp.path(),
            "wcode-commercial/team/src/lib.rs",
            "pub fn hidden() {}",
        );
        let error = check_oss_graph(
            &checkout,
            &checkout.join("Cargo.toml"),
            &fixture_policy(),
            &mut BTreeSet::new(),
        )
        .unwrap_err();
        assert!(
            error.starts_with("non-OSS compile target:"),
            "{entry}: {error}"
        );
    }
}

#[test]
fn transitive_oss_dependency_cannot_include_commercial_source() {
    let temp = tempfile::tempdir().unwrap();
    fixture(
        temp.path(),
        "Cargo.toml",
        "[package]\nname='wcode'\nversion='0.9.0'\nlicense='Apache-2.0'\n\
         [dependencies]\nbridge={path='bridge'}",
    );
    fixture(
        temp.path(),
        "bridge/Cargo.toml",
        "[package]\nname='bridge'\nversion='1.0.0'\nlicense='Apache-2.0'",
    );
    fixture(temp.path(), "src/lib.rs", "pub fn normal() {}");
    fixture(temp.path(), "bridge/src/lib.rs", "pub fn bridge() {}");
    fixture(
        temp.path(),
        "commercial/NOTICE",
        "private-transitive-fixture",
    );
    check_oss_graph(
        temp.path(),
        &temp.path().join("Cargo.toml"),
        &fixture_policy(),
        &mut BTreeSet::new(),
    )
    .unwrap();
    fixture(
        temp.path(),
        "bridge/src/lib.rs",
        r#"pub fn bridge() -> &'static str { include_str!("../../commercial/NOTICE") }"#,
    );
    let error = check_oss_graph(
        temp.path(),
        &temp.path().join("Cargo.toml"),
        &fixture_policy(),
        &mut BTreeSet::new(),
    )
    .unwrap_err();
    assert_eq!(error, "commercial source inclusion");
}

#[cfg(unix)]
#[test]
fn package_assets_cannot_follow_external_symlinks() {
    let temp = tempfile::tempdir().unwrap();
    let checkout = temp.path().join("oss");
    fixture(&checkout, "docs/local.txt", "OSS asset");
    fixture(temp.path(), "outside.txt", "external fixture");
    check_package_source(&checkout, "docs/local.txt").unwrap();
    check_package_source(&checkout, "Cargo.toml.orig").unwrap();
    std::os::unix::fs::symlink("../../outside.txt", checkout.join("docs/outside.txt")).unwrap();
    assert_eq!(
        check_package_source(&checkout, "docs/outside.txt").unwrap_err(),
        "package source escapes OSS ownership"
    );
}
