use super::*;

#[test]
fn pytest_is_selected_only_from_project_evidence() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("pyproject.toml"),
        "[project]\nname='demo'\n[project.optional-dependencies]\ntest=['pytest>=8']\n",
    )
    .unwrap();
    let mut checks = Vec::new();
    add_python_checks(root.path(), &mut checks);

    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].id, "python-tests");
    assert_eq!(checks[0].program, "pytest");
    assert_eq!(checks[0].args, ["-q"]);
}

#[test]
fn stdlib_unittest_is_selected_without_requiring_pytest() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("tests")).unwrap();
    fs::write(
        root.path().join("tests/test_answer.py"),
        "import unittest\n\nclass AnswerTest(unittest.TestCase):\n    def test_answer(self):\n        self.assertEqual(2, 2)\n",
    )
    .unwrap();
    let mut checks = Vec::new();
    add_python_checks(root.path(), &mut checks);

    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].id, "python-unittest");
    assert_eq!(checks[0].program, "python3");
    assert_eq!(checks[0].args, ["-m", "unittest", "discover"]);
}

#[test]
fn unknown_python_test_style_does_not_claim_deterministic_coverage() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("tests")).unwrap();
    fs::write(
        root.path().join("tests/test_answer.py"),
        "def test_answer():\n    assert 2 == 2\n",
    )
    .unwrap();
    let mut checks = Vec::new();
    add_python_checks(root.path(), &mut checks);

    assert!(checks.is_empty());
}
