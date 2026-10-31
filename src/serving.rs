//! The household's ports: each request read against the rules held, asked about where
//! they say, and forwarded to the media server as the target that was checked.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::header::{self, HeaderMap, HeaderName, HeaderValue};
use http::uri::PathAndQuery;
use http::{Method, Request, Response, StatusCode, Uri, Version};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty};
use hyper::body::Incoming;
use hyper::upgrade::OnUpgrade;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo};

use crate::client;
use crate::holding::Holding;
use crate::rules::{self, Carrying, Check, Decision, Going, Presenting, REFUSED};

/// What the door answers with.
pub(crate) type Body = BoxBody<Bytes, hyper::Error>;

/// How long a question or a pass may take to connect.
const CONNECTING: Duration = Duration::from_secs(5);

/// How long a question or a pass may take to be answered once connected.
const ANSWERING: Duration = Duration::from_secs(15);

/// The status every household request is answered with while no guard is held.
const UNGUARDED: u16 = 503;

/// The status a request is answered with where the media server is not reached.
const UNREACHED: u16 = 502;

/// The status a request is answered with where the media server does not answer in time.
const UNANSWERED: u16 = 504;

/// The header the door names a live stream's request in, to the pass service.
const FORWARDED_URI: &str = "x-forwarded-uri";

/// The headers that name who and what the request came through, which the door writes.
const FORWARDED_FOR: &str = "x-forwarded-for";
const FORWARDED_PROTO: &str = "x-forwarded-proto";
const FORWARDED_HOST: &str = "x-forwarded-host";

/// The headers that make an answer to the question anything but a yes or a no.
const CONDITIONAL: [&str; 6] = [
    "range",
    "if-range",
    "if-match",
    "if-none-match",
    "if-modified-since",
    "if-unmodified-since",
];

/// The headers that belong to one connection and never travel on.
const HOP_BY_HOP: [&str; 8] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// The door: the rules it holds, and the clients it asks and forwards with.
pub(crate) struct Door {
    holding: Arc<Holding>,
    asking: Client<HttpConnector, Empty<Bytes>>,
    forwarding: Client<HttpConnector, Incoming>,
    answering: Duration,
}

/// Where a request arrived, and from whom.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Arrival {
    /// The address that connected.
    pub(crate) peer: SocketAddr,
    /// Whether it arrived on the encrypted port.
    pub(crate) encrypted: bool,
}

impl Door {
    /// A door enforcing what `holding` holds.
    pub(crate) fn new(holding: Arc<Holding>) -> Self {
        Self::answered_within(holding, ANSWERING)
    }

    /// A door enforcing what `holding` holds, waiting `answering` for a question or a
    /// pass to be answered.
    pub(crate) fn answered_within(holding: Arc<Holding>, answering: Duration) -> Self {
        let mut connector = HttpConnector::new();
        connector.set_connect_timeout(Some(CONNECTING));
        connector.set_nodelay(true);
        let built = Client::builder(TokioExecutor::new());
        Self {
            holding,
            asking: built.build(connector.clone()),
            forwarding: built.build(connector),
            answering,
        }
    }

    /// The answer to `request`, which arrived as `arrival` says.
    pub(crate) async fn answer(
        &self,
        request: Request<Incoming>,
        arrival: Arrival,
    ) -> Response<Body> {
        let Some(rules) = self.holding.rules() else {
            return status(UNGUARDED);
        };
        let raw = request
            .uri()
            .path_and_query()
            .map_or_else(String::new, |target| target.as_str().to_owned());
        let authorization: Vec<&[u8]> = request
            .headers()
            .get_all(header::AUTHORIZATION)
            .iter()
            .map(HeaderValue::as_bytes)
            .collect();
        let named: Vec<&str> = rules.token_headers().collect();
        let headers: Vec<(&str, &[u8])> = request
            .headers()
            .iter()
            .filter(|(name, _)| {
                named
                    .iter()
                    .any(|one| name.as_str().eq_ignore_ascii_case(one))
            })
            .map(|(name, value)| (name.as_str(), value.as_bytes()))
            .collect();
        let going = match rules.decide(request.method().as_str(), &raw, &authorization, &headers) {
            Decision::Refuse(refused) => return status(refused),
            Decision::Forward(going) => going,
        };
        let forwarded: Vec<&[u8]> = request
            .headers()
            .get_all(FORWARDED_FOR)
            .iter()
            .map(HeaderValue::as_bytes)
            .collect();
        let client = client::client(arrival.peer.ip(), &forwarded, &rules.guarded().proxy);
        let Some(headers) = outgoing(&request, &going.presenting, client) else {
            return status(REFUSED);
        };
        let checked = match &going.check {
            Check::Nothing => Ok(()),
            Check::Question {
                to,
                method,
                asked,
                answers,
                carrying,
            } => {
                let question = Question {
                    to,
                    method: *method,
                    asked,
                    answers,
                };
                match carried(carrying, &headers) {
                    Some(carried) => self.asked(&question, carried).await,
                    None => Err(REFUSED),
                }
            }
            Check::Pass { at } => self.passed(at, &raw).await,
        };
        if let Err(refused) = checked {
            return status(refused);
        }
        self.forwarded(request, &rules.guarded().upstream, &going, headers, arrival)
            .await
    }

    /// Whether `question`, asked about each item in turn carrying `carried`, says the
    /// member may have every one of them; the status the request is refused with where
    /// not.
    async fn asked(&self, question: &Question<'_>, carried: HeaderMap) -> Result<(), u16> {
        for one in question.asked {
            let mut request = Request::new(Empty::new());
            *request.method_mut() = verb(question.method);
            *request.uri_mut() = uri(question.to, one).ok_or(UNREACHED)?;
            *request.headers_mut() = carried.clone();
            let answer = tokio::time::timeout(self.answering, self.asking.request(request))
                .await
                .map_err(|_| UNANSWERED)?
                .map_err(|_| UNREACHED)?;
            rules::answered(question.answers, answer.status().as_u16())?;
        }
        Ok(())
    }

    /// Whether the pass service at `at` says the pass the request `raw` names is good;
    /// anything but a yes, and no answer, is a refusal.
    async fn passed(&self, at: &str, raw: &str) -> Result<(), u16> {
        let mut request = Request::new(Empty::new());
        *request.uri_mut() = at.parse().map_err(|_| REFUSED)?;
        let forwarded = HeaderValue::from_str(raw).map_err(|_| REFUSED)?;
        request.headers_mut().insert(FORWARDED_URI, forwarded);
        let answered = tokio::time::timeout(self.answering, self.asking.request(request)).await;
        match answered {
            Ok(Ok(answer)) if answer.status().is_success() => Ok(()),
            _ => Err(REFUSED),
        }
    }

    /// `request` forwarded to the media server at `upstream` as `going` says, and its
    /// answer handed back, an upgraded connection joined end to end.
    async fn forwarded(
        &self,
        mut request: Request<Incoming>,
        upstream: &str,
        going: &Going,
        mut headers: HeaderMap,
        arrival: Arrival,
    ) -> Response<Body> {
        let upgrading = upgrade(request.headers());
        let downstream = upgrading.then(|| hyper::upgrade::on(&mut request));
        if upgrading {
            headers.insert(header::CONNECTION, HeaderValue::from_static("upgrade"));
            if let Some(protocol) = request.headers().get(header::UPGRADE) {
                headers.insert(header::UPGRADE, protocol.clone());
            }
        }
        headers.insert(
            FORWARDED_PROTO,
            HeaderValue::from_static(if arrival.encrypted { "https" } else { "http" }),
        );
        if let Some(host) = host(&request) {
            headers.insert(FORWARDED_HOST, host.clone());
            headers.insert(header::HOST, host);
        }
        let Some(to) = uri(upstream, &going.target) else {
            return status(UNREACHED);
        };
        let method = request.method().clone();
        let mut onward = Request::new(request.into_body());
        *onward.method_mut() = method;
        *onward.uri_mut() = to;
        *onward.version_mut() = Version::HTTP_11;
        *onward.headers_mut() = headers;
        let Ok(mut answer) = self.forwarding.request(onward).await else {
            return status(UNREACHED);
        };
        if let (Some(downstream), StatusCode::SWITCHING_PROTOCOLS) = (downstream, answer.status()) {
            let upstream = hyper::upgrade::on(&mut answer);
            tokio::spawn(joined(downstream, upstream));
            return answer.map(BodyExt::boxed);
        }
        stripped(answer.headers_mut());
        answer.map(BodyExt::boxed)
    }
}

/// The household's upgraded connection and the media server's, joined end to end until
/// either closes.
async fn joined(downstream: OnUpgrade, upstream: OnUpgrade) -> std::io::Result<(u64, u64)> {
    let down = downstream.await.map_err(std::io::Error::other)?;
    let up = upstream.await.map_err(std::io::Error::other)?;
    tokio::io::copy_bidirectional(&mut TokioIo::new(down), &mut TokioIo::new(up)).await
}

/// A question a request is checked with, as its rules say.
struct Question<'a> {
    /// Where it is sent.
    to: &'a str,
    /// Its method.
    method: lemonfiber_ports::http::Method,
    /// The path and query of the question about each item, in order.
    asked: &'a [String],
    /// What each answer means.
    answers: &'a lemonfiber_ports::service::Answers,
}

/// The headers a question carries, as `carrying` says: `headers`, the request's as the
/// media server is sent them, but those that make an answer anything but a yes or a no;
/// or the member's token alone. Either asks for JSON. Nothing where the token cannot be
/// written as a header.
fn carried(carrying: &Carrying, headers: &HeaderMap) -> Option<HeaderMap> {
    let mut question = match carrying {
        Carrying::Request => {
            let mut question = headers.clone();
            for conditional in CONDITIONAL {
                question.remove(conditional);
            }
            question.remove(header::CONTENT_LENGTH);
            question.remove(header::CONTENT_TYPE);
            question
        }
        Carrying::Token(token) => {
            let mut question = HeaderMap::new();
            if let Some((name, value)) = token {
                question.insert(
                    HeaderName::from_bytes(name.as_bytes()).ok()?,
                    HeaderValue::from_str(value).ok()?,
                );
            }
            question
        }
    };
    question.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
    Some(question)
}

/// The headers a request goes on with: its own, but those of one connection and those
/// the door writes, its token presented as `presenting` says, and `client` named as the
/// whole of `X-Forwarded-For`; nothing where the token cannot be written as a header.
fn outgoing<B>(request: &Request<B>, presenting: &Presenting, client: IpAddr) -> Option<HeaderMap> {
    let mut headers = request.headers().clone();
    stripped(&mut headers);
    for written in [
        FORWARDED_FOR,
        FORWARDED_PROTO,
        FORWARDED_HOST,
        FORWARDED_URI,
    ] {
        headers.remove(written);
    }
    match presenting {
        Presenting::Unchanged => {}
        Presenting::Removed => {
            headers.remove(header::AUTHORIZATION);
        }
        Presenting::Header { name, value } => {
            headers.remove(header::AUTHORIZATION);
            let name = HeaderName::from_bytes(name.as_bytes()).ok()?;
            headers.insert(name, HeaderValue::from_str(value).ok()?);
        }
    }
    headers.insert(
        FORWARDED_FOR,
        HeaderValue::from_str(&client.to_string()).ok()?,
    );
    Some(headers)
}

/// `headers` without those of one connection: the hop-by-hop headers, and every header
/// `Connection` names.
fn stripped(headers: &mut HeaderMap) {
    let named: Vec<String> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    for name in named.iter().map(String::as_str).chain(HOP_BY_HOP) {
        headers.remove(name);
    }
}

/// Whether `headers` ask to upgrade the connection.
fn upgrade(headers: &HeaderMap) -> bool {
    headers.contains_key(header::UPGRADE)
        && headers
            .get_all(header::CONNECTION)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
}

/// The host `request` was sent to: its `Host` header, or the authority HTTP/2 carries.
fn host<B>(request: &Request<B>) -> Option<HeaderValue> {
    request.headers().get(header::HOST).cloned().or_else(|| {
        request
            .uri()
            .authority()
            .and_then(|authority| HeaderValue::from_str(authority.as_str()).ok())
    })
}

/// `target`, a path from the root and its query, at the scheme and authority `base`
/// names, built from those parts so nothing in `target` can name another host; nothing
/// where `base` names no scheme or authority, or `target` is not a path from the root.
fn uri(base: &str, target: &str) -> Option<Uri> {
    let base: Uri = base.parse().ok()?;
    let origin = target.starts_with('/') && !target.starts_with("//") && !target.contains('#');
    let path_and_query: PathAndQuery = origin.then(|| target.parse().ok()).flatten()?;
    Uri::builder()
        .scheme(base.scheme()?.clone())
        .authority(base.authority()?.clone())
        .path_and_query(path_and_query)
        .build()
        .ok()
}

/// The method `method` names.
fn verb(method: lemonfiber_ports::http::Method) -> Method {
    match method {
        lemonfiber_ports::http::Method::Get => Method::GET,
        lemonfiber_ports::http::Method::Post => Method::POST,
        lemonfiber_ports::http::Method::Put => Method::PUT,
        lemonfiber_ports::http::Method::Patch => Method::PATCH,
        lemonfiber_ports::http::Method::Delete => Method::DELETE,
    }
}

/// An answer with `code` and nothing in it.
pub(crate) fn status(code: u16) -> Response<Body> {
    let mut answer = Response::new(Empty::new().map_err(|never| match never {}).boxed());
    *answer.status_mut() = StatusCode::from_u16(code).unwrap_or(StatusCode::FORBIDDEN);
    answer
}

#[cfg(test)]
mod tests;
