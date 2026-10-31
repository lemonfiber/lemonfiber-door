use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use lemonfiber_ports::http::Method;
use lemonfiber_ports::service::{
    Addressee, Answers, Guarded, Guarding, Presentation, Question, MAY,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};

use super::{status, Door};
use crate::fake::{self, Fake};
use crate::fixtures::{self, Scratch, ITEM, OTHER, TOKEN};
use crate::holding::Holding;
use crate::listening;
use crate::rules::Rules;
use crate::tls::Certificates;

/// A door listening on a port of its own, enforcing `guarded` where it is given.
struct Running {
    at: SocketAddr,
    _scratch: Scratch,
}

async fn running(guarded: Option<Guarded>, answering: Duration) -> Running {
    let scratch = Scratch::new();
    let holding = Arc::new(Holding::in_directory(
        scratch.path(),
        Arc::new(Certificates::in_directory(scratch.path())),
    ));
    if let Some(guarded) = guarded {
        assert!(holding.guard(&guarded).await.is_ok());
    }
    serve(Door::answered_within(holding, answering), scratch).await
}

async fn serve(door: Door, scratch: Scratch) -> Running {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|why| unreachable!("bound: {why}"));
    let at = listener
        .local_addr()
        .unwrap_or_else(|why| unreachable!("addressed: {why}"));
    tokio::spawn(listening::serve(
        listener,
        Arc::new(door),
        None,
        Box::pin(std::future::pending()),
    ));
    Running {
        at,
        _scratch: scratch,
    }
}

/// An envelope around Jellyfin's declaration, in front of `upstream` and `pass`, and
/// believing `proxy`.
fn envelope(upstream: &Fake, pass: &str, proxy: &str) -> Guarded {
    let mut guarded = fixtures::guarded(fixtures::declared());
    guarded.upstream = upstream.address();
    guarded.pass = pass.to_owned();
    guarded.proxy = proxy.to_owned();
    guarded
}

/// Send `head` to `at` as it is written, and read the whole answer back: its status and
/// everything after its status line.
async fn sent(at: SocketAddr, head: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(at)
        .await
        .unwrap_or_else(|why| unreachable!("connected: {why}"));
    let _ = stream.write_all(head.as_bytes()).await;
    let mut read = Vec::new();
    let _ = stream.read_to_end(&mut read).await;
    let text = String::from_utf8_lossy(&read).into_owned();
    let code = text
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    (code, text)
}

async fn get(at: SocketAddr, target: &str, headers: &[&str]) -> (u16, String) {
    let extra = headers.iter().fold(String::new(), |mut out, one| {
        out.push_str(one);
        out.push_str("\r\n");
        out
    });
    sent(
        at,
        &format!(
            "GET {target} HTTP/1.1\r\nHost: media.home.local\r\n{extra}Connection: close\r\n\r\n"
        ),
    )
    .await
}

#[tokio::test]
async fn a_door_holding_no_guard_answers_everything_unavailable() {
    let door = running(None, Duration::from_secs(1)).await;
    for target in ["/", &format!("/Videos/{ITEM}/stream"), "/web/index.html"] {
        assert_eq!(get(door.at, target, &[]).await.0, 503, "{target}");
    }
}

#[tokio::test]
async fn a_refused_request_is_refused_without_anything_asked() {
    let upstream = fake::media_server().await;
    let pass = fake::pass_service().await;
    let door = running(
        Some(envelope(
            &upstream,
            &format!("{}/pass", pass.address()),
            "10.80.96.3",
        )),
        Duration::from_secs(1),
    )
    .await;
    for target in [
        "/Artists/AC%2FDC/Images/Primary",
        "/Videos/%2e%2e/Items",
        &format!("/Videos/{ITEM}/stream?itemId={OTHER}"),
        "/Videos/notanitem/stream",
    ] {
        assert_eq!(get(door.at, target, &[]).await.0, 403, "{target}");
    }
    assert!(upstream.sent().is_empty());
    assert!(pass.sent().is_empty());
}

#[tokio::test]
async fn a_byte_path_is_asked_about_then_forwarded_as_it_arrived() {
    let upstream = fake::media_server().await;
    let door = running(
        Some(envelope(&upstream, "http://127.0.0.1:1/pass", "10.80.96.3")),
        Duration::from_secs(1),
    )
    .await;
    let target = format!("/Videos/{ITEM}/stream?static=true&api_key={TOKEN}");
    let (code, body) = get(
        door.at,
        &target,
        &[
            "Range: bytes=0-",
            "X-Forwarded-For: 6.6.6.6",
            "X-Forwarded-Host: evil",
        ],
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let sent = upstream.sent();
    let [question, forwarded] = sent.as_slice() else {
        unreachable!("one question and one request: {sent:?}");
    };
    assert_eq!(question.target, format!("/Items/{ITEM}?api_key={TOKEN}"));
    assert_eq!(question.header("accept"), vec!["application/json"]);
    assert!(question.header("range").is_empty());
    assert_eq!(forwarded.target, target);
    assert_eq!(forwarded.header("range"), vec!["bytes=0-"]);
    assert_eq!(forwarded.header("x-forwarded-for"), vec!["127.0.0.1"]);
    assert_eq!(forwarded.header("x-forwarded-proto"), vec!["http"]);
    assert_eq!(
        forwarded.header("x-forwarded-host"),
        vec!["media.home.local"]
    );
    assert_eq!(forwarded.header("host"), vec!["media.home.local"]);
    assert!(body.contains(&target));
}

#[tokio::test]
async fn every_item_a_request_names_is_asked_about() {
    let upstream = fake::media_server().await;
    let door = running(
        Some(envelope(&upstream, "http://127.0.0.1:1/pass", "10.80.96.3")),
        Duration::from_secs(1),
    )
    .await;
    let target = format!("/Videos/{ITEM}/Trickplay/320/0.jpg?MediaSourceId={OTHER}");
    assert_eq!(get(door.at, &target, &[]).await.0, 200);
    upstream.answer(OTHER, 404);
    assert_eq!(get(door.at, &target, &[]).await.0, 404);
    let asked: Vec<String> = upstream
        .sent()
        .into_iter()
        .map(|sent| sent.target)
        .collect();
    assert_eq!(
        asked,
        vec![
            format!("/Items/{ITEM}"),
            format!("/Items/{OTHER}"),
            target,
            format!("/Items/{ITEM}"),
            format!("/Items/{OTHER}"),
        ]
    );
}

#[tokio::test]
async fn a_declared_refusal_keeps_its_status_and_an_unclassed_answer_is_refused() {
    let upstream = fake::media_server().await;
    let door = running(
        Some(envelope(&upstream, "http://127.0.0.1:1/pass", "10.80.96.3")),
        Duration::from_secs(1),
    )
    .await;
    let target = format!("/Videos/{ITEM}/stream");
    for (answer, expected) in [(404, 404), (401, 401), (403, 403), (500, 403), (302, 403)] {
        upstream.answer(ITEM, answer);
        assert_eq!(get(door.at, &target, &[]).await.0, expected, "{answer}");
    }
    assert!(upstream
        .sent()
        .iter()
        .all(|sent| sent.target.starts_with("/Items/")));
}

#[tokio::test]
async fn a_media_server_not_reached_is_502_and_one_answering_late_is_504() {
    let upstream = fake::media_server().await;
    let mut guarded = envelope(&upstream, "http://127.0.0.1:1/pass", "10.80.96.3");
    guarded.upstream = format!("http://{}", fake::nobody().await);
    let door = running(Some(guarded), Duration::from_secs(1)).await;
    assert_eq!(
        get(door.at, &format!("/Videos/{ITEM}/stream"), &[]).await.0,
        502
    );
    assert_eq!(get(door.at, "/web/index.html", &[]).await.0, 502);
    upstream.answer(ITEM, fake::SLOW);
    let guarded = envelope(&upstream, "http://127.0.0.1:1/pass", "10.80.96.3");
    let door = running(Some(guarded), Duration::from_millis(200)).await;
    assert_eq!(
        get(door.at, &format!("/Videos/{ITEM}/stream"), &[]).await.0,
        504
    );
}

#[tokio::test]
async fn a_live_stream_goes_on_without_its_pass_only_where_the_pass_service_says_yes() {
    let upstream = fake::media_server().await;
    let pass = fake::pass_service().await;
    let door = running(
        Some(envelope(
            &upstream,
            &format!("{}/pass", pass.address()),
            "10.80.96.3",
        )),
        Duration::from_secs(1),
    )
    .await;
    let stream = "0123456789abcdef0123456789abcdef";
    let good = format!("/LiveTv/LiveStreamFiles/{stream}/stream.ts?lfpass=good&x=1");
    assert_eq!(get(door.at, &good, &[]).await.0, 200);
    let bad = format!("/LiveTv/LiveStreamFiles/{stream}/stream.ts?lfpass=bad");
    assert_eq!(get(door.at, &bad, &[]).await.0, 403);
    let told: Vec<Vec<String>> = pass
        .sent()
        .iter()
        .map(|sent| sent.header("x-forwarded-uri"))
        .collect();
    assert_eq!(told, vec![vec![good], vec![bad]]);
    let forwarded: Vec<String> = upstream
        .sent()
        .into_iter()
        .map(|sent| sent.target)
        .collect();
    assert_eq!(
        forwarded,
        vec![format!("/LiveTv/LiveStreamFiles/{stream}/stream.ts?x=1")]
    );
}

#[tokio::test]
async fn a_pass_service_not_answering_is_a_refusal() {
    let upstream = fake::media_server().await;
    let door = running(
        Some(envelope(
            &upstream,
            &format!("http://{}/pass", fake::nobody().await),
            "10.80.96.3",
        )),
        Duration::from_secs(1),
    )
    .await;
    let stream = "0123456789abcdef0123456789abcdef";
    assert_eq!(
        get(
            door.at,
            &format!("/LiveTv/LiveStreamFiles/{stream}/stream.ts?lfpass=good"),
            &[]
        )
        .await
        .0,
        403
    );
    assert!(upstream.sent().is_empty());
}

#[tokio::test]
async fn the_proxy_alone_is_believed_about_the_client_and_the_media_server_told_it_alone() {
    let upstream = fake::media_server().await;
    let believed = running(
        Some(envelope(&upstream, "http://127.0.0.1:1/pass", "127.0.0.1")),
        Duration::from_secs(1),
    )
    .await;
    let _ = get(
        believed.at,
        "/web/index.html",
        &["X-Forwarded-For: 6.6.6.6, 192.168.1.20"],
    )
    .await;
    let doubted = running(
        Some(envelope(&upstream, "http://127.0.0.1:1/pass", "10.80.96.3")),
        Duration::from_secs(1),
    )
    .await;
    let _ = get(
        doubted.at,
        "/web/index.html",
        &["X-Forwarded-For: 192.168.1.20"],
    )
    .await;
    let told: Vec<Vec<String>> = upstream
        .sent()
        .iter()
        .map(|sent| sent.header("x-forwarded-for"))
        .collect();
    assert_eq!(
        told,
        vec![
            vec!["192.168.1.20".to_owned()],
            vec!["127.0.0.1".to_owned()]
        ]
    );
}

#[tokio::test]
async fn a_bearer_token_reaches_the_media_server_as_the_declaration_presents_it() {
    let upstream = fake::media_server().await;
    let mut header = envelope(&upstream, "http://127.0.0.1:1/pass", "10.80.96.3");
    let door = running(Some(header.clone()), Duration::from_secs(1)).await;
    let target = format!("/Videos/{ITEM}/stream");
    let bearer = format!("Authorization: Bearer {TOKEN}");
    assert_eq!(get(door.at, &target, &[&bearer]).await.0, 200);
    let presented = format!(r#"MediaBrowser Token="{TOKEN}""#);
    assert!(upstream
        .sent()
        .iter()
        .all(|sent| sent.header("authorization") == vec![presented.clone()]));
    header.declared.token.presented = Presentation::Query {
        parameter: "api_key".to_owned(),
    };
    let queried = fake::media_server().await;
    header.upstream = queried.address();
    let door = running(Some(header), Duration::from_secs(1)).await;
    assert_eq!(get(door.at, &target, &[&bearer]).await.0, 200);
    let sent = queried.sent();
    assert!(sent
        .iter()
        .all(|one| one.header("authorization").is_empty()));
    let targets: Vec<&str> = sent.iter().map(|one| one.target.as_str()).collect();
    assert_eq!(
        targets,
        vec![
            format!("/Items/{ITEM}?api_key={TOKEN}"),
            format!("{target}?api_key={TOKEN}")
        ]
    );
}

#[tokio::test]
async fn a_request_presenting_a_token_twice_is_refused() {
    let upstream = fake::media_server().await;
    let door = running(
        Some(envelope(&upstream, "http://127.0.0.1:1/pass", "10.80.96.3")),
        Duration::from_secs(1),
    )
    .await;
    let bearer = format!("Authorization: Bearer {TOKEN}");
    assert_eq!(get(door.at, "/Users/Me", &[&bearer, &bearer]).await.0, 403);
    assert_eq!(
        get(door.at, &format!("/Users/Me?ApiKey={OTHER}"), &[&bearer])
            .await
            .0,
        403
    );
    assert!(upstream.sent().is_empty());
}

#[tokio::test]
async fn what_belongs_to_one_connection_does_not_travel_on() {
    let upstream = fake::media_server().await;
    let door = running(
        Some(envelope(&upstream, "http://127.0.0.1:1/pass", "10.80.96.3")),
        Duration::from_secs(1),
    )
    .await;
    let (code, answer) = get(
        door.at,
        "/web/index.html",
        &[
            "Connection: x-secret",
            "X-Secret: 1",
            "Keep-Alive: 1",
            "TE: trailers",
            "X-Forwarded-Uri: /x",
        ],
    )
    .await;
    assert_eq!(code, 200);
    let [forwarded] = upstream
        .sent()
        .try_into()
        .unwrap_or_else(|_| unreachable!("one request"));
    for gone in ["x-secret", "keep-alive", "te", "x-forwarded-uri"] {
        assert!(forwarded.header(gone).is_empty(), "{gone}");
    }
    let lowered = answer.to_ascii_lowercase();
    assert!(!lowered.contains("x-private"), "{answer}");
    assert!(!lowered.contains("keep-alive: timeout"), "{answer}");
}

#[tokio::test]
async fn an_upgraded_connection_is_joined_end_to_end() {
    let upstream = fake::media_server().await;
    let door = running(
        Some(envelope(&upstream, "http://127.0.0.1:1/pass", "10.80.96.3")),
        Duration::from_secs(1),
    )
    .await;
    let mut stream = TcpStream::connect(door.at)
        .await
        .unwrap_or_else(|why| unreachable!("connected: {why}"));
    let _ = stream
        .write_all(
            b"GET /socket HTTP/1.1\r\nHost: h\r\nConnection: Upgrade\r\nUpgrade: echo\r\n\r\n",
        )
        .await;
    let mut head = vec![0_u8; 4096];
    let read = stream.read(&mut head).await.unwrap_or_default();
    let head = String::from_utf8_lossy(head.get(..read).unwrap_or_default()).into_owned();
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");
    let _ = stream.write_all(b"ping").await;
    let mut echoed = [0_u8; 4];
    let _ = stream.read_exact(&mut echoed).await;
    assert_eq!(&echoed, b"ping");
    let [forwarded] = upstream
        .sent()
        .try_into()
        .unwrap_or_else(|_| unreachable!("one request"));
    assert_eq!(forwarded.header("upgrade"), vec!["echo"]);
    drop(stream);
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn an_upgrade_the_media_server_declines_is_answered_as_it_answered() {
    let upstream = fake::media_server().await;
    let door = running(
        Some(envelope(&upstream, "http://127.0.0.1:1/pass", "10.80.96.3")),
        Duration::from_secs(1),
    )
    .await;
    let (code, _) = get(
        door.at,
        "/web",
        &["Upgrade: echo", "Connection: upgrade, close"],
    )
    .await;
    assert_eq!(code, 200);
}

#[tokio::test]
async fn an_address_the_rules_cannot_build_is_answered_unreached() {
    let mut guarded = fixtures::guarded(fixtures::declared());
    guarded.upstream = "http://bad host".to_owned();
    let scratch = Scratch::new();
    let rules = Rules::of(guarded).unwrap_or_else(|why| unreachable!("{why:?}"));
    let door = serve(
        Door::new(Arc::new(Holding::enforcing(scratch.path(), rules))),
        scratch,
    )
    .await;
    assert_eq!(get(door.at, "/web", &[]).await.0, 502);
    assert_eq!(
        get(door.at, &format!("/Videos/{ITEM}/stream"), &[]).await.0,
        502
    );
}

#[tokio::test]
async fn a_question_sent_with_another_method_uses_it() {
    let upstream = fake::media_server().await;
    let mut guarded = envelope(&upstream, "http://127.0.0.1:1/pass", "10.80.96.3");
    let mut methods = Vec::new();
    for method in [
        lemonfiber_ports::http::Method::Post,
        lemonfiber_ports::http::Method::Put,
        lemonfiber_ports::http::Method::Patch,
        lemonfiber_ports::http::Method::Delete,
    ] {
        if let Some(question) = guarded.declared.question.as_mut() {
            question.method = method;
        }
        let door = running(Some(guarded.clone()), Duration::from_secs(1)).await;
        let _ = get(door.at, &format!("/Videos/{ITEM}/stream"), &[]).await;
        methods.push(
            upstream
                .sent()
                .iter()
                .rev()
                .nth(1)
                .map(|sent| sent.method.clone()),
        );
    }
    assert_eq!(
        methods,
        ["POST", "PUT", "PATCH", "DELETE"].map(|one| Some(one.to_owned()))
    );
}

#[test]
fn a_status_no_answer_can_carry_is_answered_as_a_refusal() {
    assert_eq!(status(42).status().as_u16(), 403);
    assert_eq!(status(404).status().as_u16(), 404);
}

#[tokio::test]
async fn a_token_the_rules_cannot_write_as_a_header_is_refused() {
    let upstream = fake::media_server().await;
    let mut guarded = envelope(&upstream, "http://127.0.0.1:1/pass", "10.80.96.3");
    guarded.declared.token.presented = Presentation::Header {
        name: "bad name".to_owned(),
        value: "{token}".to_owned(),
    };
    let scratch = Scratch::new();
    let rules = Rules::of(guarded).unwrap_or_else(|why| unreachable!("{why:?}"));
    let door = serve(
        Door::new(Arc::new(Holding::enforcing(scratch.path(), rules))),
        scratch,
    )
    .await;
    let bearer = format!("Authorization: Bearer {TOKEN}");
    assert_eq!(get(door.at, "/web", &[&bearer]).await.0, 403);
    assert!(upstream.sent().is_empty());
}

#[tokio::test]
async fn the_adapter_is_asked_carrying_the_member_token_and_nothing_else() {
    let upstream = fake::media_server().await;
    let adapter = fake::media_server().await;
    let mut guarded = envelope(&upstream, "http://127.0.0.1:1/pass", "10.80.96.3");
    guarded.adapter = Some(adapter.address());
    for path in &mut guarded.declared.bytes {
        path.space = Some("item".to_owned());
    }
    guarded.declared.question = Some(Question {
        method: Method::Get,
        path: MAY.to_owned(),
        to: Addressee::Adapter,
        answers: Answers::from_adapter(),
    });
    adapter.answer(ITEM, 204);
    adapter.answer(OTHER, 404);
    let door = running(Some(guarded), Duration::from_secs(1)).await;
    let bearer = format!("Authorization: Bearer {TOKEN}");
    let headers = [
        bearer.as_str(),
        "Cookie: session=1",
        "X-Emby-Token: another",
        "Range: bytes=0-",
    ];
    let target = format!("/Videos/{ITEM}/stream");
    assert_eq!(get(door.at, &target, &headers).await.0, 200);
    let refused = format!("/Videos/{OTHER}/stream");
    assert_eq!(get(door.at, &refused, &headers).await.0, 404);
    assert_eq!(get(door.at, &target, &["Cookie: session=1"]).await.0, 200);
    let asked = adapter.sent();
    let [question, _, tokenless] = asked.as_slice() else {
        unreachable!("one question each: {asked:?}");
    };
    let mut unborne: Vec<&str> = tokenless.headers.keys().map(String::as_str).collect();
    unborne.sort_unstable();
    assert_eq!(unborne, ["accept", "host"]);
    assert_eq!(
        question.target,
        MAY.replace("{space}", "item").replace("{item}", ITEM)
    );
    let mut carried: Vec<&str> = question.headers.keys().map(String::as_str).collect();
    carried.sort_unstable();
    assert_eq!(carried, ["accept", "authorization", "host"]);
    assert_eq!(
        question.header("authorization"),
        vec![format!(r#"MediaBrowser Token="{TOKEN}""#)]
    );
    let forwarded = upstream.sent();
    assert_eq!(
        forwarded
            .iter()
            .map(|one| one.target.as_str())
            .collect::<Vec<_>>(),
        [target.as_str(), target.as_str()]
    );
}

#[tokio::test]
async fn a_token_the_adapter_question_cannot_carry_as_a_header_is_refused() {
    for (name, value) in [("bad name", "{token}"), ("X-Token", "{token}\n")] {
        let upstream = fake::media_server().await;
        let adapter = fake::media_server().await;
        let mut guarded = envelope(&upstream, "http://127.0.0.1:1/pass", "10.80.96.3");
        guarded.adapter = Some(adapter.address());
        for path in &mut guarded.declared.bytes {
            path.space = Some("item".to_owned());
        }
        guarded.declared.question = Some(Question {
            method: Method::Get,
            path: MAY.to_owned(),
            to: Addressee::Adapter,
            answers: Answers::from_adapter(),
        });
        guarded.declared.token.presented = Presentation::Header {
            name: name.to_owned(),
            value: value.to_owned(),
        };
        let scratch = Scratch::new();
        let rules = Rules::of(guarded).unwrap_or_else(|why| unreachable!("{why:?}"));
        let door = serve(
            Door::new(Arc::new(Holding::enforcing(scratch.path(), rules))),
            scratch,
        )
        .await;
        let target = format!("/Videos/{ITEM}/stream?ApiKey={TOKEN}");
        assert_eq!(get(door.at, &target, &[]).await.0, 403, "{name}");
        assert!(adapter.sent().is_empty(), "{name}");
        assert!(upstream.sent().is_empty(), "{name}");
    }
}

#[test]
fn a_question_or_a_request_reaches_only_the_host_it_is_sent_to() {
    let at = |target: &str| super::uri("http://jellyfin:8096", target).map(|uri| uri.to_string());
    assert_eq!(
        at(&format!("/Items/{ITEM}?x=1")),
        Some(format!("http://jellyfin:8096/Items/{ITEM}?x=1"))
    );
    for target in [
        "@evil.example/Items/1",
        "//evil.example/Items/1",
        "evil.example/Items/1",
        "",
        "/Items/1#x",
    ] {
        assert_eq!(at(target), None, "{target}");
    }
    assert_eq!(super::uri("jellyfin:8096", "/Items/1"), None);
}

/// An envelope around Jellyfin's declaration whose question goes to `adapter`.
fn asking_its_adapter(upstream: &Fake, adapter: &Fake) -> Guarded {
    let mut guarded = envelope(upstream, "http://127.0.0.1:1/pass", "10.80.96.3");
    guarded.adapter = Some(adapter.address());
    guarded.declared.question = Some(Question {
        method: Method::Get,
        path: MAY.to_owned(),
        to: Addressee::Adapter,
        answers: Answers::from_adapter(),
    });
    for path in &mut guarded.declared.bytes {
        path.space = Some("item".to_owned());
    }
    guarded
}

#[tokio::test]
async fn a_header_read_as_no_token_reaches_the_upstreams_question_as_it_reaches_the_upstream() {
    let client = format!(r#"Authorization: MediaBrowser Client="x", Device="y", Token="{OTHER}""#);
    let unshaped = "Authorization: Bearer not-a-token".to_owned();
    for header in [client, unshaped] {
        let upstream = fake::media_server().await;
        let door = running(
            Some(envelope(&upstream, "http://127.0.0.1:1/pass", "10.80.96.3")),
            Duration::from_secs(1),
        )
        .await;
        let target = format!("/Videos/{ITEM}/stream?api_key={TOKEN}");
        assert_eq!(get(door.at, &target, &[&header]).await.0, 200, "{header}");
        let sent = upstream.sent();
        let [question, forwarded] = sent.as_slice() else {
            unreachable!("one question and one request: {sent:?}");
        };
        let value = header.trim_start_matches("Authorization: ").to_owned();
        assert_eq!(
            question.header("authorization"),
            vec![value.clone()],
            "{header}"
        );
        assert_eq!(forwarded.header("authorization"), vec![value], "{header}");
        assert_eq!(question.target, format!("/Items/{ITEM}?api_key={TOKEN}"));
        assert_eq!(forwarded.target, target);
    }
}

#[tokio::test]
async fn a_header_read_as_no_token_is_refused_where_the_adapter_is_asked() {
    for header in [
        format!(r#"Authorization: MediaBrowser Client="x", Token="{OTHER}""#),
        "Authorization: Bearer not-a-token".to_owned(),
    ] {
        let upstream = fake::media_server().await;
        let adapter = fake::media_server().await;
        let door = running(
            Some(asking_its_adapter(&upstream, &adapter)),
            Duration::from_secs(1),
        )
        .await;
        let target = format!("/Videos/{ITEM}/stream?api_key={TOKEN}");
        assert_eq!(get(door.at, &target, &[&header]).await.0, 403, "{header}");
        assert!(adapter.sent().is_empty(), "{header}");
        assert!(upstream.sent().is_empty(), "{header}");
    }
}

#[tokio::test]
async fn a_token_in_a_header_the_request_has_dropped_on_its_way_is_never_read() {
    let upstream = fake::media_server().await;
    let adapter = fake::media_server().await;
    let mut guarded = asking_its_adapter(&upstream, &adapter);
    guarded.declared.token.headers = vec!["X-Plex-Token".to_owned()];
    adapter.answer(ITEM, 204);
    let door = running(Some(guarded), Duration::from_secs(1)).await;
    let token = format!("X-Plex-Token: {TOKEN}");
    let target = format!("/Videos/{ITEM}/stream");
    assert_eq!(
        get(
            door.at,
            &target,
            &[&token, "Connection: x-plex-token, close"]
        )
        .await
        .0,
        200
    );
    let asked = adapter.sent();
    let [question] = asked.as_slice() else {
        unreachable!("one question: {asked:?}");
    };
    assert!(question.header("authorization").is_empty(), "{question:?}");
    let forwarded = upstream.sent();
    let [only] = forwarded.as_slice() else {
        unreachable!("the request alone: {forwarded:?}");
    };
    assert!(only.header("x-plex-token").is_empty(), "{only:?}");
}
