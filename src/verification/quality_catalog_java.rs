use super::*;

pub(super) fn add_java_candidates(
    workspace: &Workspace,
    signals: &RepoSignals,
    candidates: &mut Vec<QualityCandidate>,
) {
    use QualityCapability::{Format, Lint, StaticAnalysis, Test, TypeCheck};
    let maven = signals.has("pom.xml");
    let gradle = signals.has("build.gradle") || signals.has("build.gradle.kts");
    let build_text = format!(
        "{}\n{}\n{}",
        signals.text.get("pom.xml").cloned().unwrap_or_default(),
        signals
            .text
            .get("build.gradle")
            .cloned()
            .unwrap_or_default(),
        signals
            .text
            .get("build.gradle.kts")
            .cloned()
            .unwrap_or_default()
    )
    .to_ascii_lowercase();
    if maven {
        let program = if workspace.workspace_program_available("./mvnw") {
            "./mvnw"
        } else {
            "mvn"
        };
        let mut maven_compile = candidate!(
            "maven-compile",
            StaticAnalysis,
            QualityProviderSource::LanguageNative,
            program,
            ["-q", "-DskipTests", "compile"],
            true,
            "pom.xml",
            None,
        );
        maven_compile.covers.push(TypeCheck);
        candidates.push(maven_compile);
        candidates.push(candidate!(
            "maven-test",
            Test,
            QualityProviderSource::LanguageNative,
            program,
            ["test"],
            true,
            "pom.xml",
            None,
        ));
        candidates.push(candidate!(
            "maven-checkstyle",
            Lint,
            QualityProviderSource::RepositoryConfigured,
            program,
            ["checkstyle:check"],
            build_text.contains("checkstyle"),
            "Checkstyle plugin in pom.xml",
            None,
        ));
        candidates.push(candidate!(
            "maven-spotbugs",
            StaticAnalysis,
            QualityProviderSource::RepositoryConfigured,
            program,
            ["spotbugs:check"],
            build_text.contains("spotbugs"),
            "SpotBugs plugin in pom.xml",
            None,
        ));
        candidates.push(candidate!(
            "maven-spotless",
            Format,
            QualityProviderSource::RepositoryConfigured,
            program,
            ["spotless:check"],
            build_text.contains("spotless"),
            "Spotless plugin in pom.xml",
            None,
        ));
    } else if gradle {
        let program = if workspace.workspace_program_available("./gradlew") {
            "./gradlew"
        } else {
            "gradle"
        };
        let mut gradle_classes = candidate!(
            "gradle-classes",
            StaticAnalysis,
            QualityProviderSource::LanguageNative,
            program,
            ["classes"],
            true,
            "Gradle build",
            None,
        );
        gradle_classes.covers.push(TypeCheck);
        candidates.push(gradle_classes);
        candidates.push(candidate!(
            "gradle-test",
            Test,
            QualityProviderSource::LanguageNative,
            program,
            ["test"],
            true,
            "Gradle build",
            None,
        ));
        candidates.push(candidate!(
            "gradle-check",
            StaticAnalysis,
            QualityProviderSource::RepositoryConfigured,
            program,
            ["check"],
            true,
            "Gradle check lifecycle",
            None,
        ));
        candidates.push(candidate!(
            "gradle-spotless",
            Format,
            QualityProviderSource::RepositoryConfigured,
            program,
            ["spotlessCheck"],
            build_text.contains("spotless"),
            "Spotless plugin in Gradle build",
            None,
        ));
    }
}
