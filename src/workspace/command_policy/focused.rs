use std::path::Path;

pub(super) fn bounded_test_filter(program: &str, args: &[String]) -> bool {
    bounded_cargo_test_filter(program, args)
        || bounded_go_test_filter(program, args)
        || bounded_pytest_filter(program, args)
}

fn bounded_cargo_test_filter(program: &str, args: &[String]) -> bool {
    if program != "cargo" {
        return false;
    }
    let filter = match args {
        [command, filter] if command == "test" => filter.as_str(),
        [command, locked, filter] if command == "test" && locked == "--locked" => filter.as_str(),
        [command, lib, filter] if command == "test" && lib == "--lib" => filter.as_str(),
        [command, locked, lib, filter]
            if command == "test" && locked == "--locked" && lib == "--lib" =>
        {
            filter.as_str()
        }
        _ => return false,
    };
    simple_cargo_test_filter(filter)
}

fn simple_cargo_test_filter(value: &str) -> bool {
    if value.is_empty() || value.len() > 128 || value.starts_with('-') {
        return false;
    }
    value.split("::").all(|segment| {
        !segment.is_empty()
            && segment
                .chars()
                .all(|character| character == '_' || character.is_ascii_alphanumeric())
    })
}

fn bounded_go_test_filter(program: &str, args: &[String]) -> bool {
    if program != "go" {
        return false;
    }
    let (package, filter) = match args {
        [command, package, run, filter] if command == "test" && run == "-run" => {
            (package.as_str(), filter.as_str())
        }
        _ => return false,
    };
    safe_go_test_package(package) && exact_go_test_filter(filter)
}

fn safe_go_test_package(package: &str) -> bool {
    if package == "." {
        return true;
    }
    let Some(relative) = package.strip_prefix("./") else {
        return false;
    };
    !relative.split('/').any(|component| component == "...")
        && safe_focused_relative_path(relative, None)
}

fn exact_go_test_filter(filter: &str) -> bool {
    let Some(name) = filter
        .strip_prefix('^')
        .and_then(|value| value.strip_suffix('$'))
    else {
        return false;
    };
    let Some(suffix) = name.strip_prefix("Test") else {
        return false;
    };
    let Some(first) = suffix.chars().next() else {
        return false;
    };
    !first.is_ascii_lowercase() && simple_test_identifier(name)
}

fn bounded_pytest_filter(program: &str, args: &[String]) -> bool {
    if program != "pytest" {
        return false;
    }
    let node = match args {
        [quiet, node] if quiet == "-q" => node.as_str(),
        _ => return false,
    };
    let Some((path, symbol)) = node.split_once("::") else {
        return false;
    };
    !symbol.contains("::")
        && symbol.starts_with("test_")
        && simple_test_identifier(symbol)
        && safe_focused_relative_path(path, Some(".py"))
}

fn safe_focused_relative_path(path: &str, suffix: Option<&str>) -> bool {
    if path.is_empty()
        || path.starts_with('-')
        || path.contains(['\\', ':'])
        || suffix.is_some_and(|suffix| !path.ends_with(suffix))
        || Path::new(path).is_absolute()
        || path
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return false;
    }
    super::reject_protected_path(Path::new(path)).is_ok()
}

fn simple_test_identifier(value: &str) -> bool {
    if value.is_empty() || value.len() > 128 {
        return false;
    }
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}
