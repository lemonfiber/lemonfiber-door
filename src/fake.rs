//! A media server and a pass service the tests run the door in front of, each on a port
//! of its own, recording every request it is sent.
//!
//! The media server answers a question, `/Items/<id>`, with the status the test set for
//! that item, 200 where it set none, or after a minute where it set [`SLOW`]; `/socket`
//! with an upgrade that echoes what it is sent; and anything else with `200` and a body naming the
//! target it was sent. The pass service answers `200` where the request it is told of
//! carries `lfpass=good`, and `403` otherwise.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use lemonfiber_ports::service::MAY;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;

/// What a test sets an item's answer to for the media server to answer it a minute late.
pub(crate) const SLOW: u16 = 0;

/// One request a fake was sent: its method, its target as sent, and its headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sent {
    /// Its method.
    pub(crate) method: String,
    /// Its target, as its bytes arrived.
    pub(crate) target: String,
    /// Its headers, each name lower-case, every value of it in order.
    pub(crate) headers: BTreeMap<String, Vec<String>>,
}

impl Sent {
    /// The values of `name` it carried.
    pub(crate) fn header(&self, name: &str) -> Vec<String> {
        self.headers.get(name).cloned().unwrap_or_default()
    }
}

/// A fake on a port of its own.
#[derive(Debug, Clone)]
pub(crate) struct Fake {
    /// Where it listens.
    pub(crate) at: SocketAddr,
    sent: Arc<Mutex<Vec<Sent>>>,
    statuses: Arc<Mutex<BTreeMap<String, u16>>>,
}

impl Fake {
    /// Every request it was sent, in order.
    pub(crate) fn sent(&self) -> Vec<Sent> {
        self.sent
            .lock()
            .map(|sent| sent.clone())
            .unwrap_or_default()
    }

    /// Answer the question about `item` with `status`.
    pub(crate) fn answer(&self, item: &str, status: u16) {
        if let Ok(mut statuses) = self.statuses.lock() {
            statuses.insert(item.to_owned(), status);
        }
    }

    /// Its address, as the envelope names it.
    pub(crate) fn address(&self) -> String {
        format!("http://{}", self.at)
    }
}

/// Which fake it is.
#[derive(Debug, Clone, Copy)]
enum Kind {
    MediaServer,
    Pass,
}

/// A media server, listening.
pub(crate) async fn media_server() -> Fake {
    started(Kind::MediaServer).await
}

/// A pass service, listening.
pub(crate) async fn pass_service() -> Fake {
    started(Kind::Pass).await
}

/// An address nothing listens on.
pub(crate) async fn nobody() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await;
    let at = listener.as_ref().map(TcpListener::local_addr);
    match at {
        Ok(Ok(at)) => at,
        _ => unreachable!("a port is bound"),
    }
}

async fn started(kind: Kind) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|why| unreachable!("a port is bound: {why}"));
    let at = listener
        .local_addr()
        .unwrap_or_else(|why| unreachable!("it has an address: {why}"));
    let fake = Fake {
        at,
        sent: Arc::new(Mutex::new(Vec::new())),
        statuses: Arc::new(Mutex::new(BTreeMap::new())),
    };
    let serving = fake.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let fake = serving.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request| {
                    let fake = fake.clone();
                    async move { Ok::<_, Infallible>(answered(&fake, kind, request).await) }
                });
                let _ = Builder::new(TokioExecutor::new())
                    .serve_connection_with_upgrades(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    fake
}

async fn answered(
    fake: &Fake,
    kind: Kind,
    mut request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let target = request
        .uri()
        .path_and_query()
        .map(|target| target.as_str().to_owned())
        .unwrap_or_default();
    let mut headers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, value) in request.headers() {
        headers
            .entry(name.as_str().to_owned())
            .or_default()
            .push(String::from_utf8_lossy(value.as_bytes()).into_owned());
    }
    let sent = Sent {
        method: request.method().as_str().to_owned(),
        target: target.clone(),
        headers,
    };
    if let Ok(mut all) = fake.sent.lock() {
        all.push(sent.clone());
    }
    match kind {
        Kind::Pass => {
            let good = sent
                .header("x-forwarded-uri")
                .iter()
                .any(|forwarded| forwarded.contains("lfpass=good"));
            reply(if good { 200 } else { 403 }, "")
        }
        Kind::MediaServer if target == "/socket" => {
            let upgrading = hyper::upgrade::on(&mut request);
            tokio::spawn(async move {
                if let Ok(upgraded) = upgrading.await {
                    let mut io = TokioIo::new(upgraded);
                    let mut buffer = [0_u8; 64];
                    while let Ok(read) = io.read(&mut buffer).await {
                        if read == 0
                            || io
                                .write_all(buffer.get(..read).unwrap_or_default())
                                .await
                                .is_err()
                        {
                            break;
                        }
                    }
                }
            });
            let mut answer = reply(101, "");
            answer.headers_mut().insert(
                http::header::UPGRADE,
                http::HeaderValue::from_static("echo"),
            );
            answer.headers_mut().insert(
                http::header::CONNECTION,
                http::HeaderValue::from_static("upgrade"),
            );
            answer
        }
        Kind::MediaServer => {
            let item = target
                .strip_prefix("/Items/")
                .or_else(|| {
                    target.strip_prefix(&MAY.replace("{space}", "item").replace("{item}", ""))
                })
                .map(|rest| {
                    rest.split(['?', '/', '&'])
                        .next()
                        .unwrap_or_default()
                        .to_owned()
                });
            let status = item
                .and_then(|item| fake.statuses.lock().ok()?.get(&item).copied())
                .unwrap_or(200);
            if status == SLOW {
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
            let mut answer = reply(status, &target);
            answer.headers_mut().insert(
                http::header::CONNECTION,
                http::HeaderValue::from_static("close, x-private"),
            );
            answer
                .headers_mut()
                .insert("x-private", http::HeaderValue::from_static("1"));
            answer
                .headers_mut()
                .insert("keep-alive", http::HeaderValue::from_static("timeout=5"));
            answer
        }
    }
}

fn reply(status: u16, body: &str) -> Response<Full<Bytes>> {
    let mut answer = Response::new(Full::new(Bytes::from(body.to_owned())));
    *answer.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
    answer
}
