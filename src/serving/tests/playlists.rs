use std::time::Duration;

use super::{envelope, get, running, sent, Running};
use crate::fake::{self, Fake, PLAYLIST};

const STREAM: &str = "0123456789abcdef0123456789abcdef";

async fn on_a_pass() -> (Running, Fake) {
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
    (door, upstream)
}

fn live(query: &str) -> String {
    format!("/LiveTv/LiveStreamFiles/{STREAM}/stream.m3u8?{query}")
}

fn split(answer: &str) -> (Vec<String>, &str) {
    let (head, body) = answer.split_once("\r\n\r\n").unwrap_or((answer, ""));
    let headers = head.lines().skip(1).map(str::to_ascii_lowercase).collect();
    (headers, body)
}

fn holds(headers: &[String], name: &str) -> bool {
    headers
        .iter()
        .any(|header| header.starts_with(&format!("{name}:")))
}

#[tokio::test]
async fn a_playlist_answered_on_a_pass_carries_it_in_every_address_and_nothing_else_changes() {
    let (door, upstream) = on_a_pass().await;
    let (status, answer) = get(
        door.at,
        &live("lfpass=good&MediaSourceId=1"),
        &["Accept-Encoding: gzip, br"],
    )
    .await;
    assert_eq!(status, 200);
    let (headers, body) = split(&answer);
    let expected = "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4?lfpass=good\"\nseg.ts?x=1&lfpass=good\nhttp://media.home.local/own.ts?lfpass=good\nhttps://elsewhere.example/off.ts\n";
    assert_eq!(body, expected);
    assert!(headers.contains(&format!("content-length: {}", expected.len())));
    assert!(headers.contains(&"x-kept: 1".to_owned()));
    for stale in ["etag", "last-modified", "accept-ranges"] {
        assert!(!holds(&headers, stale), "{stale}");
    }
    let forwarded = upstream.sent();
    let [one] = forwarded.as_slice() else {
        unreachable!("one request forwarded: {forwarded:?}");
    };
    assert_eq!(
        one.target,
        format!("/LiveTv/LiveStreamFiles/{STREAM}/stream.m3u8?MediaSourceId=1")
    );
    assert_eq!(one.header("accept-encoding"), vec!["identity".to_owned()]);
}

#[tokio::test]
async fn a_playlist_the_door_cannot_write_into_whole_is_answered_unreached() {
    let (door, _upstream) = on_a_pass().await;
    for shape in ["encoded", "partial", "long", "binary", "many"] {
        let (status, _) = get(door.at, &live(&format!("lfpass=good&shape={shape}")), &[]).await;
        assert_eq!(status, 502, "{shape}");
    }
}

#[tokio::test]
async fn a_playlist_with_no_body_to_write_into_goes_on_without_its_length_or_validators() {
    let (door, _upstream) = on_a_pass().await;
    let (status, answer) = sent(
        door.at,
        &format!(
            "HEAD {} HTTP/1.1\r\nHost: media.home.local\r\nConnection: close\r\n\r\n",
            live("lfpass=good")
        ),
    )
    .await;
    assert_eq!(status, 200);
    let (headers, body) = split(&answer);
    assert!(body.is_empty());
    for stale in ["etag", "last-modified", "accept-ranges"] {
        assert!(!holds(&headers, stale), "{stale}");
    }
    let (status, answer) = get(door.at, &live("lfpass=good&shape=unchanged"), &[]).await;
    assert_eq!(status, 304);
    assert!(!holds(&split(&answer).0, "etag"));
}

#[tokio::test]
async fn a_playlist_coded_as_itself_is_written_into() {
    let (door, _upstream) = on_a_pass().await;
    let (status, answer) = get(door.at, &live("lfpass=good&shape=identity"), &[]).await;
    assert_eq!(status, 200);
    assert!(split(&answer).1.contains("seg.ts?x=1&lfpass=good\n"));
}

#[tokio::test]
async fn a_playlist_off_a_pass_goes_on_as_the_media_server_wrote_it() {
    let (door, upstream) = on_a_pass().await;
    let (status, answer) = get(door.at, "/web/list.m3u8", &["Accept-Encoding: gzip"]).await;
    assert_eq!(status, 200);
    let (headers, body) = split(&answer);
    assert_eq!(body, PLAYLIST);
    assert!(holds(&headers, "etag"));
    let forwarded = upstream.sent();
    assert_eq!(
        forwarded
            .iter()
            .map(|one| one.header("accept-encoding"))
            .collect::<Vec<_>>(),
        vec![vec!["gzip".to_owned()]]
    );
}
