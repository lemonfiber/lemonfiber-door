use lemonfiber_ports::service::{Bound, BytePath, InQuery, LivePath};

use super::{passing, question, Decision, Going, Opening, Presenting, Rules, REFUSED};
use crate::fixtures::{self, ITEM, OTHER, PASS, TOKEN};

const STREAM: &str = "0123456789abcdef0123456789abcdef_0123456789abcdef0123456789abcdef_0123456789abcdef0123456789abcdef";

fn transcoded() -> Rules {
    let mut declared = fixtures::declared();
    declared.live.push(LivePath {
        pattern: "(?i)^/Videos/[^/]+/(?:(?:live|master|main)\\.m3u8$|hls1?/)".to_owned(),
        parameter: Some("LiveStreamId".to_owned()),
        stream: "^[0-9a-f]{32}_[0-9a-f]{32}_[0-9a-f]{32}$".to_owned(),
        bound: Vec::new(),
    });
    Rules::of(fixtures::guarded(declared)).unwrap_or_else(|why| unreachable!("{why:?}"))
}

fn bearing(raw: &str) -> Decision {
    let bearer = format!("Bearer {TOKEN}");
    transcoded().decide("GET", raw, &[bearer.as_bytes()], &[])
}

fn decided(raw: &str) -> Decision {
    transcoded().decide("GET", raw, &[], &[])
}

fn live(target: &str, item: &str, written: &[(&str, &str)]) -> Decision {
    Decision::Forward(Box::new(Going {
        target: target.to_owned(),
        presenting: Presenting::Header {
            name: "Authorization".to_owned(),
            value: format!(r#"MediaBrowser Token="{TOKEN}""#),
        },
        check: question(&[&format!("/Items/{item}")]),
        pass_service: Some(PASS.to_owned()),
        writing: Some(super::writing(written, None)),
    }))
}

#[test]
fn a_live_stream_in_its_declared_parameter_is_asked_of_the_pass_service_and_its_item_of_the_question(
) {
    for path in ["live.m3u8", "main.m3u8", "hls1/main/0.ts"] {
        let raw = format!("/Videos/{ITEM}/{path}?LiveStreamId={STREAM}&lfpass=p.1.m&x=1");
        assert_eq!(
            bearing(&raw),
            live(
                &format!("/Videos/{ITEM}/{path}?LiveStreamId={STREAM}&x=1"),
                ITEM,
                &[("lfpass", "p.1.m"), ("LiveStreamId", STREAM)]
            ),
            "{path}"
        );
    }
    let decoded = STREAM.replacen('_', "%5F", 1);
    assert_eq!(
        bearing(&format!(
            "/Videos/{OTHER}/live.m3u8?LiveStreamId={decoded}&lfpass=p"
        )),
        live(
            &format!("/Videos/{OTHER}/live.m3u8?LiveStreamId={decoded}"),
            OTHER,
            &[("lfpass", "p"), ("LiveStreamId", &decoded)]
        )
    );
}

#[test]
fn a_live_stream_parameter_carried_twice_respelt_encoded_twice_or_out_of_shape_is_refused() {
    let twice = format!("LiveStreamId={STREAM}&LiveStreamId={STREAM}");
    let respelt = format!("livestreamid={STREAM}");
    let encoded_twice = format!("LiveStreamId={}", STREAM.replacen('_', "%255F", 1));
    let plus = format!("LiveStreamId={}", STREAM.replacen('_', "+", 1));
    for query in [
        twice.as_str(),
        respelt.as_str(),
        encoded_twice.as_str(),
        plus.as_str(),
        "LiveStreamId=0123",
        "LiveStreamId=",
        "LiveStreamId",
        "LiveStreamId=%FF",
    ] {
        let raw = format!("/Videos/{ITEM}/live.m3u8?{query}&lfpass=p");
        assert_eq!(bearing(&raw), Decision::Refuse(REFUSED), "{query}");
    }
    let unpassed = format!("/Videos/{ITEM}/live.m3u8?LiveStreamId={STREAM}");
    assert_eq!(bearing(&unpassed), Decision::Refuse(REFUSED));
    let naming = format!("/Videos/{ITEM}/live.m3u8?LiveStreamId={STREAM}&itemId=1&lfpass=p");
    assert_eq!(bearing(&naming), Decision::Refuse(REFUSED));
}

#[test]
fn a_live_stream_in_a_parameter_without_a_session_or_an_item_a_byte_path_places_is_refused() {
    let raw = format!("/Videos/{ITEM}/live.m3u8?LiveStreamId={STREAM}&lfpass=p");
    assert_eq!(decided(&raw), Decision::Refuse(REFUSED));
    for raw in [
        format!("/Videos/notanitem/live.m3u8?LiveStreamId={STREAM}&lfpass=p"),
        format!("/Videos/ActiveEncodings/live.m3u8?LiveStreamId={STREAM}&lfpass=p"),
    ] {
        assert_eq!(bearing(&raw), Decision::Refuse(REFUSED), "{raw}");
    }
    let mut declared = fixtures::declared();
    declared.live.push(LivePath {
        pattern: "^/elsewhere/".to_owned(),
        parameter: Some("LiveStreamId".to_owned()),
        stream: "^[0-9a-f_]+$".to_owned(),
        bound: Vec::new(),
    });
    let rules =
        Rules::of(fixtures::guarded(declared)).unwrap_or_else(|why| unreachable!("{why:?}"));
    let bearer = format!("Bearer {TOKEN}");
    assert_eq!(
        rules.decide(
            "GET",
            &format!("/elsewhere/x?LiveStreamId={STREAM}&lfpass=p"),
            &[bearer.as_bytes()],
            &[]
        ),
        Decision::Refuse(REFUSED)
    );
}

#[test]
fn a_request_without_the_live_stream_parameter_is_read_against_the_byte_paths() {
    assert_eq!(
        decided(&format!("/Videos/{ITEM}/master.m3u8?MediaSourceId={ITEM}")),
        super::forward(
            &format!("/Videos/{ITEM}/master.m3u8?MediaSourceId={ITEM}"),
            question(&[&format!("/Items/{ITEM}")])
        )
    );
    assert_eq!(
        decided(&format!("/Videos/{ITEM}/master.m3u8?lfpass=p")),
        Decision::Open(Box::new(Opening {
            at: PASS.to_owned(),
            pass: "p".to_owned(),
            target: format!("/Videos/{ITEM}/master.m3u8"),
        }))
    );
    let stream = "0123456789abcdef0123456789abcdef";
    assert_eq!(
        decided(&format!(
            "/LiveTv/LiveStreamFiles/{stream}/stream.ts?lfpass=p"
        )),
        passing(&format!("/LiveTv/LiveStreamFiles/{stream}/stream.ts"), "p")
    );
    for method in ["POST", "DELETE"] {
        assert_eq!(
            transcoded().decide(
                method,
                &format!("/LiveTv/LiveStreamFiles/{stream}/stream.ts?lfpass=p"),
                &[],
                &[]
            ),
            Decision::Refuse(REFUSED),
            "{method}"
        );
    }
}

#[test]
fn a_query_parameter_name_spelt_otherwise_on_a_live_path_is_refused() {
    for name in ["LiveStream%49d", "lf%70ass", "Live+StreamId"] {
        for raw in [
            format!("/Videos/{ITEM}/live.m3u8?LiveStreamId={STREAM}&lfpass=p&{name}=1"),
            format!(
                "/LiveTv/LiveStreamFiles/0123456789abcdef0123456789abcdef/stream.ts?lfpass=p&{name}=1"
            ),
        ] {
            assert_eq!(bearing(&raw), Decision::Refuse(REFUSED), "{raw}");
        }
    }
}

#[test]
fn a_live_segment_is_asked_about_as_the_item_its_path_names() {
    let raw = format!(
        "/Videos/{OTHER}/hls/0123456789abcdef0123456789abcdef/fedcba9876543210fedcba98765432100.ts?LiveStreamId={STREAM}&lfpass=p"
    );
    let Decision::Forward(going) = bearing(&raw) else {
        unreachable!("forwarded");
    };
    assert_eq!(going.check, question(&[&format!("/Items/{OTHER}")]));
}

const SESSION: &str = "0f2a6c1e-8b3d-4e5f-9a7b-1c2d3e4f5a6b";

const TRANSCODER: &str = "/video/:/transcode/universal/start.m3u8";

fn reading(stream: &str) -> String {
    format!("%2Flivetv%2Fsessions%2F{stream}%2Fc1%2Findex.m3u8")
}

fn bound_transcoder() -> Rules {
    let mut declared = fixtures::declared();
    declared.bytes.push(BytePath {
        pattern: "(?i)^/video/:/transcode/universal/start\\.m3u8$".to_owned(),
        query: Some(InQuery {
            parameter: "path".to_owned(),
            pattern: "^/library/metadata/(?P<item>[0-9]+)$".to_owned(),
        }),
        space: None,
        item: "^[0-9]+$".to_owned(),
        exempt: Vec::new(),
        naming: Vec::new(),
    });
    declared.live.push(LivePath {
        pattern: "(?i)^/video/:/transcode/universal/start\\.m3u8$".to_owned(),
        parameter: Some("session".to_owned()),
        stream: "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$".to_owned(),
        bound: vec![Bound {
            parameter: "path".to_owned(),
            pattern: "^/livetv/sessions/(?P<stream>[^/]+)/[0-9a-z]+/index\\.m3u8$".to_owned(),
        }],
    });
    Rules::of(fixtures::guarded(declared)).unwrap_or_else(|why| unreachable!("{why:?}"))
}

#[test]
fn a_live_stream_the_items_parameter_also_names_goes_on_on_its_pass_with_no_item_question() {
    let carried = format!("session={SESSION}&path={}", reading(SESSION));

    assert_eq!(
        bound_transcoder().decide("GET", &format!("{TRANSCODER}?{carried}&lfpass=p"), &[], &[]),
        super::passing_with(
            &format!("{TRANSCODER}?{carried}"),
            &[("lfpass", "p"), ("session", SESSION)]
        )
    );
}

#[test]
fn a_bound_parameter_naming_another_live_stream_an_item_or_carried_other_than_once_is_refused() {
    let other = "1f2a6c1e-8b3d-4e5f-9a7b-1c2d3e4f5a6b";
    for query in [
        format!("session={SESSION}&path={}", reading(other)),
        format!("session={SESSION}&path=%2Flibrary%2Fmetadata%2F5"),
        format!("session={SESSION}"),
        format!(
            "session={SESSION}&path={}&path={}",
            reading(SESSION),
            reading(SESSION)
        ),
        format!("session={SESSION}&Path={}", reading(SESSION)),
        format!(
            "session={SESSION}&path={}",
            reading(SESSION).replace("%2F", "%252F")
        ),
    ] {
        let raw = format!("{TRANSCODER}?{query}&lfpass=p");
        assert_eq!(
            bound_transcoder().decide("GET", &raw, &[], &[]),
            Decision::Refuse(REFUSED),
            "{query}"
        );
    }
}

#[test]
fn a_bound_parameter_that_is_not_the_items_still_has_the_item_asked_about() {
    let mut declared = fixtures::declared();
    declared.live.push(LivePath {
        pattern: "(?i)^/Videos/[^/]+/(?:(?:live|master|main)\\.m3u8$|hls1?/)".to_owned(),
        parameter: Some("LiveStreamId".to_owned()),
        stream: "^[0-9a-f]{32}_[0-9a-f]{32}_[0-9a-f]{32}$".to_owned(),
        bound: vec![Bound {
            parameter: "Tag".to_owned(),
            pattern: "^(?P<stream>[0-9a-f_]+)$".to_owned(),
        }],
    });
    let rules =
        Rules::of(fixtures::guarded(declared)).unwrap_or_else(|why| unreachable!("{why:?}"));
    let bearer = format!("Bearer {TOKEN}");
    let decide = |raw: &str| rules.decide("GET", raw, &[bearer.as_bytes()], &[]);

    assert_eq!(
        decide(&format!(
            "/Videos/{ITEM}/live.m3u8?LiveStreamId={STREAM}&Tag={STREAM}&lfpass=p"
        )),
        live(
            &format!("/Videos/{ITEM}/live.m3u8?LiveStreamId={STREAM}&Tag={STREAM}"),
            ITEM,
            &[("lfpass", "p"), ("LiveStreamId", STREAM)]
        )
    );
    assert_eq!(
        decide(&format!(
            "/Videos/{ITEM}/live.m3u8?LiveStreamId={STREAM}&Tag=0a&lfpass=p"
        )),
        Decision::Refuse(REFUSED)
    );
}

#[test]
fn a_live_stream_its_parameter_names_goes_on_only_where_the_paths_own_group_names_it_too() {
    let mut declared = fixtures::declared();
    declared.live.push(LivePath {
        pattern: "(?i)^/video/:/transcode/universal/session/(?P<stream>[0-9a-f-]{36})/base/[0-9]{1,10}\\.ts$".to_owned(),
        parameter: Some("session".to_owned()),
        stream: "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$".to_owned(),
        bound: Vec::new(),
    });
    let rules =
        Rules::of(fixtures::guarded(declared)).unwrap_or_else(|why| unreachable!("{why:?}"));
    let segment =
        |stream: &str| format!("/video/:/transcode/universal/session/{stream}/base/00001.ts");
    let other = "1f2a6c1e-8b3d-4e5f-9a7b-1c2d3e4f5a6b";

    assert_eq!(
        rules.decide(
            "GET",
            &format!("{}?session={SESSION}&lfpass=p", segment(SESSION)),
            &[],
            &[]
        ),
        super::passing_with(
            &format!("{}?session={SESSION}", segment(SESSION)),
            &[("lfpass", "p"), ("session", SESSION)]
        )
    );
    assert_eq!(
        rules.decide(
            "GET",
            &format!("{}?session={SESSION}&lfpass=p", segment(other)),
            &[],
            &[]
        ),
        Decision::Refuse(REFUSED)
    );
}

#[test]
fn a_request_read_more_than_one_way_is_refused_before_a_bound_parameter_spares_its_item_question() {
    let other = "1f2a6c1e-8b3d-4e5f-9a7b-1c2d3e4f5a6b";
    let carried = format!("path={}&lfpass=p", reading(SESSION));
    for raw in [
        format!("{TRANSCODER}?session={SESSION};x=B&{carried}"),
        format!("{TRANSCODER}?session={SESSION}&{carried}&x=B;session={other}"),
        format!("/video/:/transcode/universal/start%2em3u8?session={SESSION}&{carried}"),
        format!("/video/:/transcode%2Funiversal/start.m3u8?session={SESSION}&{carried}"),
        format!("{TRANSCODER}?session={SESSION}&{carried}&na:me=1"),
        format!("{TRANSCODER}?session={SESSION}&{carried}&na%6De=1"),
    ] {
        assert_eq!(
            bound_transcoder().decide("GET", &raw, &[], &[]),
            Decision::Refuse(REFUSED),
            "{raw}"
        );
    }
}

#[test]
fn a_request_read_more_than_one_way_is_refused_on_a_live_path_an_item_path_or_an_exempt_one() {
    let stream = format!("LiveStreamId={STREAM}&lfpass=p");
    for raw in [
        format!("/Videos/{ITEM}/live.m3u8?{stream}&x=1;LiveStreamId={STREAM}"),
        format!("/Videos/{ITEM}/live%2em3u8?{stream}"),
        format!("/Videos/{ITEM}/live.m3u8?{stream}&na:me=1"),
        "/LiveTv/LiveStreamFiles/0123456789abcdef0123456789abcdef/stream.ts?lfpass=p&x=1;y=2"
            .to_owned(),
        format!("/Videos/{ITEM}/stream.mp4?x=1;y=2"),
        format!("/Videos/{ITEM}/stream%2emp4"),
        format!("/Videos/{ITEM}/stream.mp4?na:me=1"),
        "/Videos/ActiveEncodings?deviceId=1;x=2".to_owned(),
        "/Videos/ActiveEncodings?na:me=1".to_owned(),
    ] {
        assert_eq!(bearing(&raw), Decision::Refuse(REFUSED), "{raw}");
    }
}
