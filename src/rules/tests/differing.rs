use lemonfiber_ports::service::{BytePath, InQuery, Presentation};

use super::{check_of, Check, Decision, Rules, REFUSED};
use crate::fixtures::{self, ITEM, OTHER, TOKEN};

const START: &str = "/video/:/transcode/universal/start";

fn stream() -> String {
    format!("/Videos/{ITEM}/stream")
}

fn rules_with(declared: lemonfiber_ports::service::Declared) -> Rules {
    Rules::of(fixtures::guarded(declared)).unwrap_or_else(|why| unreachable!("{why:?}"))
}

#[test]
fn a_request_that_changes_what_a_byte_path_serves_is_refused_whatever_its_question_answers() {
    let rules = rules_with(fixtures::declared());
    for method in ["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        assert_eq!(
            rules.decide(method, &stream(), &[], &[]),
            Decision::Refuse(REFUSED),
            "{method}"
        );
    }
    for method in ["GET", "HEAD"] {
        assert!(
            matches!(
                check_of(rules.decide(method, &stream(), &[], &[])),
                Check::Question { .. }
            ),
            "{method}"
        );
    }
}

#[test]
fn a_path_a_byte_path_exempts_or_none_covers_goes_on_with_any_method() {
    let rules = rules_with(fixtures::declared());
    for raw in ["/Videos/ActiveEncodings?deviceId=1", "/Sessions/Playing"] {
        assert!(
            matches!(rules.decide("DELETE", raw, &[], &[]), Decision::Forward(_)),
            "{raw}"
        );
    }
}

#[test]
fn a_token_in_the_parameter_the_token_is_presented_in_is_one_the_request_carries() {
    let mut declared = fixtures::declared();
    declared.token.presented = Presentation::Query {
        parameter: "X-Plex-Token".to_owned(),
    };
    declared.token.parameters = Vec::new();
    let rules = rules_with(declared);
    let bearer = format!("Bearer {OTHER}");
    for (raw, authorization) in [
        (
            format!("{}?X-Plex-Token={TOKEN}", stream()),
            vec![bearer.as_bytes()],
        ),
        (format!("{}?x-plex-token={TOKEN}", stream()), vec![]),
        (
            format!("{}?X-Plex-Token={TOKEN}&X-Plex-Token={TOKEN}", stream()),
            vec![],
        ),
        (format!("{}?X-Plex-Token=not-a-token", stream()), vec![]),
    ] {
        assert_eq!(
            rules.decide("GET", &raw, &authorization, &[]),
            Decision::Refuse(REFUSED),
            "{raw}"
        );
    }
}

#[test]
fn a_token_in_a_query_value_that_would_read_otherwise_once_decoded_is_refused() {
    let mut declared = fixtures::declared();
    declared.token.shape = "^[0-9A-Za-z+/=%]{8,64}$".to_owned();
    let rules = rules_with(declared);
    for token in ["abcd+efgh", "abcd%2Befgh"] {
        let raw = format!("{}?api_key={token}", stream());
        assert_eq!(
            rules.decide("GET", &raw, &[], &[]),
            Decision::Refuse(REFUSED),
            "{raw}"
        );
    }
}

#[test]
fn a_query_value_that_reaches_another_path_once_its_segments_are_read_is_refused() {
    let mut declared = fixtures::declared();
    declared.bytes.push(BytePath {
        pattern: format!("^{START}$"),
        query: Some(InQuery {
            parameter: "url".to_owned(),
            pattern: "(?:[a-z]+://[^/]+)?/library/metadata/(?P<item>[0-9]+)(?:/.*)?".to_owned(),
        }),
        space: None,
        item: "^[0-9]+$".to_owned(),
        exempt: Vec::new(),
        naming: Vec::new(),
    });
    let rules = rules_with(declared);
    for value in [
        "/library/metadata/1/../../parts/9",
        "/library/metadata/1/./x",
        "/library/metadata/1//x",
        "/library/metadata/1/a;b",
        "/library/metadata/1/a%5Cb",
        "/library/metadata/1/a%09b",
        "http://127.0.0.1:32400/library/metadata/1/..",
    ] {
        let raw = format!("{START}?url={value}");
        assert_eq!(
            rules.decide("GET", &raw, &[], &[]),
            Decision::Refuse(REFUSED),
            "{raw}"
        );
    }
    for value in [
        "/library/metadata/1",
        "/library/metadata/1/thumb",
        "http://127.0.0.1:32400/library/metadata/1",
    ] {
        let raw = format!("{START}?url={value}");
        assert!(
            matches!(
                check_of(rules.decide("GET", &raw, &[], &[])),
                Check::Question { .. }
            ),
            "{raw}"
        );
    }
}

fn loose_transcoder() -> Rules {
    let mut declared = fixtures::declared();
    declared.bytes.push(BytePath {
        pattern: format!("^{START}$"),
        query: Some(InQuery {
            parameter: "url".to_owned(),
            pattern: "/library/metadata/(?P<item>[0-9]+).*".to_owned(),
        }),
        space: None,
        item: "^[0-9]+$".to_owned(),
        exempt: Vec::new(),
        naming: Vec::new(),
    });
    rules_with(declared)
}

#[test]
fn a_query_value_hiding_a_path_behind_a_fragment_or_a_plus_is_refused() {
    for value in [
        "/library/metadata/1%23/../../parts/9",
        "/library/metadata/1%23x",
        "/library/metadata/1+",
        "/library/metadata/1/a+b",
    ] {
        let raw = format!("{START}?url={value}");
        assert_eq!(
            loose_transcoder().decide("GET", &raw, &[], &[]),
            Decision::Refuse(REFUSED),
            "{raw}"
        );
    }
}

#[test]
fn an_authorization_header_the_adapter_question_does_not_carry_is_refused() {
    let mut guarded = fixtures::guarded(fixtures::declared());
    guarded.declared.question = Some(lemonfiber_ports::service::Question {
        method: lemonfiber_ports::http::Method::Get,
        path: lemonfiber_ports::service::MAY.to_owned(),
        to: lemonfiber_ports::service::Addressee::Adapter,
        answers: lemonfiber_ports::service::Answers::from_adapter(),
    });
    guarded.adapter = Some("http://plex-adapter:8080".to_owned());
    for path in &mut guarded.declared.bytes {
        path.space = Some("item".to_owned());
    }
    guarded.declared.token.presented = Presentation::Header {
        name: "X-Plex-Token".to_owned(),
        value: "{token}".to_owned(),
    };
    let rules = Rules::of(guarded).unwrap_or_else(|why| unreachable!("{why:?}"));
    let raw = format!("{}?api_key={TOKEN}", stream());
    for authorization in [
        "MediaBrowser Token=\"other\"",
        "Bearer not-a-token",
        "Basic Zm9v",
    ] {
        assert_eq!(
            rules.decide("GET", &raw, &[authorization.as_bytes()], &[]),
            Decision::Refuse(REFUSED),
            "{authorization}"
        );
    }
    assert!(matches!(
        check_of(rules.decide("GET", &raw, &[], &[])),
        Check::Question { .. }
    ));
}
