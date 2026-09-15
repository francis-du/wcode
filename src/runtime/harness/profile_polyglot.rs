use super::*;

pub(super) fn add_polyglot_checks(
    root: &Path,
    project_types: &[String],
    checks: &mut Vec<CheckSpec>,
) {
    let has_type = |name: &str| {
        project_types
            .iter()
            .any(|project_type| project_type == name)
    };

    if has_type("dart") || has_type("flutter") {
        push_check(
            checks,
            "dart-format",
            "quick",
            "dart",
            &["format", "-o", "none", "--set-exit-if-changed", "."],
            "Verify Dart formatting without modifying source.",
        );
        if root.join("pubspec.yaml").is_file() {
            if has_type("flutter") {
                push_check(
                    checks,
                    "flutter-analyze",
                    "quick",
                    "flutter",
                    &["analyze", "--no-pub"],
                    "Run Flutter static analysis for the owning application.",
                );
                push_check(
                    checks,
                    "flutter-test",
                    "full",
                    "flutter",
                    &["test", "--no-pub"],
                    "Run the Flutter test suite for the owning application.",
                );
            } else {
                push_check(
                    checks,
                    "dart-analyze",
                    "quick",
                    "dart",
                    &["analyze"],
                    "Run Dart static analysis for the owning package.",
                );
                push_check(
                    checks,
                    "dart-test",
                    "full",
                    "dart",
                    &["test"],
                    "Run the Dart test suite for the owning package.",
                );
            }
        }
    }

    if has_type("deno") {
        push_check(
            checks,
            "deno-format",
            "quick",
            "deno",
            &["fmt", "--check"],
            "Verify Deno formatting without modifying source.",
        );
        push_check(
            checks,
            "deno-lint",
            "quick",
            "deno",
            &["lint"],
            "Run Deno lint for the owning project.",
        );
        push_check(
            checks,
            "deno-check",
            "quick",
            "deno",
            &["check", "--frozen", "."],
            "Type-check the owning Deno project with the locked dependency graph.",
        );
        push_check(
            checks,
            "deno-test",
            "full",
            "deno",
            &["test", "--frozen"],
            "Run the Deno test suite with the locked dependency graph.",
        );
    }

    if has_type("r") {
        let description = read_small_text(&root.join("DESCRIPTION"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        if description.contains("styler") {
            push_check(
                checks,
                "r-format",
                "quick",
                "Rscript",
                &["--vanilla", "-e", "styler::style_pkg(dry=\"fail\")"],
                "Verify R package formatting with styler without modifying source.",
            );
        }
        if description.contains("lintr") || root.join(".lintr").is_file() {
            push_check(
                checks,
                "r-lint",
                "quick",
                "Rscript",
                &[
                    "--vanilla",
                    "-e",
                    "quit(status=if(length(lintr::lint_package()))1 else 0)",
                ],
                "Run lintr for the owning R package.",
            );
        }
        if description.contains("testthat") || root.join("tests/testthat").is_dir() {
            push_check(
                checks,
                "r-test",
                "full",
                "Rscript",
                &["--vanilla", "-e", "testthat::test_local()"],
                "Run testthat for the owning R package.",
            );
        }
    }

    if project_types.iter().any(|project_type| {
        matches!(
            project_type.as_str(),
            "dotnet" | "dotnet-csharp" | "dotnet-fsharp" | "dotnet-vb"
        )
    }) {
        push_check(
            checks,
            "dotnet-format",
            "quick",
            "dotnet",
            &["format", "--verify-no-changes", "--no-restore"],
            "Verify .NET formatting without restoring packages or modifying source.",
        );
        push_check(
            checks,
            "dotnet-build",
            "quick",
            "dotnet",
            &["build", "--no-restore"],
            "Compile the owning .NET solution/project with existing restored dependencies.",
        );
        push_check(
            checks,
            "dotnet-test",
            "full",
            "dotnet",
            &["test", "--no-restore"],
            "Run the .NET test suite with existing restored dependencies.",
        );
    }
}
