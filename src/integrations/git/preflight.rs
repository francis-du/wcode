//! Read-only deployment diagnostics. A matching configuration is not Acceptance,
//! credential write authority, a merge lock, or proof of OS isolation.
use super::*;
use std::collections::BTreeSet;

const MAX_RULE_PAGES: usize = 5;
const MAX_RULESETS: usize = 16;

#[derive(Clone, Debug, Serialize)]
pub struct PreflightFinding {
    pub code: &'static str,
    pub action: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct GitHubPreflight {
    pub schema_version: u32,
    pub target: ProviderTarget,
    pub base_branch: String,
    pub required_check: String,
    pub expected_app_id: u64,
    pub configuration_verified: bool,
    pub protection_source: Option<&'static str>,
    pub ruleset_id: Option<u64>,
    pub remote_mutations: bool,
    pub repository_mutations: bool,
    pub credential_renewal_enabled: bool,
    pub acceptance_evaluated: bool,
    pub credential_write_permission_verified: bool,
    pub deployment_isolation_verified: bool,
    pub findings: Vec<PreflightFinding>,
}

impl GitHubPreflight {
    fn finding(&mut self, code: &'static str, action: &'static str) {
        self.findings.push(PreflightFinding { code, action });
    }
}

impl GitHubProvider {
    async fn preflight_target(&self, change: u64) -> Result<(ProviderTarget, String)> {
        if change == 0 {
            bail!("invalid pull request identity");
        }
        let number = change.to_string();
        let value = self
            .request(Method::GET, &["pulls", &number], None, StatusCode::OK)
            .await?;
        let pull: Pull = serde_json::from_value(value)
            .map_err(|_| anyhow!("GitHub pull metadata incomplete"))?;
        let target = self.target_from_pull(change, &pull)?;
        let branch = pull
            .base
            .reference
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 255
                    && !value.chars().any(char::is_control)
                    && !value.contains('*')
                    && !value
                        .split('/')
                        .any(|part| part.is_empty() || part == "." || part == "..")
            })
            .ok_or_else(|| anyhow!("GitHub base branch metadata incomplete"))?;
        Ok((target, branch))
    }

    // Only canonical API paths are used; never follow response URLs or redirects.
    // Missing/forbidden metadata cannot be interpreted as an empty safe policy.
    async fn preflight_get(&self, suffix: &[&str], page: Option<usize>) -> Result<(Value, bool)> {
        let mut url = self.endpoint(suffix)?;
        if let Some(page) = page {
            url.query_pairs_mut()
                .append_pair("per_page", "100")
                .append_pair("page", &page.to_string());
        }
        let mut response = self.send_authenticated(Method::GET, url, None).await?;
        if response.status() != StatusCode::OK {
            bail!(
                "GitHub preflight metadata unavailable (HTTP {})",
                response.status().as_u16()
            );
        }
        let next = has_next(response.headers())?;
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
        {
            bail!("GitHub preflight metadata exceeds bound");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow!("GitHub preflight response unavailable"))?
        {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                bail!("GitHub preflight metadata exceeds bound");
            }
            bytes.extend_from_slice(&chunk);
        }
        let value = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow!("GitHub preflight metadata invalid"))?;
        Ok((value, next))
    }

    async fn branch_rules(&self, branch: &str) -> Result<Vec<Value>> {
        let mut rules = Vec::new();
        let mut seen = BTreeSet::new();
        for page in 1..=MAX_RULE_PAGES {
            let (value, next) = self
                .preflight_get(&["rules", "branches", branch], Some(page))
                .await?;
            let batch = value
                .as_array()
                .ok_or_else(|| anyhow!("GitHub branch rules incomplete"))?;
            if batch.len() > 100 || (batch.is_empty() && next) {
                bail!("GitHub branch rules pagination invalid");
            }
            for rule in batch {
                let kind = rule
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("GitHub branch rule type missing"))?;
                let id = rule
                    .get("ruleset_id")
                    .and_then(Value::as_u64)
                    .filter(|id| *id > 0)
                    .ok_or_else(|| anyhow!("GitHub branch ruleset identity missing"))?;
                if !seen.insert((id, kind.to_owned())) {
                    bail!("GitHub branch rules duplicated across pages");
                }
            }
            rules.extend(batch.iter().cloned());
            if !next && batch.len() < 100 {
                return Ok(rules);
            }
        }
        bail!("GitHub branch rules exceed the bounded complete scan");
    }

    /// Observe exact PR/base identity and a strict App-bound required check.
    /// Reads classic protection and effective repository/organization rulesets;
    /// hidden bypass metadata stays unknown. This never creates or updates a check.
    pub async fn preflight(&self, change: u64) -> Result<GitHubPreflight> {
        tokio::time::timeout(Duration::from_secs(45), self.preflight_inner(change))
            .await
            .map_err(|_| anyhow!("GitHub deployment preflight deadline exceeded"))?
    }

    async fn preflight_inner(&self, change: u64) -> Result<GitHubPreflight> {
        let (target, branch) = self.preflight_target(change).await?;
        let mut report = GitHubPreflight {
            schema_version: 1,
            target,
            base_branch: branch.clone(),
            required_check: self.config.check_name.clone(),
            expected_app_id: self.config.app_id,
            configuration_verified: false,
            protection_source: None,
            ruleset_id: None,
            // App mode may issue a short-lived credential even though repository
            // inspection remains GET-only. Do not report globally read-only I/O.
            remote_mutations: self.credentials.renewable(),
            repository_mutations: false,
            credential_renewal_enabled: self.credentials.renewable(),
            acceptance_evaluated: false,
            credential_write_permission_verified: false,
            deployment_isolation_verified: false,
            findings: Vec::new(),
        };
        let classic = self
            .preflight_get(&["branches", &branch, "protection"], None)
            .await;
        match classic {
            Ok((value, false)) if classic_matches(&self.config, &value) => {
                report.protection_source = Some("classic_branch_protection");
            }
            Ok(_) => report.finding("classic_binding_not_verified",
                "Require the exact Check/App, strict base updates and administrator enforcement."),
            Err(_) => report.finding("classic_protection_unavailable",
                "Check repository access and Administration read permission, or inspect the effective ruleset."),
        }
        let rules = match self.branch_rules(&branch).await {
            Ok(rules) => rules,
            Err(_) => {
                report.finding("effective_rules_incomplete",
                    "Restore metadata access and retry; partial or unavailable rules are not deployment proof.");
                return Ok(report);
            }
        };
        if rules.iter().any(|rule| rule["type"] == "merge_queue") {
            report.finding("merge_group_not_supported",
                "This publisher targets PR head commits, not merge-group commits; do not enable this deployment for a merge queue.");
            return Ok(report);
        }
        if report.protection_source.is_none() {
            let candidates = rules
                .iter()
                .filter(|rule| rule_matches(&self.config, rule))
                .collect::<Vec<_>>();
            if candidates.len() > MAX_RULESETS {
                report.finding(
                    "ruleset_bound_exceeded",
                    "Reduce the ambiguous deployment scope and inspect all applicable rulesets.",
                );
                return Ok(report);
            }
            for rule in candidates {
                let id = rule["ruleset_id"]
                    .as_u64()
                    .expect("validated branch rule identity");
                let number = id.to_string();
                let detail = self.preflight_get(&["rulesets", &number], None).await;
                match detail {
                    Ok((value, false)) if ruleset_matches(&self.config, rule, &value) => {
                        report.protection_source = Some("active_ruleset");
                        report.ruleset_id = Some(id);
                        break;
                    }
                    _ => report.finding("ruleset_bypass_or_identity_unverified",
                        "Have the ruleset operator inspect its active identity and bypass actors; absent bypass metadata is not an empty bypass list."),
                }
            }
        }
        if report.protection_source.is_none() {
            report.finding("required_check_binding_unverified",
                "Configure an enforced strict required check tied to the expected App; a context name alone is insufficient.");
            return Ok(report);
        }
        // Detect sequential observation drift without claiming an atomic merge lock.
        match self.preflight_target(change).await {
            Ok((current, current_branch))
                if current == report.target && current_branch == branch =>
            {
                report.configuration_verified = true;
            }
            _ => report.finding(
                "candidate_changed_or_unavailable",
                "Repeat preflight for the current PR base/head; do not reuse this observation.",
            ),
        }
        Ok(report)
    }

    /// Operational publication performs preflight afresh and pins the candidate
    /// inside the native-capture lane. No caller JSON can turn this into green.
    pub async fn publish_preflighted(
        &self,
        change: u64,
        candidate: (&str, &str),
        root: impl AsRef<std::path::Path>,
        workspace_id: &str,
    ) -> Result<GitHubGateReceipt> {
        self.publish_preflighted_with_guard(change, candidate, root, workspace_id, None)
            .await
    }

    pub(super) async fn publish_preflighted_with_guard(
        &self,
        change: u64,
        candidate: (&str, &str),
        root: impl AsRef<std::path::Path>,
        workspace_id: &str,
        admission: Option<Arc<PublicationGuard>>,
    ) -> Result<GitHubGateReceipt> {
        let expected = ProviderTarget {
            repository: self.config.repository.clone(),
            change,
            base_sha: candidate.0.to_ascii_lowercase(),
            head_sha: candidate.1.to_ascii_lowercase(),
        };
        expected.validate()?;
        let preflight = self.preflight(change).await?;
        if preflight.target != expected {
            bail!("GitHub PR no longer matches the explicitly requested candidate");
        }
        if !preflight.configuration_verified {
            bail!(
                "GitHub deployment preflight incomplete; run github preflight --json for actions"
            );
        }
        let root = root.as_ref().to_path_buf();
        let workspace_id = workspace_id.to_owned();
        self.publish_bound_with_guard(
            change,
            Some(preflight.target.clone()),
            move |target| {
                let expected = preflight.target.clone();
                let root = root.clone();
                let workspace_id = workspace_id.clone();
                async move {
                    if target != expected {
                        bail!("GitHub candidate changed after deployment preflight");
                    }
                    crate::verification::acceptance_native::capture_local(
                        root,
                        &workspace_id,
                        &target.base_sha,
                        &target.head_sha,
                    )
                    .await
                }
            },
            admission,
        )
        .await
    }
}

pub(super) fn has_next(headers: &reqwest::header::HeaderMap) -> Result<bool> {
    let mut next = false;
    let mut links = 0usize;
    for value in headers.get_all(reqwest::header::LINK) {
        let value = value
            .to_str()
            .map_err(|_| anyhow!("GitHub pagination metadata invalid"))?;
        if value.is_empty() || value.len() > 8192 {
            bail!("GitHub pagination metadata invalid");
        }
        for link in value.split(',') {
            links += 1;
            let (url, parameters) = link
                .trim()
                .split_once('>')
                .filter(|(url, parameters)| {
                    url.starts_with('<')
                        && url.len() > 1
                        && parameters.trim_start().starts_with(';')
                })
                .ok_or_else(|| anyhow!("GitHub pagination metadata invalid"))?;
            // The URL is syntactic metadata only. Requests are constructed from
            // the pinned repository and numeric page, never from this URL.
            if links > 32 || url.chars().any(char::is_control) {
                bail!("GitHub pagination metadata invalid");
            }
            let mut relation = None;
            for parameter in parameters
                .split(';')
                .map(str::trim)
                .filter(|part| !part.is_empty())
            {
                let (key, value) = parameter
                    .split_once('=')
                    .ok_or_else(|| anyhow!("GitHub pagination metadata invalid"))?;
                if key.trim().eq_ignore_ascii_case("rel") {
                    if relation.is_some() {
                        bail!("GitHub pagination relation duplicated");
                    }
                    let value = value.trim();
                    relation = Some(if value.starts_with('"') {
                        value
                            .strip_prefix('"')
                            .and_then(|value| value.strip_suffix('"'))
                            .ok_or_else(|| anyhow!("GitHub pagination metadata invalid"))?
                    } else {
                        value
                    });
                }
            }
            let relation = relation
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("GitHub pagination relation missing"))?;
            for name in relation.split_ascii_whitespace() {
                match name {
                    "next" => next = true,
                    "prev" | "first" | "last" => {}
                    _ => bail!("GitHub pagination relation unknown"),
                }
            }
        }
    }
    Ok(next)
}

fn checks_match(config: &GitHubConfig, value: &Value, app_field: &str) -> bool {
    let Some(checks) = value.as_array().filter(|checks| checks.len() <= 100) else {
        return false;
    };
    let matching = checks
        .iter()
        .filter(|check| check["context"] == config.check_name)
        .collect::<Vec<_>>();
    !matching.is_empty()
        && matching
            .iter()
            .all(|check| check[app_field].as_u64() == Some(config.app_id))
}

fn classic_matches(config: &GitHubConfig, value: &Value) -> bool {
    value
        .pointer("/enforce_admins/enabled")
        .and_then(Value::as_bool)
        == Some(true)
        && value
            .pointer("/required_status_checks/strict")
            .and_then(Value::as_bool)
            == Some(true)
        && checks_match(config, &value["required_status_checks"]["checks"], "app_id")
}

fn rule_matches(config: &GitHubConfig, value: &Value) -> bool {
    value["type"] == "required_status_checks"
        && value["parameters"]["strict_required_status_checks_policy"].as_bool() == Some(true)
        && checks_match(
            config,
            &value["parameters"]["required_status_checks"],
            "integration_id",
        )
}

fn ruleset_matches(config: &GitHubConfig, rule: &Value, detail: &Value) -> bool {
    detail["id"].as_u64() == rule["ruleset_id"].as_u64()
        && detail["enforcement"] == "active"
        && detail["target"] == "branch"
        && rule["ruleset_source"].as_str().is_some()
        && detail["source"] == rule["ruleset_source"]
        && rule["ruleset_source_type"].as_str().is_some()
        && detail["source_type"] == rule["ruleset_source_type"]
        && detail["bypass_actors"]
            .as_array()
            .is_some_and(Vec::is_empty)
        && detail["rules"].as_array().is_some_and(|rules| {
            rules.len() <= 100
                && rules.iter().any(|candidate| {
                    rule_matches(config, candidate) && candidate["parameters"] == rule["parameters"]
                })
        })
}
