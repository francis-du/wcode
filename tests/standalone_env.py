"""Environment isolation and the controlled error type for the OSS verifier."""
from __future__ import annotations

import os


ENV_KEYS = (
    "PATH", "HOME", "USERPROFILE", "HOMEDRIVE", "HOMEPATH", "LOCALAPPDATA",
    "APPDATA", "SystemRoot", "SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT",
    "TEMP", "TMP", "TMPDIR", "CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN",
    "LANG", "LC_ALL", "TZ", "NUMBER_OF_PROCESSORS",
)
# Preserve tool discovery and library paths, not CL/LINK/RUSTFLAGS options.
# MSVC requires INCLUDE/LIB/LIBPATH; find-msvc-tools also uses the installation
# and SDK variables below. Keep them Windows-only and match names as Windows does.
WINDOWS_ENV_KEYS = (
    "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "ProgramData",
    "ALLUSERSPROFILE", "INCLUDE", "LIB", "LIBPATH", "VCINSTALLDIR",
    "VSINSTALLDIR", "VCToolsInstallDir", "VCToolsVersion", "WindowsSdkDir",
    "WindowsSDKVersion", "WindowsSDKLibVersion", "UniversalCRTSdkDir", "UCRTVersion",
    "VSCMD_ARG_HOST_ARCH", "VSCMD_ARG_TGT_ARCH",
)


class StandaloneError(RuntimeError):
    """A controlled diagnostic with no child output or environment."""


def controlled_env(source: dict[str, str] | None = None) -> dict[str, str]:
    source = os.environ if source is None else source
    if os.name == "nt":
        folded = {key.upper(): value for key, value in source.items()}
        env = {key: folded[key.upper()] for key in (*ENV_KEYS, *WINDOWS_ENV_KEYS)
               if key.upper() in folded}
    else:
        env = {key: source[key] for key in ENV_KEYS if key in source}
    env.update({
        "CARGO_NET_OFFLINE": "true",
        "CARGO_TERM_COLOR": "never", "RUST_BACKTRACE": "0",
        "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_TERMINAL_PROMPT": "0", "GIT_ATTR_NOSYSTEM": "1",
    })
    return env


