use super::*;

#[path = "credential_fixture.rs"]
mod fixture_keys;

#[test]
fn github_credentials_file_mode_fails_closed_but_inline_pem_is_portable() {
    let error = Credentials::app(56, Path::new("never-opened.pem"))
        .err()
        .unwrap();
    assert!(error
        .to_string()
        .contains("requires Unix private-file validation"));
    let pem = format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        fixture_keys::A_PKCS8
    );
    assert!(Credentials::app_pem(56, pem.as_bytes()).is_ok());
    assert!(Credentials::fixed("OFFLINE-INSTALLATION-TOKEN").is_ok());
}
