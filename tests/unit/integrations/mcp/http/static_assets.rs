use super::*;

#[tokio::test]
async fn observatory_shell_and_static_assets_are_served_without_api_authorization() {
    let page = intelligence_page().await;
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(page.headers()[header::CACHE_CONTROL], "no-store");
    assert!(page.headers()[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .contains("script-src 'self'"));
    let page_body = to_bytes(page.into_body(), 64 * 1024).await.unwrap();
    let page_body = std::str::from_utf8(&page_body).unwrap();
    assert!(page_body.contains("/intelligence/app.js"));
    assert!(page_body.contains("/intelligence/app.css"));

    let script = intelligence_script().await;
    assert_eq!(script.status(), StatusCode::OK);
    assert_eq!(
        script.headers()[header::CONTENT_TYPE],
        "text/javascript; charset=utf-8"
    );
    let expected_script = crate::intelligence_web::INTELLIGENCE_JS;
    let script_body = to_bytes(script.into_body(), expected_script.len())
        .await
        .unwrap();
    let script_body = std::str::from_utf8(&script_body).unwrap();
    assert_eq!(script_body, expected_script);
    assert!(script_body.contains("function startObservatory()"));
    assert!(script_body.contains("applyTheme();"));

    let styles = intelligence_styles().await;
    assert_eq!(styles.status(), StatusCode::OK);
    assert_eq!(
        styles.headers()[header::CONTENT_TYPE],
        "text/css; charset=utf-8"
    );
    let style_body = to_bytes(styles.into_body(), 512 * 1024).await.unwrap();
    assert!(!style_body.is_empty());

    let logo = intelligence_logo().await;
    assert_eq!(logo.status(), StatusCode::OK);
    assert_eq!(
        logo.headers()[header::CONTENT_TYPE],
        "image/svg+xml; charset=utf-8"
    );
}
