use super::url::{
    sanitize_download_name, GOOGLE_DOCS_MIME, GOOGLE_DRAWINGS_MIME, GOOGLE_SHEETS_MIME,
    GOOGLE_SLIDES_MIME,
};
use super::*;
use crate::error::DriveError;

#[test]
fn parse_google_doc_url() {
    let result =
        parse_google_url("https://docs.google.com/document/d/1aBcDeFgHiJkLmNoPqRsTuVwXyZ/edit")
            .unwrap();
    assert_eq!(result.file_id, "1aBcDeFgHiJkLmNoPqRsTuVwXyZ");
    assert_eq!(result.kind, GoogleFileKind::Document);
}

#[test]
fn parse_google_sheet_url() {
    let result = parse_google_url(
        "https://docs.google.com/spreadsheets/d/1aBcDeFgHiJkLmNoPqRsTuVwXyZ/edit#gid=0",
    )
    .unwrap();
    assert_eq!(result.file_id, "1aBcDeFgHiJkLmNoPqRsTuVwXyZ");
    assert_eq!(result.kind, GoogleFileKind::Spreadsheet);
}

#[test]
fn parse_google_slides_url() {
    let result =
        parse_google_url("https://docs.google.com/presentation/d/1aBcDeFgHiJkLmNoPqRsTuVwXyZ/edit")
            .unwrap();
    assert_eq!(result.file_id, "1aBcDeFgHiJkLmNoPqRsTuVwXyZ");
    assert_eq!(result.kind, GoogleFileKind::Presentation);
}

#[test]
fn parse_google_drawing_url() {
    let result =
        parse_google_url("https://docs.google.com/drawings/d/1aBcDeFgHiJkLmNoPqRsTuVwXyZ/edit")
            .unwrap();
    assert_eq!(result.file_id, "1aBcDeFgHiJkLmNoPqRsTuVwXyZ");
    assert_eq!(result.kind, GoogleFileKind::Drawing);
}

#[test]
fn parse_drive_file_url() {
    let result = parse_google_url(
        "https://drive.google.com/file/d/1aBcDeFgHiJkLmNoPqRsTuVwXyZ/view?usp=sharing",
    )
    .unwrap();
    assert_eq!(result.file_id, "1aBcDeFgHiJkLmNoPqRsTuVwXyZ");
    assert_eq!(result.kind, GoogleFileKind::Drive);
}

#[test]
fn parse_drive_open_url() {
    let result =
        parse_google_url("https://drive.google.com/open?id=1aBcDeFgHiJkLmNoPqRsTuVwXyZ").unwrap();
    assert_eq!(result.file_id, "1aBcDeFgHiJkLmNoPqRsTuVwXyZ");
    assert_eq!(result.kind, GoogleFileKind::Drive);
}

#[test]
fn parse_invalid_url_fails() {
    assert!(parse_google_url("https://example.com/foo").is_err());
    assert!(parse_google_url("not a url").is_err());
}

#[test]
fn parse_incomplete_drive_url_fails() {
    assert!(parse_google_url("https://drive.google.com/file/d/").is_err());
}

#[test]
fn export_format_from_name_roundtrip() {
    for name in [
        "txt", "md", "pdf", "docx", "csv", "xlsx", "pptx", "png", "svg",
    ] {
        assert!(
            ExportFormat::from_name(name).is_some(),
            "failed for: {name}"
        );
    }
    assert!(ExportFormat::from_name("unknown").is_none());
}

#[test]
fn default_formats_for_docs() {
    let (text, bin) = default_export_formats(GOOGLE_DOCS_MIME);
    assert_eq!(text, ExportFormat::PlainText);
    assert_eq!(bin, ExportFormat::Pdf);
}

#[test]
fn default_formats_for_sheets() {
    let (text, bin) = default_export_formats(GOOGLE_SHEETS_MIME);
    assert_eq!(text, ExportFormat::Csv);
    assert_eq!(bin, ExportFormat::Xlsx);
}

#[test]
fn is_google_native() {
    assert!(is_google_native_mime(
        "application/vnd.google-apps.document"
    ));
    assert!(is_google_native_mime(
        "application/vnd.google-apps.spreadsheet"
    ));
    assert!(!is_google_native_mime("application/pdf"));
    assert!(!is_google_native_mime("text/plain"));
}

#[test]
fn save_to_disk_generates_correct_name() {
    let result = DownloadResult {
        file_name: "My Document".to_string(),
        mime_type: "text/plain".to_string(),
        data: b"hello world".to_vec(),
        export_format: Some(ExportFormat::PlainText),
    };
    let dir = std::env::temp_dir().join(format!("void-gdrive-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let dest = DriveApiClient::save_to_disk(&result, None).unwrap();
    assert_eq!(
        dest.file_name().unwrap().to_str().unwrap(),
        "My Document.txt"
    );
    std::fs::remove_file(&dest).ok();
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn api_get_metadata() {
    let mock_server = wiremock::MockServer::start().await;

    let body = r#"{
        "id": "abc123",
        "name": "Test Doc",
        "mimeType": "application/vnd.google-apps.document",
        "size": null
    }"#;

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/abc123"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(body))
        .mount(&mock_server)
        .await;

    let api = DriveApiClient::with_base_url("test-token", &mock_server.uri());
    let meta = api.get_file_metadata("abc123").await.unwrap();
    assert_eq!(meta.name, "Test Doc");
    assert_eq!(meta.mime_type, "application/vnd.google-apps.document");
}

#[tokio::test]
async fn api_download_file() {
    let mock_server = wiremock::MockServer::start().await;

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/bin123"))
        .and(wiremock::matchers::query_param("alt", "media"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(b"file content here"))
        .mount(&mock_server)
        .await;

    let api = DriveApiClient::with_base_url("test-token", &mock_server.uri());
    let data = api.download_file("bin123").await.unwrap();
    assert_eq!(data, b"file content here");
}

#[tokio::test]
async fn api_export_file() {
    let mock_server = wiremock::MockServer::start().await;

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/doc123/export"))
        .and(wiremock::matchers::query_param("mimeType", "text/plain"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_string("exported plain text content"),
        )
        .mount(&mock_server)
        .await;

    let api = DriveApiClient::with_base_url("test-token", &mock_server.uri());
    let data = api.export_file("doc123", "text/plain").await.unwrap();
    assert_eq!(
        String::from_utf8(data).unwrap(),
        "exported plain text content"
    );
}

#[tokio::test]
async fn fetch_file_exports_google_native() {
    let mock_server = wiremock::MockServer::start().await;

    let meta_body = r#"{
        "id": "doc456",
        "name": "My Spreadsheet",
        "mimeType": "application/vnd.google-apps.spreadsheet"
    }"#;

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/doc456"))
        .and(wiremock::matchers::query_param(
            "fields",
            "id,name,mimeType,size",
        ))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(meta_body))
        .mount(&mock_server)
        .await;

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/doc456/export"))
        .and(wiremock::matchers::query_param("mimeType", "text/csv"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("col1,col2\na,b"))
        .mount(&mock_server)
        .await;

    let api = DriveApiClient::with_base_url("test-token", &mock_server.uri());
    let result = api.fetch_file("doc456", None).await.unwrap();
    assert_eq!(result.file_name, "My Spreadsheet");
    assert_eq!(result.export_format, Some(ExportFormat::Csv));
    assert_eq!(String::from_utf8(result.data).unwrap(), "col1,col2\na,b");
}

#[test]
fn default_formats_for_slides() {
    let (text, bin) = default_export_formats(GOOGLE_SLIDES_MIME);
    assert_eq!(text, ExportFormat::PlainText);
    assert_eq!(bin, ExportFormat::Pdf);
}

#[test]
fn default_formats_for_drawings() {
    let (text, bin) = default_export_formats(GOOGLE_DRAWINGS_MIME);
    assert_eq!(text, ExportFormat::Svg);
    assert_eq!(bin, ExportFormat::Pdf);
}

#[test]
fn default_formats_for_unknown_mime() {
    let (text, bin) = default_export_formats("application/vnd.google-apps.form");
    assert_eq!(text, ExportFormat::PlainText);
    assert_eq!(bin, ExportFormat::Pdf);
}

#[test]
fn export_format_mime_extension_roundtrip() {
    for fmt in [
        ExportFormat::PlainText,
        ExportFormat::Markdown,
        ExportFormat::Pdf,
        ExportFormat::Docx,
        ExportFormat::Csv,
        ExportFormat::Xlsx,
        ExportFormat::Pptx,
        ExportFormat::Png,
        ExportFormat::Svg,
    ] {
        // Extension is parseable back into the same format.
        assert_eq!(
            ExportFormat::from_name(fmt.extension()),
            Some(fmt),
            "extension roundtrip failed for {fmt:?}"
        );
        // MIME type is non-empty and distinct.
        assert!(!fmt.mime_type().is_empty());
    }
}

#[test]
fn save_to_disk_nested_output_path() {
    let dir = std::env::temp_dir().join(format!("void-gdrive-test-{}", uuid::Uuid::new_v4()));
    let nested = dir.join("a").join("b").join("out.csv");
    let result = DownloadResult {
        file_name: "ignored.csv".to_string(),
        mime_type: "text/csv".to_string(),
        data: b"x,y\n1,2".to_vec(),
        export_format: Some(ExportFormat::Csv),
    };
    let dest = DriveApiClient::save_to_disk(&result, Some(&nested)).unwrap();
    assert_eq!(dest, nested);
    assert_eq!(std::fs::read(&dest).unwrap(), b"x,y\n1,2");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn sanitize_download_name_strips_traversal() {
    assert_eq!(sanitize_download_name("../../../.zshrc"), ".zshrc");
    assert_eq!(sanitize_download_name("/etc/passwd"), "passwd");
    assert_eq!(sanitize_download_name("a/b/c.txt"), "c.txt");
    assert_eq!(sanitize_download_name("..\\..\\win.ini"), "win.ini");
    assert_eq!(sanitize_download_name(".."), "download.bin");
    assert_eq!(sanitize_download_name(""), "download.bin");
    assert_eq!(sanitize_download_name("evil\u{0}name"), "evilname");
    assert_eq!(sanitize_download_name("report.pdf"), "report.pdf");
}

#[test]
fn save_to_disk_auto_name_cannot_escape_cwd() {
    let result = DownloadResult {
        file_name: "../../../../tmp/void-traversal-probe".to_string(),
        mime_type: "application/octet-stream".to_string(),
        data: b"x".to_vec(),
        export_format: None,
    };
    let dest = DriveApiClient::save_to_disk(&result, None).unwrap();
    // Reduced to a single component — no parent, stays in cwd.
    assert_eq!(dest, std::path::PathBuf::from("void-traversal-probe"));
    std::fs::remove_file(&dest).ok();
}

#[tokio::test]
async fn get_metadata_unauthorized_errors() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/x"))
        .respond_with(
            wiremock::ResponseTemplate::new(401)
                .set_body_string(r#"{"error":{"message":"Invalid Credentials"}}"#),
        )
        .mount(&server)
        .await;

    let api = DriveApiClient::with_base_url("tok", &server.uri());
    let err = api.get_file_metadata("x").await.unwrap_err();
    let de = err.downcast::<DriveError>().unwrap();
    assert!(matches!(de, DriveError::Api(_)));
    assert!(de.to_string().contains("401"));
}

#[tokio::test]
async fn get_metadata_not_found_errors() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/missing"))
        .respond_with(
            wiremock::ResponseTemplate::new(404)
                .set_body_string(r#"{"error":{"message":"File not found: missing."}}"#),
        )
        .mount(&server)
        .await;

    let api = DriveApiClient::with_base_url("tok", &server.uri());
    let err = api.get_file_metadata("missing").await.unwrap_err();
    let de = err.downcast::<DriveError>().unwrap();
    assert!(matches!(de, DriveError::Api(_)));
    assert!(de.to_string().contains("File not found"));
}

#[tokio::test]
async fn get_metadata_server_error_errors() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/boom"))
        .respond_with(wiremock::ResponseTemplate::new(500).set_body_string("internal error"))
        .mount(&server)
        .await;

    let api = DriveApiClient::with_base_url("tok", &server.uri());
    let err = api.get_file_metadata("boom").await.unwrap_err();
    let de = err.downcast::<DriveError>().unwrap();
    assert!(matches!(de, DriveError::Api(_)));
    // Non-JSON body falls back to the raw text.
    assert!(de.to_string().contains("internal error"));
}

#[tokio::test]
async fn get_metadata_malformed_json_on_200_errors() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/bad"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("{not json"))
        .mount(&server)
        .await;

    let api = DriveApiClient::with_base_url("tok", &server.uri());
    // JSON decode failure surfaces as a reqwest error (not a DriveError).
    assert!(api.get_file_metadata("bad").await.is_err());
}

#[tokio::test]
async fn check_response_insufficient_scopes_maps_to_auth() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/scoped"))
        .respond_with(wiremock::ResponseTemplate::new(403).set_body_string(
            r#"{"error":{"message":"Request had insufficient authentication scopes."}}"#,
        ))
        .mount(&server)
        .await;

    let api = DriveApiClient::with_base_url("tok", &server.uri());
    let err = api.get_file_metadata("scoped").await.unwrap_err();
    let de = err.downcast::<DriveError>().unwrap();
    assert!(matches!(de, DriveError::Auth(_)));
    assert!(de.to_string().contains("Drive scopes"));
}

#[tokio::test]
async fn check_response_api_not_enabled_maps_to_auth() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/disabled"))
        .respond_with(wiremock::ResponseTemplate::new(403).set_body_string(
            r#"{"error":{"message":"Google Drive API has not been used in project 123 before or it is disabled."}}"#,
        ))
        .mount(&server)
        .await;

    let api = DriveApiClient::with_base_url("tok", &server.uri());
    let err = api.get_file_metadata("disabled").await.unwrap_err();
    let de = err.downcast::<DriveError>().unwrap();
    assert!(matches!(de, DriveError::Auth(_)));
    assert!(de.to_string().contains("not enabled"));
}

#[tokio::test]
async fn export_file_server_error_errors() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/exp/export"))
        .respond_with(wiremock::ResponseTemplate::new(500).set_body_string("export boom"))
        .mount(&server)
        .await;

    let api = DriveApiClient::with_base_url("tok", &server.uri());
    let err = api.export_file("exp", "text/plain").await.unwrap_err();
    let de = err.downcast::<DriveError>().unwrap();
    assert!(matches!(de, DriveError::Api(_)));
}

#[tokio::test]
async fn download_file_not_found_errors() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/nope"))
        .and(wiremock::matchers::query_param("alt", "media"))
        .respond_with(
            wiremock::ResponseTemplate::new(404)
                .set_body_string(r#"{"error":{"message":"Not Found"}}"#),
        )
        .mount(&server)
        .await;

    let api = DriveApiClient::with_base_url("tok", &server.uri());
    let err = api.download_file("nope").await.unwrap_err();
    let de = err.downcast::<DriveError>().unwrap();
    assert!(matches!(de, DriveError::Api(_)));
}

#[tokio::test]
async fn fetch_file_propagates_export_error() {
    let server = wiremock::MockServer::start().await;
    let meta_body = r#"{
        "id": "doc1",
        "name": "Doc One",
        "mimeType": "application/vnd.google-apps.document"
    }"#;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/doc1"))
        .and(wiremock::matchers::query_param(
            "fields",
            "id,name,mimeType,size",
        ))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(meta_body))
        .mount(&server)
        .await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/doc1/export"))
        .respond_with(wiremock::ResponseTemplate::new(500).set_body_string("export failed"))
        .mount(&server)
        .await;

    let api = DriveApiClient::with_base_url("tok", &server.uri());
    let err = api.fetch_file("doc1", None).await.unwrap_err();
    let de = err.downcast::<DriveError>().unwrap();
    assert!(matches!(de, DriveError::Api(_)));
}

#[tokio::test]
async fn fetch_file_honors_explicit_export_format() {
    let server = wiremock::MockServer::start().await;
    let meta_body = r#"{
        "id": "doc2",
        "name": "Doc Two",
        "mimeType": "application/vnd.google-apps.document"
    }"#;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/doc2"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(meta_body))
        .mount(&server)
        .await;
    // Only matches when mimeType=application/pdf is requested.
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/doc2/export"))
        .and(wiremock::matchers::query_param(
            "mimeType",
            "application/pdf",
        ))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(b"%PDF-1.4"))
        .mount(&server)
        .await;

    let api = DriveApiClient::with_base_url("tok", &server.uri());
    let result = api
        .fetch_file("doc2", Some(ExportFormat::Pdf))
        .await
        .unwrap();
    assert_eq!(result.export_format, Some(ExportFormat::Pdf));
    assert_eq!(result.mime_type, "application/pdf");
    assert_eq!(result.data, b"%PDF-1.4");
}

#[tokio::test]
async fn fetch_file_rejects_format_on_binary() {
    let server = wiremock::MockServer::start().await;
    let meta_body = r#"{
        "id": "bin1",
        "name": "photo.png",
        "mimeType": "image/png",
        "size": "2048"
    }"#;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/bin1"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(meta_body))
        .mount(&server)
        .await;

    let api = DriveApiClient::with_base_url("tok", &server.uri());
    let err = api
        .fetch_file("bin1", Some(ExportFormat::Pdf))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not a Google-native format"));
}

#[tokio::test]
async fn fetch_file_downloads_binary() {
    let mock_server = wiremock::MockServer::start().await;

    let meta_body = r#"{
        "id": "pdf789",
        "name": "Report.pdf",
        "mimeType": "application/pdf",
        "size": "1024"
    }"#;

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/pdf789"))
        .and(wiremock::matchers::query_param(
            "fields",
            "id,name,mimeType,size",
        ))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(meta_body))
        .mount(&mock_server)
        .await;

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/drive/v3/files/pdf789"))
        .and(wiremock::matchers::query_param("alt", "media"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(b"pdf-binary-content"))
        .mount(&mock_server)
        .await;

    let api = DriveApiClient::with_base_url("test-token", &mock_server.uri());
    let result = api.fetch_file("pdf789", None).await.unwrap();
    assert_eq!(result.file_name, "Report.pdf");
    assert!(result.export_format.is_none());
    assert_eq!(result.data, b"pdf-binary-content");
}
