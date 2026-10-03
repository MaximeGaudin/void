use super::*;

#[test]
fn value_roundtrip() {
    let enc = encrypt_value("user_token", "xoxp-secret").unwrap();
    assert!(is_encrypted_value(&enc));
    assert!(!enc.contains("xoxp-secret"));
    assert_eq!(decrypt_value("user_token", &enc).unwrap(), "xoxp-secret");
}

#[test]
fn value_nonce_is_random() {
    let a = encrypt_value("token", "same").unwrap();
    let b = encrypt_value("token", "same").unwrap();
    assert_ne!(a, b);
}

#[test]
fn value_is_bound_to_field() {
    let enc = encrypt_value("app_token", "xapp-1").unwrap();
    assert!(matches!(
        decrypt_value("user_token", &enc),
        Err(SecretError::Decrypt)
    ));
}

#[test]
fn tampered_value_is_rejected() {
    let enc = encrypt_value("token", "ghp_abc").unwrap();
    let mut raw = B64.decode(enc.strip_prefix(ENC_PREFIX).unwrap()).unwrap();
    let last = raw.len() - 1;
    raw[last] ^= 1;
    let tampered = format!("{ENC_PREFIX}{}", B64.encode(raw));
    assert!(matches!(
        decrypt_value("token", &tampered),
        Err(SecretError::Decrypt)
    ));
}

#[test]
fn malformed_value_is_rejected() {
    assert!(matches!(
        decrypt_value("token", "enc:v1:!!!"),
        Err(SecretError::Malformed)
    ));
    assert!(matches!(
        decrypt_value("token", "enc:v1:AAAA"),
        Err(SecretError::Malformed)
    ));
    assert!(matches!(
        decrypt_value("token", "plain"),
        Err(SecretError::Malformed)
    ));
}

#[test]
fn secret_fields_cover_known_credentials() {
    for f in [
        "app_token",
        "user_token",
        "client_secret",
        "api_key",
        "token",
    ] {
        assert!(is_secret_field(f), "{f}");
    }
    for f in [
        "id",
        "app_id",
        "client_id",
        "username",
        "dsn",
        "credentials_file",
    ] {
        assert!(!is_secret_field(f), "{f}");
    }
}

#[test]
fn file_roundtrip_and_plaintext_never_hits_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gmail-token.json");
    write_secret_file(&path, br#"{"access_token":"ya29.secret"}"#).unwrap();

    let raw = std::fs::read(&path).unwrap();
    assert!(is_encrypted_file(&raw));
    assert!(!String::from_utf8_lossy(&raw).contains("ya29.secret"));

    let file = read_secret_file(&path).unwrap();
    assert!(!file.was_plaintext);
    assert_eq!(file.contents, br#"{"access_token":"ya29.secret"}"#);
}

#[test]
fn legacy_plaintext_file_is_read_and_migrated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("telegram.json");
    std::fs::write(&path, b"{\"auth\":\"k\"}").unwrap();

    let file = read_secret_file(&path).unwrap();
    assert!(file.was_plaintext);

    let contents = read_and_migrate_secret_file(&path).unwrap();
    assert_eq!(contents, b"{\"auth\":\"k\"}");
    assert!(is_encrypted_file(&std::fs::read(&path).unwrap()));
}

#[test]
fn tampered_file_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.json");
    write_secret_file(&path, b"payload").unwrap();
    let mut raw = std::fs::read(&path).unwrap();
    let last = raw.len() - 1;
    raw[last] ^= 1;
    std::fs::write(&path, raw).unwrap();
    assert!(read_secret_file(&path).is_err());
}
