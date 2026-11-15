use lemonfiber_ports::service::LivePath;

use super::{passing, question, Decision, Going, Opening, Presenting, Rules, REFUSED};
use crate::fixtures::{self, ITEM, OTHER, PASS, TOKEN};

const STREAM: &str = "0123456789abcdef0123456789abcdef_0123456789abcdef0123456789abcdef_0123456789abcdef0123456789abcdef";

fn transcoded() -> Rules {
    let mut declared = fixtures::declared();
    declared.live.push(LivePath {
        pattern: "(?i)^/Videos/[^/]+/(?:(?:live|master|main)\\.m3u8$|hls1?/)".to_owned(),
        parameter: Some("LiveStreamId".to_owned()),
        stream: "^[0-9a-f]{32}_[0-9a-f]{32}_[0-9a-f]{32}$".to_owned(),
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
