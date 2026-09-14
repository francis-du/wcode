use super::*;

pub(super) fn validate_language_development_tool(program: &str, args: &[String]) -> Result<()> {
    match program {
        "shellcheck" | "shfmt" | "clang-format" | "gofmt" | "staticcheck" | "govulncheck"
        | "eslint" | "stylelint" | "tsc" | "busted" | "luacheck" | "stylua" | "mypy"
        | "pyright" | "bandit" | "swift-format" | "swiftlint" | "cargo-audit" | "ocamlformat" => {
            Ok(())
        }
        "clang-tidy" => validate_clang_tidy(args),
        "gcc" | "g++" | "clang" | "clang++" | "cc" | "c++" => validate_native_compiler(args),
        "javac" => validate_java_compiler(args),
        "java" => validate_java_runner(args),
        "lua" => validate_script_runner(args, &[".lua"], &["-e", "-i"]),
        "ruby" => validate_script_runner(args, &[".rb"], &["-e", "--eval", "-r"]),
        "php" => validate_php_runner(args),
        "Rscript" => validate_rscript(args),
        "elixir" => validate_script_runner(args, &[".ex", ".exs"], &["-e", "--eval"]),
        "ocamlc" | "ocamlopt" => validate_native_compiler(args),
        "composer" => validate_composer(args),
        _ => bail!("unsupported language development tool: {program}"),
    }
}

fn validate_native_compiler(args: &[String]) -> Result<()> {
    if args.is_empty() {
        bail!("compiler invocation requires explicit arguments");
    }
    for arg in args {
        if arg.starts_with('@')
            || arg == "-wrapper"
            || arg == "-specs"
            || arg.starts_with("-specs=")
            || arg == "-fplugin"
            || arg.starts_with("-fplugin=")
            || arg == "-Xclang"
            || arg == "-B"
        {
            bail!("compiler helper/plugin/response-file redirection is blocked: {arg}");
        }
    }
    Ok(())
}

fn validate_clang_tidy(args: &[String]) -> Result<()> {
    for arg in args {
        if matches!(arg.as_str(), "--load" | "-load" | "--config-file")
            || arg.starts_with("--load=")
            || arg.starts_with("--config-file=")
        {
            bail!("clang-tidy plugin/config redirection is blocked: {arg}");
        }
    }
    Ok(())
}

fn validate_java_compiler(args: &[String]) -> Result<()> {
    if args.is_empty() {
        bail!("javac requires explicit source arguments");
    }
    for arg in args {
        if arg.starts_with('@')
            || matches!(
                arg.as_str(),
                "-J-javaagent" | "-J-agentlib" | "-J-agentpath"
            )
        {
            bail!("javac response-file/agent injection is blocked: {arg}");
        }
    }
    Ok(())
}

fn validate_java_runner(args: &[String]) -> Result<()> {
    if args.is_empty() {
        bail!("java requires an explicit main class or workspace JAR");
    }
    for arg in args {
        let lower = arg.to_ascii_lowercase();
        if arg.starts_with('@')
            || lower.starts_with("-javaagent")
            || lower.starts_with("-agentlib")
            || lower.starts_with("-agentpath")
            || lower.starts_with("-xbootclasspath")
        {
            bail!("java agent/response/bootstrap injection is blocked: {arg}");
        }
    }
    Ok(())
}

fn validate_script_runner(args: &[String], suffixes: &[&str], blocked: &[&str]) -> Result<()> {
    let Some(first) = args.first().map(String::as_str) else {
        bail!("script runner requires an explicit workspace script");
    };
    if blocked.contains(&first) {
        bail!("inline or interactive interpreter execution is blocked; execute a workspace script instead");
    }
    if first.starts_with('-') || !suffixes.iter().any(|suffix| first.ends_with(suffix)) {
        bail!("script runner requires a workspace script with a supported extension");
    }
    Ok(())
}

fn validate_php_runner(args: &[String]) -> Result<()> {
    match args {
        [flag, script, ..] if flag == "-l" && script.ends_with(".php") => Ok(()),
        [script, ..] if !script.starts_with('-') && script.ends_with(".php") => Ok(()),
        [flag, ..] if matches!(flag.as_str(), "-r" | "-a" | "-S") => {
            bail!("inline, interactive, or ad-hoc PHP server execution is blocked; execute a workspace script instead")
        }
        _ => bail!("php requires a workspace .php script or `-l` syntax check"),
    }
}

fn validate_rscript(args: &[String]) -> Result<()> {
    match args {
        [flag, expr] if flag == "-e" && is_known_r_quality_expression(expr) => Ok(()),
        [script, ..]
            if !script.starts_with('-') && (script.ends_with(".R") || script.ends_with(".r")) =>
        {
            Ok(())
        }
        [flag, ..] if flag == "-e" => {
            bail!("arbitrary inline R execution is blocked; execute a workspace script instead")
        }
        _ => bail!("Rscript requires a workspace R script or a built-in quality expression"),
    }
}

fn is_known_r_quality_expression(expr: &str) -> bool {
    matches!(
        expr,
        "quit(status=if(length(lintr::lint_package()))1 else 0)"
            | "testthat::test_local()"
            | "testthat::test_dir('tests/testthat')"
    )
}

fn validate_composer(args: &[String]) -> Result<()> {
    let Some(command) = args.first().map(String::as_str) else {
        bail!("composer requires an explicit command");
    };
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--global" | "-g") || arg == "--working-dir")
    {
        bail!("Composer global or cwd redirection is blocked");
    }
    match command {
        "install"
        | "update"
        | "require"
        | "remove"
        | "dump-autoload"
        | "validate"
        | "check-platform-reqs"
        | "test"
        | "run-script" => Ok(()),
        "config" | "global" | "self-update" => {
            bail!("Composer host-wide configuration/update commands are blocked")
        }
        _ => require_risky_exec(&format!("composer project operation: {command}"), false),
    }
}

pub(super) fn language_tool_writes_workspace(program: &str, args: &[String]) -> bool {
    match program {
        "shfmt" => args.iter().any(|arg| arg == "-w"),
        "clang-format" => args.iter().any(|arg| arg == "-i" || arg == "--in-place"),
        "gofmt" => args.iter().any(|arg| arg == "-w"),
        "eslint" | "stylelint" => args
            .iter()
            .any(|arg| arg == "--fix" || arg.starts_with("--fix=")),
        "stylua" => !args.iter().any(|arg| arg == "--check"),
        "ocamlformat" => args.iter().any(|arg| arg == "--inplace"),
        "gcc" | "g++" | "clang" | "clang++" | "cc" | "c++" | "javac" | "ocamlc" | "ocamlopt" => {
            !args
                .iter()
                .any(|arg| matches!(arg.as_str(), "-fsyntax-only" | "-E"))
        }
        "composer" => matches!(
            args.first().map(String::as_str),
            Some("install" | "update" | "require" | "remove" | "dump-autoload")
        ),
        _ => false,
    }
}
