use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

mod reports;
pub use reports::{
    verification_metrics_summary, ChangeReviewReport, ChangedFileReview, ProjectVerificationImpact,
    ProjectVerificationImpactReason, ReviewFinding, ReviewProbeSummary, VerificationCheck,
    VerificationCostDecision, VerificationCostFrontierEntry, VerificationReport,
};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredVerificationCheck {
    pub id: String,
    pub signature: String,
}

impl RequiredVerificationCheck {
    pub fn from_command(id: &str, program: &str, args: &[String], cwd: &str, island: &str) -> Self {
        use sha2::{Digest, Sha256};
        let input = serde_json::to_vec(&(id, program, args, cwd, island))
            .expect("string command binding is serializable");
        Self {
            id: id.to_owned(),
            signature: format!("sha256:{:x}", Sha256::digest(input)),
        }
    }

    pub fn valid(&self) -> bool {
        valid_text_id(&self.id, 160)
            && self
                .signature
                .strip_prefix("sha256:")
                .is_some_and(|digest| {
                    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationCheckExecution {
    #[default]
    Unknown,
    Executed,
    Unavailable,
    TimedOut,
}

fn valid_text_id(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionGitBinding {
    pub repository: String,
    pub head_sha: String,
    pub tree_sha: String,
    pub dirty: bool,
    pub index_fingerprint: String,
}

impl ExecutionGitBinding {
    pub fn valid(&self) -> bool {
        sha256_identity(&self.repository)
            && full_oid(&self.head_sha)
            && full_oid(&self.tree_sha)
            && self.head_sha.len() == self.tree_sha.len()
            && sha256_identity(&self.index_fingerprint)
    }
}

fn full_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn sha256_identity(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

pub fn authority_state_root() -> io::Result<PathBuf> {
    platform_state_root(false)
}

#[cfg(test)]
pub fn intelligence_state_root() -> io::Result<PathBuf> {
    Ok(std::env::temp_dir()
        .join("wcode-test-intelligence")
        .join(std::process::id().to_string()))
}

#[cfg(not(test))]
pub fn intelligence_state_root() -> io::Result<PathBuf> {
    platform_state_root(true)
}

fn platform_state_root(intelligence: bool) -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("WCODE_STATE_DIR").filter(|value| !value.is_empty()) {
        return Ok(if intelligence {
            PathBuf::from(path).join("intelligence")
        } else {
            PathBuf::from(path)
        });
    }
    let base = if cfg!(target_os = "windows") {
        required_env("LOCALAPPDATA", Some("USERPROFILE"))?
    } else if let Some(path) = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty())
    {
        return Ok(PathBuf::from(path).join(if intelligence {
            "wcode/intelligence"
        } else {
            "wcode"
        }));
    } else {
        required_env("HOME", None)?
    };
    if cfg!(target_os = "windows") {
        Ok(PathBuf::from(base).join(if intelligence {
            "wcode/intelligence"
        } else {
            "wcode"
        }))
    } else {
        Ok(PathBuf::from(base).join(if intelligence {
            ".local/state/wcode/intelligence"
        } else {
            ".local/state/wcode"
        }))
    }
}

fn required_env(primary: &str, fallback: Option<&str>) -> io::Result<OsString> {
    std::env::var_os(primary)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            fallback.and_then(|name| std::env::var_os(name).filter(|value| !value.is_empty()))
        })
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                match fallback {
                    Some(_) => "required platform state environment is not set",
                    None => "HOME is not set",
                },
            )
        })
}
