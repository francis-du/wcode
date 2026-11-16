use super::*;
use std::fs;

#[test]
fn option_like_repository_paths_are_refused_as_tool_arguments() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".clang-tidy"), "Checks: '*'\n").unwrap();
    fs::write(dir.path().join("compile_commands.json"), "[]\n").unwrap();
    fs::write(
        dir.path().join("-load=evil.c"),
        "int main() { return 0; }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let signals = RepoSignals::load(&workspace);

    // A file name that starts with `-` would be parsed by the tool as an
    // option, so clang-tidy could load it as a plugin or use it as its driver.
    let error = crate::quality_catalog::candidates_for(
        &workspace,
        &signals,
        SemanticLanguage::Cpp,
        &["-load=evil.c".into()],
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("-load=evil.c"), "{error}");

    assert!(
        crate::quality_catalog::candidates_for(
            &workspace,
            &signals,
            SemanticLanguage::Cpp,
            &["main.cpp".into()],
        )
        .is_ok(),
        "ordinary file names must still produce candidates"
    );
}
