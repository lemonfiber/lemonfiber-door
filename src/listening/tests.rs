use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt as _, Empty};
use hyper_util::rt::{TokioExecutor, TokioIo};
use lemonfiber_ports::service::Guarding;
use rustls::pki_types::{CertificateDer, ServerName};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

use super::{serve, taken};
use crate::fake;
use crate::fixtures::{self, Scratch, ITEM};
use crate::holding::Holding;
use crate::serving::Door;
use crate::tls::{self, Certificates};

async fn listener() -> (TcpListener, std::net::SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|why| unreachable!("bound: {why}"));
    let at = listener
        .local_addr()
        .unwrap_or_else(|why| unreachable!("addressed: {why}"));
    (listener, at)
}

#[tokio::test]
async fn the_encrypted_port_serves_http2_with_the_certificate_written() {
    let upstream = fake::media_server().await;
    let scratch = Scratch::new();
    let der = tls::tests::written(scratch.path());
    let certificates = Arc::new(Certificates::in_directory(scratch.path()));
    let holding = Arc::new(Holding::in_directory(
        scratch.path(),
        Arc::clone(&certificates),
    ));
    let mut guarded = fixtures::guarded(fixtures::declared());
    guarded.upstream = upstream.address();
    assert!(holding.guard(&guarded).await.is_ok());
    let served = tls::served(certificates).unwrap_or_else(|why| unreachable!("{why}"));
    let (listener, at) = listener().await;
    tokio::spawn(serve(
        listener,
        Arc::new(Door::new(holding)),
        Some(TlsAcceptor::from(served)),
        Box::pin(std::future::pending()),
    ));
    let mut roots = rustls::RootCertStore::empty();
    let _ = roots.add(CertificateDer::from(der));
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap_or_else(|why| unreachable!("{why}"))
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec()];
    let tcp = TcpStream::connect(at)
        .await
        .unwrap_or_else(|why| unreachable!("connected: {why}"));
    let name = ServerName::try_from("localhost").unwrap_or_else(|why| unreachable!("{why}"));
    let encrypted = TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await
        .unwrap_or_else(|why| unreachable!("handshake: {why}"));
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(encrypted))
            .await
            .unwrap_or_else(|why| unreachable!("h2: {why}"));
    tokio::spawn(connection);
    let request = Request::builder()
        .uri(format!("https://media.home.local/Videos/{ITEM}/stream"))
        .body(Empty::<Bytes>::new())
        .unwrap_or_else(|why| unreachable!("{why}"));
    let answer = sender
        .send_request(request)
        .await
        .unwrap_or_else(|why| unreachable!("answered: {why}"));
    assert_eq!(answer.status().as_u16(), 200);
    let _ = answer.into_body().collect().await;
    drop(sender);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let forwarded = upstream.sent().pop();
    assert_eq!(
        forwarded
            .as_ref()
            .map(|sent| (sent.header("host"), sent.header("x-forwarded-proto"))),
        Some((
            vec!["media.home.local".to_owned()],
            vec!["https".to_owned()]
        ))
    );
}

#[tokio::test]
async fn a_handshake_that_fails_closes_only_that_connection() {
    let scratch = Scratch::new();
    let certificates = Arc::new(Certificates::in_directory(scratch.path()));
    let holding = Arc::new(Holding::in_directory(
        scratch.path(),
        Arc::clone(&certificates),
    ));
    let served = tls::served(certificates).unwrap_or_else(|why| unreachable!("{why}"));
    let (listener, at) = listener().await;
    tokio::spawn(serve(
        listener,
        Arc::new(Door::new(holding)),
        Some(TlsAcceptor::from(served)),
        Box::pin(std::future::pending()),
    ));
    for _ in 0..2 {
        let mut stream = TcpStream::connect(at)
            .await
            .unwrap_or_else(|why| unreachable!("connected: {why}"));
        let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, b"GET / HTTP/1.1\r\n\r\n").await;
        let mut read = Vec::new();
        let _ = tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut read).await;
        assert!(!String::from_utf8_lossy(&read).contains("HTTP/1.1"));
    }
}

#[tokio::test]
async fn serving_ends_once_stopped() {
    let scratch = Scratch::new();
    let holding = Arc::new(Holding::in_directory(
        scratch.path(),
        Arc::new(Certificates::in_directory(scratch.path())),
    ));
    let (listener, _) = listener().await;
    let ended = tokio::time::timeout(
        Duration::from_secs(5),
        serve(
            listener,
            Arc::new(Door::new(holding)),
            None,
            Box::pin(async {}),
        ),
    )
    .await;
    assert!(ended.is_ok());
}

#[tokio::test]
async fn a_connection_that_could_not_be_accepted_is_left() {
    let scratch = Scratch::new();
    let holding = Arc::new(Holding::in_directory(
        scratch.path(),
        Arc::new(Certificates::in_directory(scratch.path())),
    ));
    taken(
        Err(std::io::Error::other("refused")),
        &Arc::new(Door::new(holding)),
        None,
    );
}
