use super::{is_playlist, written};

const PASS: &str = "bWVtYmVy.1700000000.bWFj";

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
    let playlist = "#EXTM3U\r\n#EXT-X-VERSION:3\r\n\r\n#EXTINF:6.0,\r\nhls1/main/0.ts?a=1&b=2\r\n#EXTINF:6.0,\r\n  /abs/1.ts  \r\nhttps://h/2.ts#frag\n# a comment URI=\"left.ts\"\nlast.ts";
    assert_eq!(
        written(playlist, PASS),
        format!(
            "#EXTM3U\r\n#EXT-X-VERSION:3\r\n\r\n#EXTINF:6.0,\r\nhls1/main/0.ts?a=1&b=2&lfpass={PASS}\r\n#EXTINF:6.0,\r\n  /abs/1.ts?lfpass={PASS}  \r\nhttps://h/2.ts?lfpass={PASS}#frag\n# a comment URI=\"left.ts\"\nlast.ts?lfpass={PASS}"
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
