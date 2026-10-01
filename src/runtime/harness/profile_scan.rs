use super::*;
use ignore::DirEntry;

const MAX_DISCOVERY_DIAGNOSTICS: usize = 16;
const MAX_PROFILE_FINGERPRINT_BYTES: u64 = 16 * 1024 * 1024;
const EXTRA_PROFILE_FILES: &[&str] = &[
    "setup.cfg",
    "tox.ini",
    "requirements-dev.txt",
    "requirements-test.txt",
    "conftest.py",
];

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ProfileDiscoveryIssue {
    pub path: String,
    pub kind: String,
}

/// Completeness covers the bounded discovery view, not semantic/runtime proof.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ProfileDiscoveryCompleteness {
    pub complete: bool,
    pub reasons: Vec<String>,
    pub errors: Vec<ProfileDiscoveryIssue>,
    pub errors_total: usize,
    pub diagnostics_truncated: bool,
    pub entries_seen: usize,
    pub profile_entries_seen: usize,
    pub contract_entries_seen: usize,
    pub manifest_candidates: usize,
    pub islands_returned: usize,
    pub contract_configs_seen: usize,
}

impl Default for ProfileDiscoveryCompleteness {
    fn default() -> Self {
        Self {
            complete: true,
            reasons: Vec::new(),
            errors: Vec::new(),
            errors_total: 0,
            diagnostics_truncated: false,
            entries_seen: 0,
            profile_entries_seen: 0,
            contract_entries_seen: 0,
            manifest_candidates: 0,
            islands_returned: 0,
            contract_configs_seen: 0,
        }
    }
}

impl ProfileDiscoveryCompleteness {
    pub(super) fn partial(&mut self, reason: &str) {
        self.complete = false;
        if self.reasons.iter().any(|value| value == reason) {
            return;
        }
        if self.reasons.len() < MAX_DISCOVERY_DIAGNOSTICS {
            self.reasons.push(reason.to_owned());
            self.reasons.sort();
        } else {
            self.diagnostics_truncated = true;
        }
    }

    fn issue(&mut self, root: &Path, path: &Path, kind: &str, reason: &str) {
        self.partial(reason);
        self.errors_total = self.errors_total.saturating_add(1);
        let relative = path
            .strip_prefix(root)
            .ok()
            .and_then(Path::to_str)
            .unwrap_or(".");
        let path = if relative.is_empty() { "." } else { relative };
        let issue = ProfileDiscoveryIssue {
            path: path.chars().filter(|c| !c.is_control()).take(300).collect(),
            kind: kind.to_owned(),
        };
        if self.errors.contains(&issue) {
            return;
        }
        if self.errors.len() < MAX_DISCOVERY_DIAGNOSTICS {
            self.errors.push(issue);
            self.errors
                .sort_by(|left, right| (&left.path, &left.kind).cmp(&(&right.path, &right.kind)));
        } else {
            self.diagnostics_truncated = true;
        }
    }

    fn walk_error(&mut self, root: &Path, error: &ignore::Error, path: &Path) {
        match error {
            ignore::Error::WithPath { path, err } => self.walk_error(root, err, path),
            ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => {
                self.walk_error(root, err, path)
            }
            ignore::Error::Partial(errors) => {
                for error in errors {
                    self.walk_error(root, error, path);
                }
            }
            _ => {
                let (kind, reason) = error.io_error().map_or(
                    ("configuration".to_owned(), "scan_configuration_error"),
                    |error| (io_kind(error), "scan_io_error"),
                );
                self.issue(root, path, &kind, reason);
            }
        }
    }
}

fn io_kind(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "not_found",
        std::io::ErrorKind::PermissionDenied => "permission_denied",
        std::io::ErrorKind::InvalidData => "invalid_data",
        std::io::ErrorKind::NotADirectory => "not_directory",
        _ => "io_error",
    }
    .to_owned()
}

pub(super) struct ProfileDiscoveryPaths {
    pub(super) candidate_dirs: Vec<PathBuf>,
    pub(super) contract_configs: Vec<PathBuf>,
    pub(super) manifest_files: Vec<PathBuf>,
    profile_input_files: Vec<PathBuf>,
    captured_sources: Arc<BTreeMap<PathBuf, Option<Arc<str>>>>,
    captured_digests: Arc<BTreeMap<PathBuf, Option<String>>>,
    pub(super) completeness: ProfileDiscoveryCompleteness,
}

impl ProfileDiscoveryPaths {
    // Persist only digests from the bytes used by this profile build. Guidance
    // is not a check definition; retaining it would stale policy on README edits.
    pub(super) fn policy_sources(
        &self,
        root: &Path,
    ) -> Result<Vec<crate::verification::policy::PolicySourceDigest>> {
        let mut sources = Vec::new();
        for (path, digest) in self.captured_digests.iter() {
            let relative = path
                .strip_prefix(root)
                .context("Policy source escaped the captured Workspace")?;
            if GUIDANCE_FILES
                .iter()
                .any(|name| relative.ends_with(Path::new(name)))
            {
                continue;
            }
            let path = relative
                .to_str()
                .context("Policy source path is not UTF-8")?
                .replace('\\', "/");
            sources.push(crate::verification::policy::PolicySourceDigest {
                path,
                sha256: digest.clone(),
            });
        }
        Ok(sources)
    }
}

#[derive(Clone, Copy)]
pub(super) struct ScanLimits {
    pub(super) entries: usize,
    pub(super) depth: usize,
    pub(super) contracts: usize,
}

pub(super) fn scan(workspace_root: &Path) -> ProfileDiscoveryPaths {
    scan_with_limits(
        workspace_root,
        ScanLimits {
            entries: MAX_PROFILE_SCAN_ENTRIES,
            depth: MAX_PROFILE_SCAN_DEPTH,
            contracts: contracts::MAX_CONTRACT_CONFIGS,
        },
    )
}

pub(super) fn scan_with_limits(workspace_root: &Path, limits: ScanLimits) -> ProfileDiscoveryPaths {
    let mut candidate_dirs = BTreeSet::new();
    let mut contract_configs = Vec::new();
    let mut manifest_files = Vec::new();
    let mut profile_input_files = Vec::new();
    let mut completeness = ProfileDiscoveryCompleteness::default();
    let mut builder = crate::workspace::repository_ignore_builder(workspace_root, true);
    builder
        .max_depth(Some(limits.depth.saturating_add(1)))
        .filter_entry(shared_visible_entry);
    for result in builder.build() {
        // The extra observed entry distinguishes a full budget from actual omission.
        completeness.entries_seen = completeness.entries_seen.saturating_add(1);
        if completeness.entries_seen > limits.entries {
            completeness.partial("scan_entry_budget");
            break;
        }
        let entry = match result {
            Ok(entry) => entry,
            Err(error) => {
                completeness.walk_error(workspace_root, &error, workspace_root);
                continue;
            }
        };
        if let Some(error) = entry.error() {
            completeness.walk_error(workspace_root, error, entry.path());
        }
        let profile_visible = entry.depth() >= 1
            && entry_visible_for(workspace_root, &entry, islands::profile_excluded_directory);
        let contract_visible = entry.depth() >= 1
            && entry_visible_for(
                workspace_root,
                &entry,
                contracts::contract_excluded_directory,
            );
        if (profile_visible || contract_visible)
            && entry.depth() == limits.depth.saturating_add(1)
            && entry.file_type().is_some_and(|kind| kind.is_dir())
        {
            // This directory is returned, but its descendants were not inspected.
            completeness.partial("scan_depth_unknown");
        }
        if profile_visible {
            let name = entry.file_name().to_string_lossy();
            if entry.file_type().is_some_and(|kind| kind.is_file())
                && name.ends_with(".py")
                && (name.starts_with("test") || name.ends_with("_test.py"))
            {
                profile_input_files.push(entry.path().to_path_buf());
            }
            completeness.profile_entries_seen = completeness.profile_entries_seen.saturating_add(1);
            if entry.file_type().is_some_and(|kind| kind.is_file())
                && islands::is_manifest_file_name(&entry.file_name().to_string_lossy())
            {
                manifest_files.push(entry.path().to_path_buf());
                if let Some(parent) = entry.path().parent() {
                    if parent != workspace_root {
                        candidate_dirs.insert(parent.to_path_buf());
                    }
                }
            }
        }
        if contract_visible {
            completeness.contract_entries_seen =
                completeness.contract_entries_seen.saturating_add(1);
            if entry.file_type().is_some_and(|kind| kind.is_file())
                && contracts::is_contract_config_name(&entry.file_name().to_string_lossy())
            {
                completeness.contract_configs_seen =
                    completeness.contract_configs_seen.saturating_add(1);
                if contract_configs.len() <= limits.contracts {
                    contract_configs.push(entry.path().to_path_buf());
                }
                if completeness.contract_configs_seen > limits.contracts {
                    completeness.partial("contract_config_budget");
                }
            }
        }
    }
    let mut candidate_dirs = candidate_dirs.into_iter().collect::<Vec<_>>();
    candidate_dirs.sort_by(|left, right| {
        left.components()
            .count()
            .cmp(&right.components().count())
            .then_with(|| left.cmp(right))
    });
    contract_configs.sort();
    manifest_files.sort();
    completeness.manifest_candidates = candidate_dirs.len();
    ProfileDiscoveryPaths {
        candidate_dirs,
        contract_configs,
        manifest_files,
        profile_input_files,
        completeness,
        captured_sources: Arc::new(BTreeMap::new()),
        captured_digests: Arc::new(BTreeMap::new()),
    }
}

pub(super) fn fingerprint_sources(
    root: &Path,
    discovery: &mut ProfileDiscoveryPaths,
    hasher: &mut DefaultHasher,
) {
    let mut paths = BTreeSet::new();
    for directory in
        std::iter::once(root).chain(discovery.candidate_dirs.iter().map(PathBuf::as_path))
    {
        for relative in PROFILE_FILES
            .iter()
            .chain(MANIFEST_FILES.iter())
            .chain(EXTRA_PROFILE_FILES.iter())
        {
            paths.insert(directory.join(relative));
        }
    }
    paths.extend(discovery.manifest_files.iter().cloned());
    paths.extend(discovery.profile_input_files.iter().cloned());
    paths.extend(discovery.contract_configs.iter().cloned());
    let mut bytes_read = 0u64;
    let mut sources = BTreeMap::new();
    let mut digests = BTreeMap::new();
    for path in paths {
        hash_source(
            root,
            &path,
            hasher,
            &mut discovery.completeness,
            &mut bytes_read,
            &mut sources,
            &mut digests,
        );
    }
    discovery.captured_sources = Arc::new(sources);
    discovery.captured_digests = Arc::new(digests);
    discovery.completeness.hash(hasher);
}

fn hash_source(
    root: &Path,
    path: &Path,
    hasher: &mut DefaultHasher,
    completeness: &mut ProfileDiscoveryCompleteness,
    bytes_read: &mut u64,
    sources: &mut BTreeMap<PathBuf, Option<Arc<str>>>,
    digests: &mut BTreeMap<PathBuf, Option<String>>,
) {
    use std::io::Read;
    path.strip_prefix(root).unwrap_or(path).hash(hasher);
    digests.insert(path.to_path_buf(), None);
    let Some(metadata) = plain_metadata(root, path, completeness) else {
        sources.insert(path.to_path_buf(), None);
        "unavailable".hash(hasher);
        return;
    };
    metadata.len().hash(hasher);
    sources.insert(path.to_path_buf(), None);
    if metadata.len() > MAX_PROFILE_SOURCE_BYTES
        || bytes_read.saturating_add(metadata.len()) > MAX_PROFILE_FINGERPRINT_BYTES
    {
        completeness.issue(root, path, "size_budget", "profile_source_budget");
        return;
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let result = options.open(path).and_then(|file| {
        let opened = file.metadata()?;
        if !opened.is_file() || opened.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "not a regular file",
            ));
        }
        let remaining = MAX_PROFILE_FINGERPRINT_BYTES.saturating_sub(*bytes_read);
        let mut bytes = Vec::new();
        file.take(MAX_PROFILE_SOURCE_BYTES.min(remaining).saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_PROFILE_SOURCE_BYTES.min(remaining) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                "source size bound",
            ));
        }
        *bytes_read = bytes_read.saturating_add(bytes.len() as u64);
        bytes.hash(hasher);
        digests.insert(
            path.to_path_buf(),
            Some(format!("{:x}", Sha256::digest(&bytes))),
        );
        if path.file_name().and_then(|name| name.to_str()) == Some("bun.lockb") {
            return Ok(());
        }
        match std::str::from_utf8(&bytes) {
            Ok(text) => {
                if !configuration_parses(path, text) {
                    completeness.issue(
                        root,
                        path,
                        "invalid_configuration",
                        "profile_source_invalid",
                    );
                }
                sources.insert(path.to_path_buf(), Some(Arc::from(text)));
            }
            Err(_) => {
                completeness.issue(root, path, "unsupported_encoding", "profile_source_invalid")
            }
        }
        Ok(())
    });
    if let Err(error) = result {
        let reason = if error.kind() == std::io::ErrorKind::FileTooLarge {
            "profile_source_budget"
        } else {
            "profile_source_unreadable"
        };
        completeness.issue(root, path, &io_kind(&error), reason);
    }
}

fn configuration_parses(path: &Path, text: &str) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    match name {
        "package.json" | "composer.json" | "tsconfig.json" => {
            serde_json::from_str::<Value>(text).is_ok_and(|value| value.is_object())
        }
        "Cargo.toml" => text.parse::<toml_edit::DocumentMut>().is_ok(),
        _ if contracts::is_contract_config_name(name)
            && name != "buf.yaml"
            && name != "buf.yml" =>
        {
            if name.ends_with(".json") {
                serde_json::from_str::<Value>(text).is_ok()
            } else if name.ends_with(".yaml") || name.ends_with(".yml") {
                serde_yaml::from_str::<Value>(text).is_ok()
            } else {
                true
            }
        }
        _ => true,
    }
}

// The scope is thread-local to one synchronous profile build. Captured text is
// immutable and never serialized or retained in the public project cache.
struct CapturedSources {
    root: PathBuf,
    sources: Arc<BTreeMap<PathBuf, Option<Arc<str>>>>,
    digests: Arc<BTreeMap<PathBuf, Option<String>>>,
    issues: std::cell::RefCell<ProfileDiscoveryCompleteness>,
}

thread_local! {
    static SOURCE_CAPTURE: std::cell::RefCell<Option<std::rc::Rc<CapturedSources>>> = const {
        std::cell::RefCell::new(None)
    };
}

struct CaptureGuard(Option<std::rc::Rc<CapturedSources>>);
impl Drop for CaptureGuard {
    fn drop(&mut self) {
        SOURCE_CAPTURE.with(|slot| slot.replace(self.0.take()));
    }
}

pub(super) fn with_captured_sources<T>(
    root: &Path,
    discovery: &ProfileDiscoveryPaths,
    build: impl FnOnce() -> T,
) -> (T, ProfileDiscoveryCompleteness) {
    let capture = std::rc::Rc::new(CapturedSources {
        root: root.to_path_buf(),
        sources: discovery.captured_sources.clone(),
        digests: discovery.captured_digests.clone(),
        issues: std::cell::RefCell::new(ProfileDiscoveryCompleteness::default()),
    });
    let previous = SOURCE_CAPTURE.with(|slot| slot.replace(Some(capture.clone())));
    let _guard = CaptureGuard(previous);
    let result = build();
    let issues = capture.issues.borrow().clone();
    (result, issues)
}

/// Outer None means no build capture; inner None is an unavailable captured
/// input. A build never falls back to reading a different live body.
pub(super) fn captured_text(path: &Path) -> Option<Option<String>> {
    SOURCE_CAPTURE.with(|slot| {
        let current = slot.borrow();
        let capture = current.as_ref()?;
        if let Some(Some(text)) = capture.sources.get(path) {
            return Some(Some(text.to_string()));
        }
        // Binary or invalid text can still have a captured byte digest. Missing
        // inputs have neither: detect later appearance without reading live bytes.
        if capture.digests.get(path).is_some_and(Option::is_some) {
            return Some(None);
        }
        let kind = match fs::symlink_metadata(path) {
            Ok(_) => Some("uncaptured_input".to_owned()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => Some(io_kind(&error)),
        };
        if let Some(kind) = kind {
            capture.issues.borrow_mut().issue(
                &capture.root,
                path,
                &kind,
                "profile_capture_incomplete",
            );
        }
        Some(None)
    })
}

impl ProfileDiscoveryCompleteness {
    pub(super) fn absorb_capture_issues(&mut self, other: Self) {
        self.errors_total = self.errors_total.saturating_add(other.errors_total);
        self.diagnostics_truncated |= other.diagnostics_truncated;
        for reason in other.reasons {
            self.partial(&reason);
        }
        for issue in other.errors {
            if self.errors.contains(&issue) {
                continue;
            }
            if self.errors.len() < MAX_DISCOVERY_DIAGNOSTICS {
                self.errors.push(issue);
            } else {
                self.diagnostics_truncated = true;
            }
        }
        self.errors
            .sort_by(|left, right| (&left.path, &left.kind).cmp(&(&right.path, &right.kind)));
    }
}

fn plain_metadata(
    root: &Path,
    path: &Path,
    completeness: &mut ProfileDiscoveryCompleteness,
) -> Option<fs::Metadata> {
    let relative = path.strip_prefix(root).ok()?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => {
                completeness.issue(root, path, &io_kind(&error), "profile_source_unreadable");
                return None;
            }
        };
        if metadata.file_type().is_symlink() {
            completeness.issue(root, path, "symlink", "profile_source_unreadable");
            return None;
        }
        if current == path {
            if metadata.is_file() {
                return Some(metadata);
            }
            completeness.issue(root, path, "not_regular", "profile_source_unreadable");
            return None;
        }
    }
    None
}

fn shared_visible_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    if entry.file_type().is_some_and(|kind| kind.is_symlink()) {
        return false;
    }
    if !entry.file_type().is_some_and(|kind| kind.is_dir()) {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    !islands::profile_excluded_directory(&name) || !contracts::contract_excluded_directory(&name)
}

fn entry_visible_for(
    workspace_root: &Path,
    entry: &DirEntry,
    excluded_directory: fn(&str) -> bool,
) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    if entry.file_type().is_some_and(|kind| kind.is_symlink()) {
        return false;
    }
    let Ok(relative) = entry.path().strip_prefix(workspace_root) else {
        return false;
    };
    let checked = if entry.file_type().is_some_and(|kind| kind.is_dir()) {
        relative
    } else {
        relative.parent().unwrap_or_else(|| Path::new(""))
    };
    checked.components().all(|component| {
        component
            .as_os_str()
            .to_str()
            .is_none_or(|name| !excluded_directory(name))
    })
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/discovery.rs"]
mod tests;
