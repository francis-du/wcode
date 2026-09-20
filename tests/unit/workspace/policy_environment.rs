use super::*;

#[test]
fn python_execution_uses_one_isolated_nonwriting_bytecode_namespace() {
    let mut prefixes = Vec::new();
    for program in ["python3", "pytest"] {
        let mut command = tokio::process::Command::new(program);
        scrub_sensitive_environment(&mut command, program, &[], false);
        let env = command
            .as_std()
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();

        assert_eq!(
            env.get("PYTHONDONTWRITEBYTECODE")
                .and_then(|value| value.as_deref()),
            Some("1")
        );
        let prefix = env
            .get("PYTHONPYCACHEPREFIX")
            .and_then(|value| value.as_deref())
            .expect("Python execution must isolate bytecode cache lookup");
        assert!(prefix.contains("wcode-python-cache-"));
        prefixes.push(prefix.to_owned());
    }
    assert_eq!(prefixes[0], prefixes[1]);
}
