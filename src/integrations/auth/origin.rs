use axum::http::{header, HeaderMap};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ENDPOINT_EPOCH: AtomicU64 = AtomicU64::new(1);
use std::sync::{Arc, RwLock};
use url::Url;

/// Tracks public origins verified by this runtime. Registered endpoints retain
/// lifecycle and browser-Origin semantics, while a syntactically valid request
/// Host may identify a stable custom HTTPS reverse-proxy endpoint.
#[derive(Clone)]
pub(crate) struct PublicEndpoints {
    primary: Arc<RwLock<String>>,
    active: Arc<RwLock<BTreeMap<String, u64>>>,
    trusted_resources: Arc<RwLock<BTreeSet<String>>>,
}

impl PublicEndpoints {
    pub(crate) fn new(initial: String) -> Self {
        let initial = normalize(&initial);
        Self {
            primary: Arc::new(RwLock::new(initial.clone())),
            active: Arc::new(RwLock::new(BTreeMap::from([(
                initial.clone(),
                NEXT_ENDPOINT_EPOCH.fetch_add(1, Ordering::Relaxed),
            )]))),
            trusted_resources: Arc::new(RwLock::new(BTreeSet::from([initial]))),
        }
    }

    pub(crate) fn primary(&self) -> String {
        self.primary
            .read()
            .expect("public endpoint lock poisoned")
            .clone()
    }

    pub(crate) fn set_primary(&self, value: String) {
        let value = normalize(&value);
        // Promotion is not a new endpoint owner. Preserve the registration
        // epoch so a primary change does not invalidate its own cleanup lease.
        self.active
            .write()
            .expect("public endpoint lock poisoned")
            .entry(value.clone())
            .or_insert_with(|| NEXT_ENDPOINT_EPOCH.fetch_add(1, Ordering::Relaxed));
        self.trusted_resources
            .write()
            .expect("trusted resource lock poisoned")
            .insert(value.clone());
        *self.primary.write().expect("public endpoint lock poisoned") = value;
    }

    pub(crate) fn register(&self, value: String) -> u64 {
        let epoch = NEXT_ENDPOINT_EPOCH.fetch_add(1, Ordering::Relaxed);
        self.active
            .write()
            .expect("public endpoint lock poisoned")
            .insert(normalize(&value), epoch);
        self.trusted_resources
            .write()
            .expect("trusted resource lock poisoned")
            .insert(normalize(&value));
        epoch
    }

    pub(crate) fn registration_epoch(&self, value: &str) -> Option<u64> {
        self.active
            .read()
            .expect("public endpoint lock poisoned")
            .get(&normalize(value))
            .copied()
    }

    pub(crate) fn unregister_if_epoch(&self, value: &str, expected: u64) -> bool {
        let value = normalize(value);
        let mut active = self.active.write().expect("public endpoint lock poisoned");
        if active.get(&value) != Some(&expected) {
            return false;
        }
        active.remove(&value);
        true
    }

    #[cfg(test)]
    pub(crate) fn unregister(&self, value: &str) {
        self.active
            .write()
            .expect("public endpoint lock poisoned")
            .remove(&normalize(value));
    }

    pub(crate) fn trust_resource(&self, resource: &str) {
        if let Some(origin) = mcp_origin(resource) {
            self.trusted_resources
                .write()
                .expect("trusted resource lock poisoned")
                .insert(normalize(origin));
        }
    }

    pub(crate) fn for_headers(&self, headers: &HeaderMap) -> Option<String> {
        let mut hosts = headers.get_all(header::HOST).iter();
        let Some(authority) = hosts.next() else {
            // Direct handler tests and internal calls may omit the authority,
            // but must not resurrect an endpoint removed from the active set.
            let primary = self.primary();
            return self
                .active
                .read()
                .expect("public endpoint lock poisoned")
                .contains_key(&primary)
                .then_some(primary);
        };
        if hosts.next().is_some() {
            return None;
        }
        let authority = authority.to_str().ok()?;
        if authority.contains(['/', '\\', '?', '#', '@'])
            || authority
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        {
            return None;
        }
        if let Some(origin) = self
            .active
            .read()
            .expect("public endpoint lock poisoned")
            .keys()
            .find(|origin| authority_matches(origin, authority))
            .cloned()
        {
            return Some(origin);
        }

        // Custom reverse proxies are not part of the managed-tunnel lifecycle.
        // Accept their request authority without adding it to the active Origin
        // allow-list or to historical OAuth resource equivalence.
        let request = Url::parse(&format!("https://{authority}")).ok()?;
        (request.username().is_empty()
            && request.password().is_none()
            && request.path() == "/"
            && request.query().is_none()
            && request.fragment().is_none()
            && request.host_str().is_some())
        .then(|| request.origin().ascii_serialization())
    }

    pub(crate) fn origin_allowed(&self, headers: &HeaderMap) -> bool {
        let mut origins = headers.get_all(header::ORIGIN).iter();
        let Some(value) = origins.next() else {
            return true;
        };
        if origins.next().is_some() {
            return false;
        }
        let Some(origin) = value.to_str().ok().and_then(parse_origin) else {
            return false;
        };
        // Only live, configured or instance-verified aliases are trusted.
        // Historical OAuth resources must never grant browser-origin access.
        self.active
            .read()
            .expect("public endpoint lock poisoned")
            .keys()
            .filter_map(|known| Url::parse(known).ok())
            .any(|known| known.origin() == origin)
    }

    pub(crate) fn equivalent_mcp_resources(&self, left: &str, right: &str) -> bool {
        if left == right {
            return true;
        }
        let (Some(left), Some(right)) = (mcp_origin(left), mcp_origin(right)) else {
            return false;
        };
        let trusted = self
            .trusted_resources
            .read()
            .expect("trusted resource lock poisoned");
        let active = self.active.read().expect("public endpoint lock poisoned");
        trusted.contains(&normalize(left)) && active.contains_key(&normalize(right))
    }
}

fn mcp_origin(resource: &str) -> Option<&str> {
    resource
        .strip_suffix("/mcp")
        .filter(|origin| !origin.is_empty())
}

fn normalize(value: &str) -> String {
    value.trim_end_matches('/').to_owned()
}

fn parse_origin(value: &str) -> Option<url::Origin> {
    // URL parsers repair whitespace, backslashes and dot paths. A security
    // header must already be a single serialized HTTP(S) origin, not a URL
    // that becomes an allowed origin after repairing malformed input.
    if value
        .bytes()
        .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return None;
    }
    let value = value.strip_suffix('/').unwrap_or(value);
    let (scheme, authority) = value.split_once("://")?;
    if !(scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http"))
        || authority.is_empty()
        || authority.contains(['/', '\\', '?', '#', '@'])
    {
        return None;
    }
    Url::parse(value).ok().map(|url| url.origin())
}

fn authority_matches(origin: &str, authority: &str) -> bool {
    if authority.contains(['/', '\\', '?', '#', '@'])
        || authority
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return false;
    }
    let Ok(origin) = Url::parse(origin) else {
        return false;
    };
    let Ok(request) = Url::parse(&format!("{}://{authority}", origin.scheme())) else {
        return false;
    };
    request.username().is_empty()
        && request.password().is_none()
        && request.path() == "/"
        && request.query().is_none()
        && request.fragment().is_none()
        && request.host_str().is_some_and(|host| {
            origin
                .host_str()
                .is_some_and(|known| host.eq_ignore_ascii_case(known))
        })
        && request.port_or_known_default() == origin.port_or_known_default()
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/auth/origin.rs"]
mod tests;
