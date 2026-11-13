use std::time::Duration;

use lemonfiber_ports::service::LivePath;

use super::{envelope, get, running, Running};
use crate::fake::{self, Fake};
use crate::fixtures::{ITEM, OTHER, TOKEN};

const STREAM: &str = "0123456789abcdef0123456789abcdef_0123456789abcdef0123456789abcdef_0123456789abcdef0123456789abcdef";

async fn transcoding() -> (Running, Fake, Fake) {
    let upstream = fake::media_server().await;
    let pass = fake::pass_service().await;
    let mut guarded = envelope(&upstream, &format!("{}/pass", pass.address()), "10.80.96.3");
    guarded.declared.live.push(LivePath {
        pattern: "(?i)^/Videos/[^/]+/(?:(?:live|master|main)\\.m3u8$|hls1?/)".to_owned(),
        parameter: Some("LiveStreamId".to_owned()),
        stream: "^[0-9a-f]{32}_[0-9a-f]{32}_[0-9a-f]{32}$".to_owned(),
    });
    let door = running(Some(guarded), Duration::from_secs(1)).await;
    (door, upstream, pass)
}

#[tokio::test]
async fn a_transcoded_live_stream_is_served_on_its_pass_where_the_member_may_have_its_item() {
    let (door, upstream, pass) = transcoding().await;
    let bearer = format!("Authorization: Bearer {TOKEN}");
    let raw = format!("/Videos/{ITEM}/live.m3u8?LiveStreamId={STREAM}&lfpass=good");
    let (status, answer) = get(door.at, &raw, &[bearer.as_str()]).await;
    assert_eq!(status, 200);
    assert!(answer.contains(&format!("seg.ts?x=1&lfpass=good&LiveStreamId={STREAM}\n")));
    assert_eq!(pass.sent().len(), 1);
    let targets: Vec<String> = upstream.sent().into_iter().map(|one| one.target).collect();
    assert_eq!(
        targets,
        vec![
            format!("/Items/{ITEM}"),
            format!("/Videos/{ITEM}/live.m3u8?LiveStreamId={STREAM}")
        ]
    );
}

#[tokio::test]
async fn a_pass_for_a_live_stream_on_the_route_of_an_item_the_member_may_not_have_is_refused() {
    let (door, upstream, _pass) = transcoding().await;
    upstream.answer(OTHER, 404);
    let bearer = format!("Authorization: Bearer {TOKEN}");
    let raw = format!("/Videos/{OTHER}/master.m3u8?LiveStreamId={STREAM}&lfpass=good");
    assert_eq!(get(door.at, &raw, &[bearer.as_str()]).await.0, 404);
    let unbearing = format!("/Videos/{ITEM}/master.m3u8?LiveStreamId={STREAM}&lfpass=good");
    assert_eq!(get(door.at, &unbearing, &[]).await.0, 403);
    let targets: Vec<String> = upstream.sent().into_iter().map(|one| one.target).collect();
    assert_eq!(targets, vec![format!("/Items/{OTHER}")]);
}

#[tokio::test]
async fn a_transcoded_live_stream_whose_pass_is_not_good_is_refused_before_its_item_is_asked_about()
{
    let (door, upstream, _pass) = transcoding().await;
    let bearer = format!("Authorization: Bearer {TOKEN}");
    let raw = format!("/Videos/{ITEM}/live.m3u8?LiveStreamId={STREAM}&lfpass=bad");
    assert_eq!(get(door.at, &raw, &[bearer.as_str()]).await.0, 403);
    assert!(upstream.sent().is_empty());
}

#[tokio::test]
async fn a_live_segment_of_an_item_the_member_may_not_have_is_refused_on_a_good_pass() {
    let (door, upstream, pass) = transcoding().await;
    upstream.answer(OTHER, 404);
    let bearer = format!("Authorization: Bearer {TOKEN}");
    let segment = format!(
        "/Videos/{OTHER}/hls/0123456789abcdef0123456789abcdef/fedcba9876543210fedcba98765432100.ts?LiveStreamId={STREAM}&lfpass=good"
    );
    assert_eq!(get(door.at, &segment, &[bearer.as_str()]).await.0, 404);
    assert_eq!(pass.sent().len(), 1);
    let targets: Vec<String> = upstream.sent().into_iter().map(|one| one.target).collect();
    assert_eq!(targets, vec![format!("/Items/{OTHER}")]);
}

#[tokio::test]
async fn a_token_the_media_server_writes_into_a_live_playlist_is_taken_off() {
    let (door, _upstream, _pass) = transcoding().await;
    let raw = format!(
        "/Videos/{ITEM}/main.m3u8?LiveStreamId={STREAM}&shape=echoing&lfpass=good&api_key={TOKEN}"
    );
    let (status, answer) = get(door.at, &raw, &[]).await;
    assert_eq!(status, 200);
    let body = answer.split_once("\r\n\r\n").map_or("", |(_, body)| body);
    assert!(!body.to_ascii_lowercase().contains(TOKEN));
    assert!(body.contains(&format!("seg.ts?x=1&lfpass=good&LiveStreamId={STREAM}\n")));
}
