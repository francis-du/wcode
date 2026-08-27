use super::*;

pub(super) fn validate_fd_command(args: &[String]) -> Result<()> {
    for arg in args {
        if matches!(
            arg.as_str(),
            "-H" | "--hidden"
                | "-I"
                | "--no-ignore"
                | "--no-ignore-vcs"
                | "-u"
                | "--unrestricted"
                | "-L"
                | "--follow"
                | "-a"
                | "--absolute-path"
                | "-x"
                | "--exec"
                | "-X"
                | "--exec-batch"
                | "--search-path"
                | "--base-directory"
                | "--ignore-file"
        ) || arg.starts_with("--search-path=")
            || arg.starts_with("--base-directory=")
            || arg.starts_with("--ignore-file=")
        {
            bail!("fd option is blocked because it can escape normal repository visibility or execute commands: {arg}");
        }
    }
    Ok(())
}

pub(super) fn validate_jq_command(args: &[String]) -> Result<()> {
    if args.is_empty() {
        bail!("jq requires an explicit filter");
    }
    for arg in args {
        if matches!(
            arg.as_str(),
            "-f" | "--from-file"
                | "-L"
                | "--library-path"
                | "--slurpfile"
                | "--rawfile"
                | "--argfile"
                | "--run-tests"
        ) || arg.starts_with("--from-file=")
            || arg.starts_with("--library-path=")
            || arg.starts_with("--slurpfile=")
            || arg.starts_with("--rawfile=")
            || arg.starts_with("--argfile=")
        {
            bail!("jq option is blocked because it can load additional files or modules outside the explicit input arguments: {arg}");
        }
    }
    Ok(())
}
