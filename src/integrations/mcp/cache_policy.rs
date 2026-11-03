//! Cache policy for protected intelligence responses.
use axum::extract::Request;
use axum::http::header;
use axum::middleware::Next;
use axum::response::Response;

/// Every `/intelligence/*` response carries protected project data. Handlers
/// set `Cache-Control: no-store` individually; this guarantees the header even
/// for a handler that forgets it, without overriding an explicit policy such as
/// the static asset cache lifetime.
pub(super) async fn intelligence_no_store(request: Request, next: Next) -> Response {
    let protected = request.uri().path().starts_with("/intelligence/");
    let mut response = next.run(request).await;
    if protected && !response.headers().contains_key(header::CACHE_CONTROL) {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    }
    response
}
