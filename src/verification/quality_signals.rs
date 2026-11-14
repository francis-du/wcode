use super::*;

impl RepoSignals {
    pub(crate) fn load(workspace: &Workspace) -> Self {
        let paths = [
            "Cargo.toml",
            "Cargo.lock",
            "Makefile",
            "package.json",
            "deno.json",
            "deno.jsonc",
            "pyproject.toml",
            "ruff.toml",
            ".ruff.toml",
            "mypy.ini",
            ".mypy.ini",
            "setup.cfg",
            "tox.ini",
            "pytest.ini",
            "requirements.txt",
            "go.mod",
            "pubspec.yaml",
            "mix.exs",
            "Gemfile",
            "composer.json",
            "dune-project",
            "DESCRIPTION",
            "Package.swift",
            "pom.xml",
            "build.gradle",
            "build.gradle.kts",
            "tsconfig.json",
            ".editorconfig",
            ".shellcheckrc",
            ".clang-format",
            ".clang-tidy",
            ".rubocop.yml",
            ".rubocop.yaml",
            ".luacheckrc",
            ".stylua.toml",
            ".ocamlformat",
            ".lintr",
            "phpstan.neon",
            "phpstan.neon.dist",
            "psalm.xml",
            ".swift-format",
            "biome.json",
            "biome.jsonc",
            "eslint.config.js",
            "eslint.config.mjs",
            ".eslintrc",
            ".eslintrc.json",
        ];
        let mut text = BTreeMap::new();
        for path in paths {
            if let Some(content) = read_small(workspace.root().join(path).as_path()) {
                text.insert(path, content);
            }
        }
        let package = text
            .get("package.json")
            .and_then(|content| serde_json::from_str(content).ok())
            .unwrap_or_default();
        Self { text, package }
    }

    pub(crate) fn has(&self, path: &str) -> bool {
        self.text.contains_key(path)
    }

    pub(crate) fn contains(&self, path: &str, needle: &str) -> bool {
        self.text.get(path).is_some_and(|content| {
            content
                .to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase())
        })
    }

    pub(crate) fn any_contains(&self, paths: &[&str], needle: &str) -> bool {
        paths.iter().any(|path| self.contains(path, needle))
    }

    pub(crate) fn package_dependency(&self, name: &str) -> bool {
        [
            "dependencies",
            "devDependencies",
            "peerDependencies",
            "optionalDependencies",
        ]
        .into_iter()
        .any(|section| {
            self.package
                .get(section)
                .and_then(Value::as_object)
                .is_some_and(|dependencies| dependencies.contains_key(name))
        })
    }

    pub(crate) fn package_script(&self, name: &str) -> bool {
        self.package
            .get("scripts")
            .and_then(Value::as_object)
            .is_some_and(|scripts| scripts.contains_key(name))
    }
}
