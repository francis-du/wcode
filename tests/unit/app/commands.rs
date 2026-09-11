use super::*;

#[test]
fn complete_catalog_covers_every_declared_command_and_argument() {
    let mut root = super::super::Args::command();
    root.build();
    let catalog = command_catalog(&root);
    fn check(command: &Command, value: &Value) {
        assert_eq!(value["name"], command.get_name());
        let arguments = value["arguments"].as_array().unwrap();
        assert_eq!(arguments.len(), command.get_arguments().count());
        let mut rendered = Vec::new();
        write_command_help(command, &mut rendered).unwrap();
        let rendered = String::from_utf8(rendered).unwrap();
        for arg in command.get_arguments() {
            let row = arguments
                .iter()
                .find(|row| row["id"] == arg.get_id().as_str())
                .unwrap();
            assert_eq!(row["global"], arg.is_global_set());
            if let Some(long) = arg.get_long() {
                assert_eq!(row["long"], long);
                assert!(rendered.contains(&format!("--{long}")), "omitted {long}");
            }
            if let Some(short) = arg.get_short() {
                assert_eq!(row["short"], short.to_string());
                assert!(rendered.contains(&format!("-{short}")));
            }
            assert_eq!(
                row["possible_values"].as_array().unwrap().len(),
                arg.get_possible_values().len()
            );
        }
        let children = value["subcommands"].as_array().unwrap();
        assert_eq!(children.len(), command.get_subcommands().count());
        for (child, row) in command.get_subcommands().zip(children) {
            check(child, row);
        }
    }
    check(&root, &catalog);
}

#[test]
fn complete_help_preserves_the_original_parser_and_compact_help() {
    let mut root = super::super::Args::command();
    root.build();
    let before = root.render_long_help().to_string();
    assert!(!before.contains("--max-parallel-tools"));
    assert!(root.find_subcommand("agent-plugin").unwrap().is_hide_set());
    let mut bytes = Vec::new();
    write_command_help(&root, &mut bytes).unwrap();
    assert_eq!(before, root.render_long_help().to_string());
    assert!(root.find_subcommand("agent-plugin").unwrap().is_hide_set());
    root.clone().debug_assert();
}

#[test]
fn complete_help_inherits_globals_and_retains_value_and_alias_metadata() {
    let catalog: Value =
        serde_json::from_str(&complete_help(&["verification".into()], true).unwrap()).unwrap();
    let args = catalog["command"]["arguments"].as_array().unwrap();
    let preset = args
        .iter()
        .find(|arg| arg["long"] == "performance")
        .unwrap();
    assert_eq!(preset["default_values"], json!(["balanced"]));
    assert_eq!(preset["global"], true);
    assert_eq!(
        preset["possible_values"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["balanced", "fast", "light"]
    );
    let readonly = args.iter().find(|arg| arg["long"] == "read-only").unwrap();
    assert_eq!(readonly["action"], "SetFalse");
    assert_eq!(readonly["default_values"], json!(["true"]));
    let plan = args.iter().find(|arg| arg["long"] == "plan-id").unwrap();
    assert_eq!(plan["aliases"], json!(["plan"]));
    assert!(!args.iter().any(|arg| arg["long"] == "host"));
}

#[test]
fn complete_catalog_recurses_into_hidden_nested_commands() {
    let mut root = Command::new("fixture")
        .disable_help_subcommand(true)
        .arg(
            clap::Arg::new("shared")
                .long("shared")
                .global(true)
                .hide(true),
        )
        .subcommand(
            Command::new("outer").hide(true).alias("o").subcommand(
                Command::new("inner").hide(true).arg(
                    clap::Arg::new("secret-mode")
                        .long("secret-mode")
                        .short('s')
                        .alias("mode")
                        .short_alias('m')
                        .hide_long_help(true)
                        .default_value("one")
                        .value_parser(["one", "two"]),
                ),
            ),
        );
    root.build();
    let mut bytes = Vec::new();
    write_command_help(&root, &mut bytes).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    for term in [
        "fixture outer inner",
        "--secret-mode",
        "--mode",
        "-m",
        "--shared",
        "one",
        "two",
        "Command aliases: o",
    ] {
        assert!(text.contains(term), "{term} missing from {text}");
    }
    let catalog = command_catalog(&root);
    assert_eq!(catalog["subcommands"][0]["subcommands"][0]["name"], "inner");
}

#[test]
fn complete_catalog_output_handles_closed_pipes_but_not_other_io_failures() {
    struct FailingWriter(io::ErrorKind);
    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(self.0))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    assert!(write_catalog(&mut FailingWriter(io::ErrorKind::BrokenPipe), b"help").is_ok());
    assert_eq!(
        write_catalog(&mut FailingWriter(io::ErrorKind::PermissionDenied), b"help")
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
}
