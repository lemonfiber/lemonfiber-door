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
use http_body_util::{BodyExt, Empty, Full, Limited};
use hyper::body::Incoming;
use hyper::upgrade::OnUpgrade;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo};
use lemonfiber_sidecar::pass::{FORWARDED as FORWARDED_URI, SESSION};

use crate::client;
use crate::holding::Holding;
use crate::playlist::{self, Origin};
use crate::rules::{self, Carrying, Check, Decision, Going, Presenting, Writing, REFUSED};

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

/// The headers that name who and what the request came through, which the door writes.
const FORWARDED_FOR: &str = "x-forwarded-for";
const FORWARDED_PROTO: &str = "x-forwarded-proto";
const FORWARDED_HOST: &str = "x-forwarded-host";

/// The coding that leaves a body as it is, the one a request on a pass accepts, so a
/// playlist answered on it can be written into.
const IDENTITY: &str = "identity";

/// The headers of a playlist answered on a pass that name the playlist as the media
/// server wrote it, rather than as the door writes it.
const STALE: [HeaderName; 4] = [
    header::CONTENT_LENGTH,
    header::ETAG,
    header::LAST_MODIFIED,
    header::ACCEPT_RANGES,
];

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
        let mut passing = request.headers().clone();
        stripped(&mut passing);
        let authorization: Vec<&[u8]> = passing
            .get_all(header::AUTHORIZATION)
            .iter()
            .map(HeaderValue::as_bytes)
            .collect();
        let named: Vec<&str> = rules.token_headers().collect();
        let headers: Vec<(&str, &[u8])> = passing
            .iter()
            .filter(|(name, _)| {
                named
                    .iter()
                    .any(|one| name.as_str().eq_ignore_ascii_case(one))
            })
            .map(|(name, value)| (name.as_str(), value.as_bytes()))
            .collect();
        let method = request.method().as_str();
        let going = match rules.decide(method, &raw, &authorization, &headers) {
            Decision::Refuse(refused) => return status(refused),
            Decision::Forward(going) => going,
            Decision::Open(opening) => match self.session(&opening.at, &raw).await {
                Ok(session) => match rules.opened(method, &opening, &session) {
                    Decision::Forward(going) => going,
                    _ => return status(REFUSED),
                },
                Err(refused) => return status(refused),
            },
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
        if let Some(at) = &going.pass_service {
            if let Err(refused) = self.passed(at, &raw).await {
                return status(refused);
            }
        }
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
                    carrying,
                };
                match carried(carrying, &headers) {
                    Some(carried) => self.asked(&question, carried).await,
                    None => Err(REFUSED),
                }
            }
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
            rules::answered(
                question.answers,
                question.carrying,
                answer.status().as_u16(),
            )?;
        }
        Ok(())
    }

    /// Whether the pass service at `at` says the pass the request `raw` names is good;
    /// anything but a yes, and no answer, is a refusal.
    async fn passed(&self, at: &str, raw: &str) -> Result<(), u16> {
        self.pass_answer(at, raw).await.map(drop)
    }

    /// The session the pass service at `at` says the pass the request `raw` names opens;
    /// a yes naming none, anything but a yes, and no answer, are a refusal.
    async fn session(&self, at: &str, raw: &str) -> Result<String, u16> {
        self.pass_answer(at, raw)
            .await?
            .headers()
            .get(SESSION)
            .and_then(|session| session.to_str().ok())
            .map(str::to_owned)
            .ok_or(REFUSED)
    }

    /// The pass service's yes about the pass the request `raw` names, asked at `at`;
    /// anything but a yes, and no answer, is a refusal.
    async fn pass_answer(&self, at: &str, raw: &str) -> Result<Response<Incoming>, u16> {
        let mut request = Request::new(Empty::new());
        *request.uri_mut() = at.parse().map_err(|_| REFUSED)?;
        let forwarded = HeaderValue::from_str(raw).map_err(|_| REFUSED)?;
        let named = HeaderName::from_bytes(FORWARDED_URI.as_bytes()).map_err(|_| REFUSED)?;
        request.headers_mut().insert(named, forwarded);
        let answered = tokio::time::timeout(self.answering, self.asking.request(request)).await;
        match answered {
            Ok(Ok(answer)) if answer.status().is_success() => Ok(answer),
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
        let upgrading = upgrade(request.headers()) && going.writing.is_none();
        let downstream = upgrading.then(|| hyper::upgrade::on(&mut request));
        if upgrading {
            headers.insert(header::CONNECTION, HeaderValue::from_static("upgrade"));
            if let Some(protocol) = request.headers().get(header::UPGRADE) {
                headers.insert(header::UPGRADE, protocol.clone());
            }
        }
        let scheme = if arrival.encrypted { "https" } else { "http" };
        headers.insert(FORWARDED_PROTO, HeaderValue::from_static(scheme));
        let host = host(&request);
        if let Some(host) = &host {
            headers.insert(FORWARDED_HOST, host.clone());
            headers.insert(header::HOST, host.clone());
        }
        let Some(to) = uri(upstream, &going.target) else {
            return status(UNREACHED);
        };
        if going.writing.is_some() {
            headers.insert(header::ACCEPT_ENCODING, HeaderValue::from_static(IDENTITY));
        }
        let method = request.method().clone();
        let head = method == Method::HEAD;
        let mut onward = Request::new(request.into_body());
        *onward.method_mut() = method;
        *onward.uri_mut() = to;
        *onward.version_mut() = Version::HTTP_11;
        *onward.headers_mut() = headers;
        let Ok(mut answer) = self.forwarding.request(onward).await else {
            return status(UNREACHED);
        };
        answer.headers_mut().remove(SESSION);
        if let (Some(downstream), StatusCode::SWITCHING_PROTOCOLS) = (downstream, answer.status()) {
            let upstream = hyper::upgrade::on(&mut answer);
            tokio::spawn(joined(downstream, upstream));
            return answer.map(BodyExt::boxed);
        }
        stripped(answer.headers_mut());
        let Some(writing) = &going.writing else {
            return answer.map(BodyExt::boxed);
        };
        let origin = Origin {
            scheme,
            authority: host.as_ref().and_then(|host| host.to_str().ok()),
        };
        on_pass(answer, writing, origin, head).await
    }
}

/// `answer`, to a request let through on a pass at `origin`, handed back as `writing`
/// says. A playlist has its parameters written into, and its token parameters taken off,
/// every address that leads back to `origin`, and loses the length and validators that
/// name it as the media server wrote it. Where the request was presented a session, an
/// answer whose headers hold it is answered unreached, and so is one whose body, read
/// whole unless its type is one only media is served as, holds it. A body the door
/// cannot read whole, being partial, encoded, longer than it reads or writes, or for a
/// playlist not text, is answered unreached.
async fn on_pass(
    mut answer: Response<Incoming>,
    writing: &Writing,
    origin: Origin<'_>,
    head: bool,
) -> Response<Body> {
    let session = writing.session.as_deref();
    let holding = |bytes: &[u8]| session.is_some_and(|session| playlist::holds(bytes, session));
    if answer
        .headers()
        .values()
        .any(|value| holding(value.as_bytes()))
    {
        return status(UNREACHED);
    }
    let kind = answer
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|kind| kind.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let playlist = playlist::is_playlist(&kind);
    if !playlist && (session.is_none() || playlist::is_media(&kind)) {
        return answer.map(BodyExt::boxed);
    }
    let encoded = answer
        .headers()
        .get_all(header::CONTENT_ENCODING)
        .iter()
        .any(|coding| !coding.as_bytes().eq_ignore_ascii_case(IDENTITY.as_bytes()));
    if encoded || answer.status() == StatusCode::PARTIAL_CONTENT {
        return status(UNREACHED);
    }
    if playlist {
        for stale in STALE {
            answer.headers_mut().remove(stale);
        }
    }
    if head
        || matches!(
            answer.status(),
            StatusCode::NO_CONTENT | StatusCode::NOT_MODIFIED
        )
    {
        return answer.map(BodyExt::boxed);
    }
    let (parts, body) = answer.into_parts();
    let Ok(collected) = Limited::new(body, playlist::MOST).collect().await else {
        return status(UNREACHED);
    };
    let read = collected.to_bytes();
    let body = if playlist {
        let Some(written) = written(read, writing, origin) else {
            return status(UNREACHED);
        };
        written
    } else {
        read
    };
    if holding(&body) {
        return status(UNREACHED);
    }
    Response::from_parts(
        parts,
        Full::new(body).map_err(|never| match never {}).boxed(),
    )
}

/// `read`, a playlist, written as `writing` says for `origin`; nothing where it is not
/// text or the door does not write it.
fn written(read: Bytes, writing: &Writing, origin: Origin<'_>) -> Option<Bytes> {
    let text = String::from_utf8(Vec::from(read)).ok()?;
    let carried: Vec<(&str, &str)> = writing
        .carried
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let stripped: Vec<&str> = writing.stripped.iter().map(String::as_str).collect();
    playlist::written(&text, &carried, &stripped, origin).map(Bytes::from)
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
    /// What it carries besides its path.
    carrying: &'a Carrying,
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
        SESSION,
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
