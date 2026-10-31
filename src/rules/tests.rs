use lemonfiber_ports::http::Method;
use lemonfiber_ports::service::{Addressee, Answers, Held, Naming, Presentation, Statuses};

use super::{answered, Carrying, Check, Decision, Going, Presenting, Rules, Uncompiled, REFUSED};
use crate::fixtures::{self, ITEM, OTHER, PASS, TOKEN, UPSTREAM};

fn jellyfin() -> Rules {
    Rules::of(fixtures::guarded(fixtures::declared())).unwrap_or_else(|why| {
        unreachable!("the fixture compiles: {why:?}");
    })
}

fn decided(raw: &str) -> Decision {
    jellyfin().decide("GET", raw, &[], &[])
}

fn bearing(raw: &str, authorization: &[&str]) -> Decision {
    let headers: Vec<&[u8]> = authorization.iter().map(|one| one.as_bytes()).collect();
    jellyfin().decide("GET", raw, &headers, &[])
}

fn question(asked: &[&str]) -> Check {
    Check::Question {
        to: UPSTREAM.to_owned(),
        method: Method::Get,
        asked: asked.iter().map(|one| (*one).to_owned()).collect(),
        answers: fixtures::declared().question.map_or_else(
            || unreachable!("the fixture asks"),
            |question| question.answers,
        ),
        carrying: Carrying::Request,
    }
}

fn forward(target: &str, check: Check) -> Decision {
    going(target, Presenting::Unchanged, check)
}

fn going(target: &str, presenting: Presenting, check: Check) -> Decision {
    Decision::Forward(Box::new(Going {
        target: target.to_owned(),
        presenting,
        check,
    }))
}

fn presenting_of(decision: Decision) -> Presenting {
    match decision {
        Decision::Forward(going) => going.presenting,
        Decision::Refuse(status) => unreachable!("refused with {status}"),
    }
}

fn check_of(decision: Decision) -> Check {
    match decision {
        Decision::Forward(going) => going.check,
        Decision::Refuse(status) => unreachable!("refused with {status}"),
    }
}

#[test]
fn a_target_out_of_the_normal_form_is_refused_before_anything_reads_it() {
    for raw in [
        "http://jellyfin/Videos",
        &format!("/Videos/{ITEM}/../stream"),
        &format!("/Videos//{ITEM}"),
        &format!("/Videos/{ITEM};x/stream"),
        "/Videos/a%5cb",
        "/%56ideos/a",
        "/Videos/%2e%2e/Items",
        "/Videos/a%252fb",
        "/Videos/ö",
        "/Items?api%5Fkey=1",
    ] {
        assert_eq!(decided(raw), Decision::Refuse(REFUSED), "{raw}");
    }
}

#[test]
fn a_refused_path_is_refused_whether_its_name_arrived_encoded_or_not() {
    for raw in [
        "/Artists/AC%2FDC/Images/Primary",
        "/Artists/Abba/Images/Primary",
        "/emby/Persons/x/Images/Primary",
        "/Branding/Splashscreen",
        "/MEDIABROWSER/branding/splashscreen/",
    ] {
        assert_eq!(decided(raw), Decision::Refuse(REFUSED), "{raw}");
    }
}

#[test]
fn a_name_holding_an_encoded_delimiter_off_the_byte_paths_goes_on_as_it_came() {
    assert_eq!(
        decided("/Artists/AC%2FDC?userId=1"),
        forward("/Artists/AC%2FDC?userId=1", Check::Nothing)
    );
}

#[test]
fn a_byte_path_seen_only_once_decoded_is_refused() {
    for raw in [
        &format!("/Items/{ITEM}%2FImages"),
        &format!("/Videos%2F{ITEM}/stream"),
        "/Items/x%2FDownload",
    ] {
        assert_eq!(decided(raw), Decision::Refuse(REFUSED), "{raw}");
    }
}

#[test]
fn any_percent_on_a_byte_path_is_refused() {
    assert_eq!(
        decided(&format!("/Videos/{ITEM}/a%2Cb")),
        Decision::Refuse(REFUSED)
    );
}

#[test]
fn a_byte_path_asks_the_question_about_its_item() {
    assert_eq!(
        decided(&format!("/Videos/{ITEM}/stream?static=true")),
        forward(
            &format!("/Videos/{ITEM}/stream?static=true"),
            question(&[&format!("/Items/{ITEM}")])
        )
    );
    assert_eq!(
        decided(&format!("/emby/Items/{ITEM}/Images/Primary")),
        forward(
            &format!("/emby/Items/{ITEM}/Images/Primary"),
            question(&[&format!("/Items/{ITEM}")])
        )
    );
}

#[test]
fn an_item_of_another_shape_is_refused_without_asking() {
    for raw in [
        "/Videos/notanitem/stream",
        "/Videos/(0123456789abcdef0123456789abcdef)/stream",
        "/Items/x/Download",
    ] {
        assert_eq!(decided(raw), Decision::Refuse(REFUSED), "{raw}");
    }
}

#[test]
fn a_path_its_byte_path_exempts_goes_on_without_asking() {
    assert_eq!(
        decided("/Videos/ActiveEncodings?x=1"),
        forward("/Videos/ActiveEncodings?x=1", Check::Nothing)
    );
}

#[test]
fn a_parameter_naming_another_item_marked_refused_is_refused_in_any_case() {
    for query in [
        "itemId=x",
        "ITEMID=x",
        "LiveStreamId=1",
        "livestreamid",
        "a=1&itemid=2",
    ] {
        assert_eq!(
            decided(&format!("/Videos/{ITEM}/subtitles?{query}")),
            Decision::Refuse(REFUSED),
            "{query}"
        );
    }
}

#[test]
fn a_parameter_naming_another_item_marked_asked_is_asked_about_too() {
    assert_eq!(
        decided(&format!(
            "/Videos/{ITEM}/Trickplay/320/0.jpg?MediaSourceId={OTHER}"
        )),
        forward(
            &format!("/Videos/{ITEM}/Trickplay/320/0.jpg?MediaSourceId={OTHER}"),
            question(&[&format!("/Items/{ITEM}"), &format!("/Items/{OTHER}")])
        )
    );
}

#[test]
fn an_asked_parameter_twice_under_another_spelling_or_in_another_shape_is_refused() {
    for query in [
        format!("mediasourceid={OTHER}"),
        format!("MediaSourceId={OTHER}&MediaSourceId={ITEM}"),
        "MediaSourceId=nothex".to_owned(),
        "MediaSourceId".to_owned(),
    ] {
        assert_eq!(
            decided(&format!("/Videos/{ITEM}/Trickplay/320/0.jpg?{query}")),
            Decision::Refuse(REFUSED),
            "{query}"
        );
    }
}

#[test]
fn a_parameter_name_holding_anything_but_plain_characters_on_a_byte_path_is_refused() {
    for name in ["item+Id", "item[]", "a,b", "a:b"] {
        assert_eq!(
            decided(&format!("/Videos/{ITEM}/stream?{name}=1")),
            Decision::Refuse(REFUSED),
            "{name}"
        );
    }
}

#[test]
fn a_token_parameter_is_carried_into_the_question_as_it_came() {
    assert_eq!(
        decided(&format!(
            "/Videos/{ITEM}/stream?static=true&api_key={TOKEN}"
        )),
        forward(
            &format!("/Videos/{ITEM}/stream?static=true&api_key={TOKEN}"),
            question(&[&format!("/Items/{ITEM}?api_key={TOKEN}")])
        )
    );
}

#[test]
fn a_token_parameter_twice_under_another_spelling_or_in_another_shape_is_refused() {
    for query in [
        format!("API_KEY={TOKEN}"),
        format!("apikey={TOKEN}"),
        format!("ApiKey={TOKEN}&api_key={OTHER}"),
        format!("api_key={TOKEN}&api_key={TOKEN}"),
        "api_key=short".to_owned(),
        "api_key".to_owned(),
    ] {
        assert_eq!(
            decided(&format!("/Users/Me?{query}")),
            Decision::Refuse(REFUSED),
            "{query}"
        );
    }
}

#[test]
fn a_bearer_token_is_presented_as_the_declaration_says() {
    let raw = format!("/Videos/{ITEM}/stream");
    let presented = |presentation: Presentation, authorization: &str| {
        Rules::of(fixtures::presenting(presentation))
            .unwrap_or_else(|why| unreachable!("compiles: {why:?}"))
            .decide("GET", &raw, &[authorization.as_bytes()], &[])
    };
    assert_eq!(
        presenting_of(bearing(&raw, &[&format!("Bearer {TOKEN}")])),
        Presenting::Header {
            name: "Authorization".to_owned(),
            value: format!(r#"MediaBrowser Token="{TOKEN}""#),
        }
    );
    assert_eq!(
        presented(Presentation::AsIs, &format!("bearer {TOKEN}")),
        forward(&raw, question(&[&format!("/Items/{ITEM}")]))
    );
    assert_eq!(
        presented(
            Presentation::Query {
                parameter: "api_key".to_owned()
            },
            &format!("BEARER {TOKEN}")
        ),
        going(
            &format!("{raw}?api_key={TOKEN}"),
            Presenting::Removed,
            question(&[&format!("/Items/{ITEM}?api_key={TOKEN}")])
        )
    );
}

#[test]
fn a_bearer_value_of_another_shape_or_another_scheme_is_left_as_it_came() {
    for authorization in [
        "Bearer short".to_owned(),
        format!("Bearer  {TOKEN}"),
        format!(r#"MediaBrowser Token="{TOKEN}""#),
        "Bear".to_owned(),
    ] {
        assert_eq!(
            presenting_of(bearing(
                &format!("/Videos/{ITEM}/stream"),
                &[&authorization]
            )),
            Presenting::Unchanged,
            "{authorization}"
        );
    }
    let invalid: &[u8] = b"Bearer \xff";
    assert_eq!(
        presenting_of(jellyfin().decide("GET", "/Users", &[invalid], &[])),
        Presenting::Unchanged
    );
}

#[test]
fn a_request_presenting_a_token_more_than_once_is_refused() {
    let bearer = format!("Bearer {TOKEN}");
    assert_eq!(
        bearing("/Users", &[&bearer, &bearer]),
        Decision::Refuse(REFUSED)
    );
    assert_eq!(
        bearing("/Users", &["MediaBrowser Client=\"x\"", "Basic eA=="]),
        Decision::Refuse(REFUSED)
    );
    assert_eq!(
        bearing(&format!("/Users?ApiKey={OTHER}"), &[&bearer]),
        Decision::Refuse(REFUSED)
    );
    assert_eq!(
        presenting_of(bearing(
            &format!("/Users?ApiKey={TOKEN}"),
            &[r#"MediaBrowser Client="x", Token="y""#]
        )),
        Presenting::Unchanged
    );
}

#[test]
fn a_live_stream_goes_on_without_its_pass_once_the_pass_service_is_asked() {
    let stream = "0123456789abcdef0123456789abcdef";
    assert_eq!(
        decided(&format!(
            "/LiveTv/LiveStreamFiles/{stream}/stream.ts?a=1&lfpass=p.1.m&b=2"
        )),
        forward(
            &format!("/LiveTv/LiveStreamFiles/{stream}/stream.ts?a=1&b=2"),
            Check::Pass {
                at: PASS.to_owned()
            }
        )
    );
    assert_eq!(
        decided(&format!(
            "/LiveTv/LiveStreamFiles/{stream}/stream.m3u8?lfpass=p"
        )),
        forward(
            &format!("/LiveTv/LiveStreamFiles/{stream}/stream.m3u8"),
            Check::Pass {
                at: PASS.to_owned()
            }
        )
    );
}

#[test]
fn a_live_stream_without_its_pass_once_in_its_spelling_or_of_another_shape_is_refused() {
    let stream = "0123456789abcdef0123456789abcdef";
    for raw in [
        format!("/LiveTv/LiveStreamFiles/{stream}/stream.ts"),
        format!("/LiveTv/LiveStreamFiles/{stream}/stream.ts?LFPASS=p"),
        format!("/LiveTv/LiveStreamFiles/{stream}/stream.ts?lfpass=p&lfpass=q"),
        format!("/LiveTv/LiveStreamFiles/{stream}/stream.ts?lfpass="),
        format!("/LiveTv/LiveStreamFiles/{stream}/stream.ts?lfpass"),
        format!("/LiveTv/LiveStreamFiles/{stream}/stream.ts?lfpass=p&a+b=1"),
        format!("/LiveTv/LiveStreamFiles/{stream}/stream.ts?lfpass=p&itemId=1"),
        format!("/LiveTv/LiveStreamFiles/{stream}/stream.ts?lfpass=p&LiveStreamId=1"),
        "/LiveTv/LiveStreamFiles/NOTHEX/stream.ts?lfpass=p".to_owned(),
        format!("/LiveTv/LiveStreamFiles/{stream}%2Fx/stream.ts?lfpass=p"),
    ] {
        assert_eq!(decided(&raw), Decision::Refuse(REFUSED), "{raw}");
    }
}

#[test]
fn every_other_path_goes_on_as_it_came() {
    for raw in ["/web/index.html", "/Users/Me?x=1&&y", "/System/Info/Public"] {
        assert_eq!(decided(raw), forward(raw, Check::Nothing), "{raw}");
    }
}

#[test]
fn the_question_goes_to_the_adapter_where_the_declaration_sends_it_there() {
    let mut declared = fixtures::declared();
    if let Some(question) = declared.question.as_mut() {
        question.to = Addressee::Adapter;
    }
    for path in &mut declared.bytes {
        path.space = Some("item".to_owned());
    }
    let mut guarded = fixtures::guarded(declared.clone());
    let rules = Rules::of(guarded.clone()).unwrap_or_else(|why| unreachable!("{why:?}"));
    let raw = format!("/Videos/{ITEM}/stream");
    assert_eq!(
        rules.decide("GET", &raw, &[], &[]),
        Decision::Refuse(REFUSED)
    );
    guarded.adapter = Some("http://jellyfin-adapter:8080".to_owned());
    let rules = Rules::of(guarded).unwrap_or_else(|why| unreachable!("{why:?}"));
    let Check::Question { to, .. } = check_of(rules.decide("GET", &raw, &[], &[])) else {
        unreachable!("asks");
    };
    assert_eq!(to, "http://jellyfin-adapter:8080");
    declared.question = None;
    let rules =
        Rules::of(fixtures::guarded(declared)).unwrap_or_else(|why| unreachable!("{why:?}"));
    assert_eq!(
        rules.decide("GET", &raw, &[], &[]),
        Decision::Refuse(REFUSED)
    );
}

#[test]
fn a_question_path_holding_a_query_takes_the_token_after_it() {
    let mut declared = fixtures::declared();
    if let Some(question) = declared.question.as_mut() {
        question.path = "/Items/{item}?fields=x".to_owned();
    }
    let rules =
        Rules::of(fixtures::guarded(declared)).unwrap_or_else(|why| unreachable!("{why:?}"));
    let Check::Question { asked, .. } = check_of(rules.decide(
        "GET",
        &format!("/Videos/{ITEM}/stream?ApiKey={TOKEN}"),
        &[],
        &[],
    )) else {
        unreachable!("asks");
    };
    assert_eq!(
        asked,
        vec![format!("/Items/{ITEM}?fields=x&ApiKey={TOKEN}")]
    );
}

#[test]
fn an_answer_is_read_by_the_class_it_is_declared_in_and_refused_in_none() {
    let answers = Answers {
        may: vec![Statuses { from: 200, to: 299 }],
        may_not: vec![Statuses::only(404)],
        no_session: vec![Statuses::only(401)],
        not_from_there: vec![Statuses::only(403)],
    };
    assert_eq!(answered(&answers, 200), Ok(()));
    assert_eq!(answered(&answers, 206), Ok(()));
    assert_eq!(answered(&answers, 404), Err(404));
    assert_eq!(answered(&answers, 401), Err(401));
    assert_eq!(answered(&answers, 403), Err(403));
    for unlisted in [302, 304, 400, 500, 502] {
        assert_eq!(answered(&answers, unlisted), Err(REFUSED), "{unlisted}");
    }
}

#[test]
fn a_pattern_that_does_not_compile_leaves_the_envelope_uncompiled() {
    let broken = "(unclosed";
    let cases: [fn(&mut lemonfiber_ports::service::Declared, &str); 7] = [
        |declared, broken| declared.refused.push(broken.to_owned()),
        |declared, broken| {
            if let Some(path) = declared.bytes.first_mut() {
                path.pattern = broken.to_owned();
            }
        },
        |declared, broken| {
            if let Some(path) = declared.bytes.first_mut() {
                path.item = broken.to_owned();
            }
        },
        |declared, broken| {
            if let Some(path) = declared.bytes.first_mut() {
                path.exempt.push(broken.to_owned());
            }
        },
        |declared, broken| {
            if let Some(path) = declared.bytes.first_mut() {
                path.naming.push(Naming {
                    parameter: "x".to_owned(),
                    held: Held::Asked {
                        shape: broken.to_owned(),
                    },
                });
            }
        },
        |declared, broken| {
            if let Some(path) = declared.live.first_mut() {
                path.pattern = broken.to_owned();
            }
        },
        |declared, broken| declared.token.shape = broken.to_owned(),
    ];
    for (at, case) in cases.iter().enumerate() {
        let mut declared = fixtures::declared();
        case(&mut declared, broken);
        assert_eq!(
            Rules::of(fixtures::guarded(declared)).map(|_| ()),
            Err(Uncompiled(broken.to_owned())),
            "case {at}"
        );
    }
    let mut declared = fixtures::declared();
    if let Some(path) = declared.live.first_mut() {
        path.stream = broken.to_owned();
    }
    assert!(Rules::of(fixtures::guarded(declared)).is_err());
}

#[test]
fn the_rules_hold_the_envelope_they_were_compiled_from() {
    assert_eq!(
        jellyfin().guarded(),
        &fixtures::guarded(fixtures::declared())
    );
}

#[test]
fn a_pattern_that_does_not_compile_is_named_in_the_refusal() {
    assert_eq!(
        String::from(Uncompiled("(x".to_owned())),
        "the pattern (x does not compile"
    );
}

mod adapter;
mod differing;
mod injecting;
mod normalising;
mod querying;
mod tokens;
