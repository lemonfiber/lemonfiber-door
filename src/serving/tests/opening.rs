use std::time::Duration;

use lemonfiber_ports::service::Presentation;

use super::{asking_its_adapter, envelope, get, running, Running};
use crate::fake::{self, Fake};
use crate::fixtures::{ITEM, TOKEN};

async fn on_passes(adapter: Option<&Fake>) -> (Running, Fake, Fake) {
    let upstream = fake::media_server().await;
    let pass = fake::pass_service().await;
    let at = format!("{}/pass", pass.address());
    let mut guarded = match adapter {
        Some(adapter) => asking_its_adapter(&upstream, adapter),
        None => envelope(&upstream, &at, "10.80.96.3"),
    };
    guarded.pass = at;
    let door = running(Some(guarded), Duration::from_secs(1)).await;
    (door, upstream, pass)
}

fn presented() -> Vec<String> {
    vec![format!(r#"MediaBrowser Token="{TOKEN}""#)]
}

#[tokio::test]
async fn a_byte_path_on_a_pass_is_asked_about_and_forwarded_with_the_session_it_opens() {
    let (door, upstream, pass) = on_passes(None).await;
    let raw = format!("/Videos/{ITEM}/stream?static=true&lfpass=opens");
    let (status, answer) = get(door.at, &raw, &["Lemonfiber-Session: forged"]).await;
    assert_eq!(status, 200);
    assert!(!answer.to_ascii_lowercase().contains("lemonfiber-session"));
    assert!(!answer.contains(TOKEN));
    let told: Vec<Vec<String>> = pass
        .sent()
        .iter()
        .map(|sent| sent.header("x-forwarded-uri"))
        .collect();
    assert_eq!(told, vec![vec![raw]]);
    let sent = upstream.sent();
    let [question, forwarded] = sent.as_slice() else {
        unreachable!("a question and the request: {sent:?}");
    };
    assert_eq!(question.target, format!("/Items/{ITEM}"));
    assert_eq!(question.header("authorization"), presented());
    assert_eq!(
        forwarded.target,
        format!("/Videos/{ITEM}/stream?static=true")
    );
    assert_eq!(forwarded.header("authorization"), presented());
    assert!(forwarded.header("lemonfiber-session").is_empty());
}

#[tokio::test]
async fn a_pass_the_pass_service_opens_no_session_for_is_refused_without_asking_anything() {
    let (door, upstream, pass) = on_passes(None).await;
    for pass in ["good", "odd", "bad"] {
        let raw = format!("/Videos/{ITEM}/stream?lfpass={pass}");
        assert_eq!(get(door.at, &raw, &[]).await.0, 403, "{pass}");
    }
    let bearer = format!("Authorization: Bearer {TOKEN}");
    let raw = format!("/Videos/{ITEM}/stream?lfpass=opens");
    assert_eq!(get(door.at, &raw, &[bearer.as_str()]).await.0, 403);
    assert_eq!(pass.sent().len(), 3);
    assert!(upstream.sent().is_empty());
}

#[tokio::test]
async fn a_pass_service_not_answering_opens_nothing() {
    let upstream = fake::media_server().await;
    let at = format!("http://{}/pass", fake::nobody().await);
    let door = running(
        Some(envelope(&upstream, &at, "10.80.96.3")),
        Duration::from_secs(1),
    )
    .await;
    let raw = format!("/Videos/{ITEM}/stream?lfpass=opens");
    assert_eq!(get(door.at, &raw, &[]).await.0, 403);
    assert!(upstream.sent().is_empty());
}

#[tokio::test]
async fn the_adapter_is_asked_with_the_session_a_pass_opens() {
    let adapter = fake::media_server().await;
    let (door, upstream, _pass) = on_passes(Some(&adapter)).await;
    adapter.answer(ITEM, 204);
    let raw = format!("/Videos/{ITEM}/stream?lfpass=opens");
    assert_eq!(get(door.at, &raw, &[]).await.0, 200);
    let asked = adapter.sent();
    let [question] = asked.as_slice() else {
        unreachable!("one question: {asked:?}");
    };
    assert_eq!(question.header("authorization"), presented());
    let forwarded = upstream.sent();
    assert_eq!(
        forwarded
            .iter()
            .map(|one| one.header("authorization"))
            .collect::<Vec<_>>(),
        vec![presented()]
    );
}

#[tokio::test]
async fn a_playlist_served_on_an_item_pass_carries_it_in_every_address() {
    let (door, _upstream, _pass) = on_passes(None).await;
    let raw = format!("/Videos/{ITEM}/master.m3u8?lfpass=opens");
    let (status, answer) = get(door.at, &raw, &[]).await;
    assert_eq!(status, 200);
    assert!(answer.contains("seg.ts?x=1&lfpass=opens\n"));
    assert!(!answer.contains(TOKEN));
}

#[tokio::test]
async fn a_session_is_presented_in_a_header_even_where_a_token_is_declared_in_the_query() {
    let upstream = fake::media_server().await;
    let pass = fake::pass_service().await;
    let mut guarded = envelope(&upstream, &format!("{}/pass", pass.address()), "10.80.96.3");
    guarded.declared.token.presented = Presentation::Query {
        parameter: "api_key".to_owned(),
    };
    let headerless = running(Some(guarded.clone()), Duration::from_secs(1)).await;
    let raw = format!("/Videos/{ITEM}/stream?lfpass=opens");
    assert_eq!(get(headerless.at, &raw, &[]).await.0, 403);
    assert!(upstream.sent().is_empty());
    guarded.declared.token.headers = vec!["X-Emby-Token".to_owned()];
    let door = running(Some(guarded), Duration::from_secs(1)).await;
    assert_eq!(get(door.at, &raw, &[]).await.0, 200);
    let sent = upstream.sent();
    assert_eq!(
        sent.iter()
            .map(|one| one.target.as_str())
            .collect::<Vec<_>>(),
        vec![format!("/Items/{ITEM}"), format!("/Videos/{ITEM}/stream")]
    );
    assert!(sent
        .iter()
        .all(|one| one.header("x-emby-token") == vec![TOKEN.to_owned()]));
}

#[tokio::test]
async fn a_session_the_media_server_writes_back_is_taken_off_or_never_answered() {
    let (door, _upstream, _pass) = on_passes(None).await;
    let raw = format!("/Videos/{ITEM}/master.m3u8?shape=echoing&lfpass=opens");
    let (status, answer) = get(door.at, &raw, &[]).await;
    assert_eq!(status, 200);
    assert!(!answer.to_ascii_lowercase().contains(TOKEN));
    assert!(answer.contains("seg.ts?x=1&lfpass=opens\n"));
    assert!(answer.contains("main.m3u8?MediaSourceId=1&lfpass=opens\n"));
    for leaking in [
        format!("/Videos/{ITEM}/master.m3u8?shape=leaking&lfpass=opens"),
        format!("/Videos/{ITEM}/subtitles.vtt?echo=text&lfpass=opens"),
        format!("/Videos/{ITEM}/stream?echo=header&lfpass=opens"),
    ] {
        assert_eq!(get(door.at, &leaking, &[]).await.0, 502, "{leaking}");
    }
    let (status, answer) = get(
        door.at,
        &format!("/Videos/{ITEM}/stream?echo=media&lfpass=opens"),
        &[],
    )
    .await;
    assert_eq!(status, 200);
    assert!(answer.ends_with("G@\u{1}"));
}
