#![cfg(test)]

use std::path::Path;

use reqwest::Client;
use tempfile::TempDir;
use tokio::fs;
use tokio::sync::mpsc::channel;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::utils::test_utils;

use super::{DownloadProgress, download_file};

#[derive(Debug)]
struct FixtureCertificate(std::sync::Arc<rustls::sign::CertifiedKey>);

impl rustls::server::ResolvesServerCert for FixtureCertificate {
    fn resolve(
        &self,
        _client_hello: rustls::server::ClientHello<'_>,
    ) -> Option<std::sync::Arc<rustls::sign::CertifiedKey>> {
        Some(self.0.clone())
    }
}

async fn tls_fixture_request(
    certificates: Vec<rustls::pki_types::CertificateDer<'static>>,
    root: &[u8],
    hostname: &str,
) -> Result<reqwest::Response, reqwest::Error> {
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let key = rustls::pki_types::PrivateKeyDer::try_from(
        include_bytes!("../../../tests/fixtures/tls/localhost-key.der").to_vec(),
    )
    .unwrap();
    let key = rustls::crypto::ring::sign::any_supported_type(&key).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_cert_resolver(Arc::new(FixtureCertificate(Arc::new(
        rustls::sign::CertifiedKey::new(certificates, key),
    ))));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        if let Ok(mut stream) = tokio_rustls::TlsAcceptor::from(Arc::new(config))
            .accept(stream)
            .await
        {
            let mut request = [0u8; 4096];
            stream.read(&mut request).await.unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
        }
    });
    let result = Client::builder()
        .no_proxy()
        .tls_built_in_root_certs(false)
        .add_root_certificate(reqwest::Certificate::from_der(root).unwrap())
        .resolve(hostname, address)
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap()
        .get(format!("https://{hostname}:{}/", address.port()))
        .send()
        .await;
    server.await.unwrap();
    result
}

#[tokio::test]
async fn test_https_preserves_custom_trust_and_downloads() {
    let response = tls_fixture_request(
        vec![rustls::pki_types::CertificateDer::from(
            include_bytes!("../../../tests/fixtures/tls/localhost.der").to_vec(),
        )],
        include_bytes!("../../../tests/fixtures/tls/localhost-ca.der"),
        "localhost",
    )
    .await
    .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "ok");
}

#[tokio::test]
async fn test_https_rejects_compromised_emudhra_before_handshake_signature() {
    let result = tls_fixture_request(
        vec![
            rustls::pki_types::CertificateDer::from(
                include_bytes!("../../../tests/fixtures/tls/emudhra-leaf.der").to_vec(),
            ),
            rustls::pki_types::CertificateDer::from(
                include_bytes!("../../../tests/fixtures/tls/emudhra-issuer.der").to_vec(),
            ),
            rustls::pki_types::CertificateDer::from(
                include_bytes!("../../../tests/fixtures/tls/emudhra-cross-signed-root.der")
                    .to_vec(),
            ),
        ],
        include_bytes!("../../../tests/fixtures/tls/emudhra-root.der"),
        "dv-g3b-emudhra.we-have-a-problem.com",
    )
    .await;
    let error = format!("{:?}", result.unwrap_err());
    assert!(error.contains("CertDistrusted"), "{error}");
}

#[tokio::test]
async fn test_download_rejects_unsafe_filenames() {
    let owned = TempDir::new().unwrap();
    let downloads = owned.path().join("downloads");
    fs::create_dir(&downloads).await.unwrap();
    let sentinel = owned.path().join("outside.txt");
    fs::write(&sentinel, b"unchanged").await.unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("download"))
        .mount(&server)
        .await;

    for filename in [
        "../outside.txt",
        "..\\outside.txt",
        "file.zip:stream",
        ".",
        "",
    ] {
        let (tx, _rx) = channel(4);
        let result = download_file(
            Client::new().get(server.uri()),
            &downloads,
            Some(filename),
            tx,
        )
        .await;
        assert!(result.is_err(), "accepted unsafe filename {filename:?}");
        assert_eq!(fs::read(&sentinel).await.unwrap(), b"unchanged");
    }

    let (tx, _rx) = channel(4);
    let result = download_file(
        Client::new().get(format!("{}/%2E%2E%2Foutside.txt", server.uri())),
        &downloads,
        None,
        tx,
    )
    .await;
    assert!(result.is_err());
    assert_eq!(fs::read(&sentinel).await.unwrap(), b"unchanged");

    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/header"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("download")
                .insert_header(
                    "Content-Disposition",
                    "attachment; filename=\"../outside.txt\"",
                ),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    let (tx, _rx) = channel(4);
    let result = download_file(
        Client::new().get(format!("{}/header", server.uri())),
        &downloads,
        None,
        tx,
    )
    .await;
    assert!(result.is_err());
    assert_eq!(fs::read(&sentinel).await.unwrap(), b"unchanged");
    assert_eq!(std::fs::read_dir(&downloads).unwrap().count(), 0);
}

#[cfg(unix)]
#[tokio::test]
async fn test_download_replaces_symlink_without_writing_its_target() {
    let owned = TempDir::new().unwrap();
    let downloads = owned.path().join("downloads");
    fs::create_dir(&downloads).await.unwrap();
    let sentinel = owned.path().join("outside.txt");
    fs::write(&sentinel, b"unchanged").await.unwrap();
    std::os::unix::fs::symlink(&sentinel, downloads.join("mod.zip")).unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("download"))
        .mount(&server)
        .await;
    let (tx, _rx) = channel(4);
    let result = download_file(
        Client::new().get(server.uri()),
        &downloads,
        Some("mod.zip"),
        tx,
    )
    .await
    .unwrap();
    assert_eq!(result, downloads.join("mod.zip"));
    assert_eq!(fs::read(&sentinel).await.unwrap(), b"unchanged");
    assert_eq!(fs::read(&result).await.unwrap(), b"download");
    assert!(!fs::symlink_metadata(&result).await.unwrap().is_symlink());
}

#[tokio::test]
pub async fn test_download_file_with_given_filename() {
    test_utils::setup_stdout_logger();
    let tmp_dir = TempDir::new().unwrap();
    let mock_server = MockServer::start().await;
    let (tx, mut rx) = channel::<DownloadProgress>(3);

    let fixture_path = Path::new("tests/fixtures/test.zip");
    let fixture_data = fs::read(fixture_path).await.unwrap();
    let fixture_len = fixture_data.len() as u64;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture_data.clone()))
        .mount(&mock_server)
        .await;

    let client = Client::new();
    let url = mock_server.uri();
    let request = client.get(url);

    let expected_download_path = tmp_dir.path().join("test.zip");

    let result = download_file(request, tmp_dir.path(), Some("test.zip"), tx).await;

    assert!(result.is_ok(), "{}", result.unwrap_err());
    let download_path = result.unwrap();
    assert_eq!(download_path, expected_download_path);

    let downloaded_data = fs::read(&download_path).await.unwrap();
    assert_eq!(downloaded_data, fixture_data);
    assert_eq!(downloaded_data.len() as u64, fixture_len);

    let first_progress = rx.recv().await.unwrap();
    assert_eq!(first_progress.downloaded_bytes, 0);
    assert_eq!(first_progress.total_bytes, fixture_len);

    let mut last_progress = DownloadProgress {
        downloaded_bytes: 0,
        total_bytes: 0,
    };

    while let Some(progress) = rx.recv().await {
        last_progress = progress;
    }

    assert_eq!(last_progress.downloaded_bytes, fixture_len);
    assert_eq!(last_progress.total_bytes, fixture_len);
}

#[tokio::test]
pub async fn test_download_file_with_filename_from_url() {
    test_utils::setup_stdout_logger();
    let tmp_dir = TempDir::new().unwrap();
    let mock_server = MockServer::start().await;
    let (tx, mut rx) = channel::<DownloadProgress>(3);

    let fixture_path = Path::new("tests/fixtures/test.zip");
    let fixture_data = fs::read(fixture_path).await.unwrap();
    let fixture_len = fixture_data.len() as u64;

    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/test-from-url.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture_data.clone()))
        .mount(&mock_server)
        .await;

    let client = Client::new();
    let url = mock_server.uri() + "/test-from-url.zip";
    let request = client.get(url);

    let expected_download_path = tmp_dir.path().join("test-from-url.zip");

    let result = download_file(request, tmp_dir.path(), None, tx).await;

    assert!(result.is_ok(), "{}", result.unwrap_err());
    let download_path = result.unwrap();
    assert_eq!(download_path, expected_download_path);

    let downloaded_data = fs::read(&download_path).await.unwrap();
    assert_eq!(downloaded_data, fixture_data);
    assert_eq!(downloaded_data.len() as u64, fixture_len);

    let first_progress = rx.recv().await.unwrap();
    assert_eq!(first_progress.downloaded_bytes, 0);
    assert_eq!(first_progress.total_bytes, fixture_len);

    let mut last_progress = DownloadProgress {
        downloaded_bytes: 0,
        total_bytes: 0,
    };

    while let Some(progress) = rx.recv().await {
        last_progress = progress;
    }

    assert_eq!(last_progress.downloaded_bytes, fixture_len);
    assert_eq!(last_progress.total_bytes, fixture_len);
}

#[tokio::test]
pub async fn test_download_file_with_filename_from_content_disposition() {
    test_utils::setup_stdout_logger();
    let tmp_dir = TempDir::new().unwrap();
    let mock_server = MockServer::start().await;
    let (tx, mut rx) = channel::<DownloadProgress>(3);

    let fixture_path = Path::new("tests/fixtures/test.zip");
    let fixture_data = fs::read(fixture_path).await.unwrap();
    let fixture_len = fixture_data.len() as u64;

    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/test-from-url.zip"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(fixture_data.clone())
                .insert_header(
                    "Content-Disposition",
                    "attachment; filename=\"test-from-header.zip\"",
                ),
        )
        .mount(&mock_server)
        .await;

    let client = Client::new();
    let url = mock_server.uri() + "/test-from-url.zip";
    let request = client.get(url);

    let expected_download_path = tmp_dir.path().join("test-from-header.zip");

    let result = download_file(request, tmp_dir.path(), None, tx).await;

    assert!(result.is_ok(), "{}", result.unwrap_err());
    let download_path = result.unwrap();
    assert_eq!(download_path, expected_download_path);

    let downloaded_data = fs::read(&download_path).await.unwrap();
    assert_eq!(downloaded_data, fixture_data);
    assert_eq!(downloaded_data.len() as u64, fixture_len);

    let first_progress = rx.recv().await.unwrap();
    assert_eq!(first_progress.downloaded_bytes, 0);
    assert_eq!(first_progress.total_bytes, fixture_len);

    let mut last_progress = DownloadProgress {
        downloaded_bytes: 0,
        total_bytes: 0,
    };

    while let Some(progress) = rx.recv().await {
        last_progress = progress;
    }

    assert_eq!(last_progress.downloaded_bytes, fixture_len);
    assert_eq!(last_progress.total_bytes, fixture_len);
}
