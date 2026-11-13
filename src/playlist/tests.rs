use super::{holds, is_media, is_playlist, Origin, LINES_MOST, MOST};

const PASS: &str = "bWVtYmVy.1700000000.bWFj";

const DOOR: Origin<'static> = Origin {
    scheme: "https",
    authority: Some("media.home.local:8920"),
};

fn written(playlist: &str, pass: &str) -> String {
    super::written(playlist, &[("lfpass", pass)], &[], DOOR)
        .unwrap_or_else(|| unreachable!("written"))
}

#[test]
fn a_playlist_is_known_by_its_media_type_in_any_case() {
    for kind in [
        "application/vnd.apple.mpegurl",
        "Application/X-MpegURL",
        "audio/mpegurl; charset=utf-8",
        " AUDIO/MPEGURL ",
    ] {
        assert!(is_playlist(kind), "{kind}");
    }
    for kind in [
        "video/mp2t",
        "text/plain",
        "application/vnd.apple.mpegurl+json",
        "",
    ] {
        assert!(!is_playlist(kind), "{kind}");
    }
}

#[test]
fn every_address_line_carries_the_pass_last_and_nothing_else_changes() {
    let playlist = "#EXTM3U\r\n#EXT-X-VERSION:3\r\n\r\n#EXTINF:6.0,\r\nhls1/main/0.ts?a=1&b=2\r\n#EXTINF:6.0,\r\n  /abs/1.ts  \r\n2.ts#frag\n# a comment URI=\"left.ts\"\nlast.ts";
    assert_eq!(
        written(playlist, PASS),
        format!(
            "#EXTM3U\r\n#EXT-X-VERSION:3\r\n\r\n#EXTINF:6.0,\r\nhls1/main/0.ts?a=1&b=2&lfpass={PASS}\r\n#EXTINF:6.0,\r\n  /abs/1.ts?lfpass={PASS}  \r\n2.ts?lfpass={PASS}#frag\n# a comment URI=\"left.ts\"\nlast.ts?lfpass={PASS}"
        )
    );
}

#[test]
fn every_quoted_uri_attribute_carries_the_pass_and_no_other_attribute_does() {
    let playlist = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key?k=1\",IV=0x1\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXT-X-MEDIA:TYPE=AUDIO,NAME=\"a,URI=x\",URI=\"audio.m3u8\",XURI=\"no\"\n#EXT-X-STREAM-INF:BANDWIDTH=1,CODECS=\"a,b\"\nmain.m3u8?MediaSourceId=1\n#EXT-X-ENDLIST\n#EXT-BROKEN:URI=\"open\n";
    assert_eq!(
        written(playlist, PASS),
        format!(
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key?k=1&lfpass={PASS}\",IV=0x1\n#EXT-X-MAP:URI=\"init.mp4?lfpass={PASS}\"\n#EXT-X-MEDIA:TYPE=AUDIO,NAME=\"a,URI=x\",URI=\"audio.m3u8?lfpass={PASS}\",XURI=\"no\"\n#EXT-X-STREAM-INF:BANDWIDTH=1,CODECS=\"a,b\"\nmain.m3u8?MediaSourceId=1&lfpass={PASS}\n#EXT-X-ENDLIST\n#EXT-BROKEN:URI=\"open\n"
        )
    );
}

#[test]
fn an_address_already_carrying_a_pass_carries_only_the_one_written() {
    assert_eq!(
        written("a.ts?lfpass=old&x=1\nb.ts?LFPASS=old\nc.ts?\nd.ts?x=1&\n", PASS),
        format!("a.ts?x=1&lfpass={PASS}\nb.ts?lfpass={PASS}\nc.ts?lfpass={PASS}\nd.ts?x=1&lfpass={PASS}\n")
    );
}

#[test]
fn a_byte_order_mark_is_kept_and_not_read_as_an_address() {
    assert_eq!(
        written("\u{feff}#EXTM3U\nseg.ts\n", PASS),
        format!("\u{feff}#EXTM3U\nseg.ts?lfpass={PASS}\n")
    );
}

#[test]
fn a_tag_without_attributes_or_with_an_unfinished_one_is_kept() {
    for line in [
        "#EXT-X-ENDLIST",
        "#EXT-X-X:A",
        "#EXT-X-X:A=1,",
        "#EXTINF:6.0,title",
    ] {
        assert_eq!(written(line, PASS), line);
    }
}

#[test]
fn an_address_leading_anywhere_but_back_to_the_door_is_left_as_it_came() {
    let playlist = "https://media.home.local:8920/own.ts\nHTTPS://MEDIA.HOME.LOCAL:8920?x=1\nhttps://media.home.local/port.ts\nhttp://media.home.local:8920/scheme.ts\nhttps://elsewhere.example/off.ts\n//media.home.local:8920/relative-scheme.ts\ndata:text/plain,x\nskd://key\na/b:c.ts\n#EXT-X-KEY:METHOD=AES-128,URI=\"https://keys.example/k\"\n";
    assert_eq!(
        written(playlist, PASS),
        format!("https://media.home.local:8920/own.ts?lfpass={PASS}\nHTTPS://MEDIA.HOME.LOCAL:8920?x=1&lfpass={PASS}\nhttps://media.home.local/port.ts\nhttp://media.home.local:8920/scheme.ts\nhttps://elsewhere.example/off.ts\n//media.home.local:8920/relative-scheme.ts\ndata:text/plain,x\nskd://key\na/b:c.ts?lfpass={PASS}\n#EXT-X-KEY:METHOD=AES-128,URI=\"https://keys.example/k\"\n")
    );
}

#[test]
fn an_absolute_address_is_left_as_it_came_where_the_request_named_no_authority() {
    let nameless = Origin {
        scheme: "http",
        authority: None,
    };
    assert_eq!(
        super::written(
            "http://media.home.local/a.ts\nb.ts\n",
            &[("lfpass", PASS)],
            &[],
            nameless
        ),
        Some(format!(
            "http://media.home.local/a.ts\nb.ts?lfpass={PASS}\n"
        ))
    );
}

#[test]
fn a_playlist_of_more_lines_than_the_door_writes_into_is_not_written() {
    let most = "#\n".repeat(LINES_MOST);
    assert!(super::written(&most, &[("lfpass", PASS)], &[], DOOR).is_some());
    let more = format!("{most}#");
    assert_eq!(super::written(&more, &[("lfpass", PASS)], &[], DOOR), None);
}

#[test]
fn a_playlist_that_would_grow_beyond_what_the_door_answers_is_not_written() {
    let line = format!("{}\n", "s".repeat(79));
    let lines = MOST / line.len() - 1;
    let playlist = line.repeat(lines.min(LINES_MOST));
    assert!(playlist.len() <= MOST);
    let long = "p".repeat(1024);
    assert_eq!(
        super::written(&playlist, &[("lfpass", &long)], &[], DOOR),
        None
    );
}

#[test]
fn a_live_stream_parameter_is_written_beside_the_pass_as_the_request_carried_it() {
    let carried = [("lfpass", PASS), ("LiveStreamId", "a%5Fb")];
    assert_eq!(
        super::written(
            "#EXTM3U\nhls1/main/0.ts?livestreamid=old&x=1\n#EXT-X-MAP:URI=\"init.mp4\"\n",
            &carried,
            &[],
            DOOR
        ),
        Some(format!(
            "#EXTM3U\nhls1/main/0.ts?x=1&lfpass={PASS}&LiveStreamId=a%5Fb\n#EXT-X-MAP:URI=\"init.mp4?lfpass={PASS}&LiveStreamId=a%5Fb\"\n"
        ))
    );
}

#[test]
fn every_token_parameter_is_taken_off_every_address_in_any_case_once_decoded() {
    let stripped = ["ApiKey", "api_key"];
    assert_eq!(
        super::written(
            "#EXT-X-MAP:URI=\"init.mp4?ApiKey=t\"\nmain.m3u8?a=1&api_key=t\nseg.ts?APIKEY=t&x=1\nseg2.ts?api%5Fkey=t\nseg3.ts?api_keys=t\nhttps://elsewhere.example/off.ts?api_key=t\n",
            &[("lfpass", PASS)],
            &stripped,
            DOOR
        ),
        Some(format!(
            "#EXT-X-MAP:URI=\"init.mp4?lfpass={PASS}\"\nmain.m3u8?a=1&lfpass={PASS}\nseg.ts?x=1&lfpass={PASS}\nseg2.ts?lfpass={PASS}\nseg3.ts?api_keys=t&lfpass={PASS}\nhttps://elsewhere.example/off.ts?api_key=t\n"
        ))
    );
}

#[test]
fn only_a_type_media_alone_is_served_as_is_media() {
    for kind in [
        "video/mp2t",
        "Audio/AAC",
        "image/jpeg; q=1",
        "application/octet-stream",
        "application/mp4",
    ] {
        assert!(is_media(kind), "{kind}");
    }
    for kind in [
        "text/vtt",
        "image/svg+xml",
        "application/json",
        "video/x+json",
        "",
    ] {
        assert!(!is_media(kind), "{kind}");
    }
}

#[test]
fn a_secret_is_found_anywhere_in_any_case() {
    assert!(holds(b"x=ABCDEF&y", "abcdef"));
    assert!(holds(b"abcdef", "abcdef"));
    assert!(!holds(b"abcde", "abcdef"));
    assert!(holds(b"", ""));
}
