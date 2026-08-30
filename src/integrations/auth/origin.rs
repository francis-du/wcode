use axum::http::{header, HeaderMap};
use std::collections::BTreeSet;
use std::sync::{Arc, RwLock};
use url::Url;

/// Tracks public origins verified by this runtime. Request headers may only
/// select an origin from this allow-list, so an arbitrary Host cannot rewrite
/// OAuth metadata or weaken resource binding.
#[derive(Clone)]
pub(crate) struct PublicEndpoints {
    primary: Arc<RwLock<String>>,
    active: Arc<RwLock<BTreeSet<String>>>,
    trusted_resources: Arc<RwLock<BTreeSet<String>>>,
}

impl PublicEndpoints {
    pub(crate) fn new(initial: String) -> Self {
        let initial = normalize(&initial);
        Self {
            primary: Arc::new(RwLock::new(initial.clone())),
            active: Arc::new(RwLock::new(BTreeSet::from([initial.clone()]))),
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
        self.register(value.clone());
        *self.primary.write().expect("public endpoint lock poisoned") = value;
    }

    pub(crate) fn register(&self, value: String) {
        self.active
            .write()
            .expect("public endpoint lock poisoned")
            .insert(normalize(&value));
        self.trusted_resources
            .write()
            .expect("trusted resource lock poisoned")
            .insert(normalize(&value));
    }

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
        let Some(authority) = headers.get(header::HOST) else {
            // Direct handler tests and internal calls do not carry an HTTP
            // authority. Real HTTP/1.1 and HTTP/2 requests always do.
            return Some(self.primary());
        };
        let authority = authority.to_str().ok()?;
        self.active
            .read()
            .expect("public endpoint lock poisoned")
            .iter()
            .find(|origin| authority_matches(origin, authority))
            .cloned()
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
        trusted.contains(&normalize(left)) && active.contains(&normalize(right))
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

fn authority_matches(origin: &str, authority: &str) -> bool {
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
