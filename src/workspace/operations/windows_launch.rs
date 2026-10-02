//! Resolve installed Node package-manager scripts for fixed verification commands.
//! Rust's Windows executable lookup does not infer the .cmd extension.
use std::path::PathBuf;

#[cfg(windows)]
pub(super) fn verification_launcher(program: &str, args: &[String]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let directories = std::env::split_paths(&path).collect::<Vec<_>>();
    find_verification_launcher(program, args, &directories)
}

fn find_verification_launcher(
    program: &str,
    args: &[String],
    directories: &[PathBuf],
) -> Option<PathBuf> {
    // Never infer a shell for arbitrary programs or arguments. These package
    // managers share the existing fixed `run <check>` verification contract.
    if !matches!(program, "npm" | "pnpm" | "yarn")
        || super::validate_verification_command_shape(program, args).is_err()
    {
        return None;
    }
    // Keep existing native executable installations working, including proxies.
    if directories.iter().any(|directory| {
        directory.is_absolute() && directory.join(format!("{program}.exe")).is_file()
    }) {
        return None;
    }
    directories
        .iter()
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(format!("{program}.cmd")))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
#[path = "../../../tests/unit/workspace/windows_launch.rs"]
mod tests;
