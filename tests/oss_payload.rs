use std::{collections::BTreeSet, fs, process::Command};

fn cargo_file_paths(output: &str, separator: char) -> BTreeSet<String> {
    output
        .lines()
        .map(|line| line.replace(separator, "/"))
        .collect()
}

#[test]
fn cargo_list_paths_follow_the_native_separator_contract() {
    let expected = BTreeSet::from(["src/lib.rs".to_owned(), "tests/fixture name.py".to_owned()]);
    assert_eq!(
        cargo_file_paths("src/lib.rs\ntests/fixture name.py\n", '/'),
        expected
    );
    assert_eq!(
        cargo_file_paths("src\\lib.rs\r\ntests\\fixture name.py\r\n", '\\'),
        expected
    );
    assert_eq!(
        cargo_file_paths("tests/literal\\name.py\n", '/'),
        BTreeSet::from(["tests/literal\\name.py".to_owned()])
    );
}

#[test]
fn source_payload_excludes_python_bytecode_but_keeps_adjacent_sources() {
    let manifest: toml_edit::DocumentMut = include_str!("../Cargo.toml")
        .parse()
        .expect("valid manifest");
    let include = manifest["package"]["include"]
        .as_array()
        .expect("explicit source package contract");
    let fixture = tempfile::tempdir().unwrap();
    fs::write(
        fixture.path().join("Cargo.toml"),
        format!(
            "[package]\nname = \"source-payload-contract\"\nversion = \"0.0.0\"\nedition = \"2021\"\ninclude = {include}\n"
        ),
    )
    .unwrap();
    let sources = [
        "src/lib.rs",
        "src/helper.py",
        "tests/unit/cache_contract.py",
        "tests/unit/cache_contract.cjs",
        "tests/fixtures/byte-pattern.bin",
        "tests/fixtures/snap.pycase",
        ".wcode/design/cache-contract.yaml",
    ];
    let caches = [
        "tests/__pycache__/cache_contract.cpython-312.pyc",
        "tests/unit/__pycache__/cache_contract.cpython-315.pyc",
        "tests/unit/cache_contract.pyc",
        "tests/unit/cache_contract.pyo",
        "src/__pycache__/unused.cache",
    ];
    for path in sources.iter().chain(caches.iter()) {
        let destination = fixture.path().join(path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, b"// synthetic package contract fixture\n").unwrap();
    }
    let output = Command::new("cargo")
        .args(["package", "--list", "--offline", "--allow-dirty"])
        .env("CARGO_TARGET_DIR", fixture.path().join("target"))
        .current_dir(fixture.path())
        .output()
        .expect("real Cargo is required for source payload selection");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");
    let files = cargo_file_paths(&stdout, std::path::MAIN_SEPARATOR);
    for path in sources {
        assert!(files.contains(path), "source missing: {path}\n{stdout}");
    }
    for path in caches {
        assert!(!files.contains(path), "generated cache included: {path}");
    }
}
