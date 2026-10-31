use lemonfiber_ports::service::{BytePath, InQuery};

use super::{check_of, question, Decision, Rules, REFUSED};
use crate::fixtures;

const START: &str = "/video/:/transcode/universal/start";

fn transcoding(pattern: &str, item: &str) -> Rules {
    let mut declared = fixtures::declared();
    declared.bytes.push(BytePath {
        pattern: format!("^{START}$"),
        query: Some(InQuery {
            parameter: "url".to_owned(),
            pattern: pattern.to_owned(),
        }),
        space: None,
        item: item.to_owned(),
        exempt: Vec::new(),
        naming: Vec::new(),
    });
    Rules::of(fixtures::guarded(declared)).unwrap_or_else(|why| unreachable!("{why:?}"))
}

fn metadata() -> Rules {
    transcoding("/library/metadata/(?P<item>[0-9]+)", "^[0-9]+$")
}

#[test]
fn an_item_in_a_query_value_is_asked_about_as_it_reads_decoded_once() {
    for query in [
        "url=%2Flibrary%2Fmetadata%2F123&protocol=hls",
        "protocol=hls&url=/library/metadata/123",
    ] {
        let raw = format!("{START}?{query}");
        assert_eq!(
            check_of(metadata().decide("GET", &raw, &[], &[])),
            question(&["/Items/123"]),
            "{raw}"
        );
    }
}

#[test]
fn a_query_value_carried_twice_respelt_or_not_at_all_is_refused() {
    for query in [
        "url=/library/metadata/1&url=/library/metadata/1",
        "URL=/library/metadata/1",
        "Url=/library/metadata/1&url=/library/metadata/1",
        "protocol=hls",
        "url",
        "",
    ] {
        let raw = format!("{START}?{query}");
        assert_eq!(
            metadata().decide("GET", &raw, &[], &[]),
            Decision::Refuse(REFUSED),
            "{raw}"
        );
    }
}

#[test]
fn a_query_value_the_pattern_does_not_match_whole_is_refused() {
    for value in [
        "/library/metadata/1/children",
        "x/library/metadata/1",
        "/library/metadata/",
        "/library/metadata/1a",
    ] {
        let raw = format!("{START}?url={value}");
        assert_eq!(
            metadata().decide("GET", &raw, &[], &[]),
            Decision::Refuse(REFUSED),
            "{raw}"
        );
    }
}

#[test]
fn a_query_value_still_holding_a_percent_once_decoded_or_not_text_is_refused() {
    for value in [
        "%252Flibrary%252Fmetadata%252F1",
        "/library/metadata/1%",
        "/library/metadata/%FF",
    ] {
        let rules = transcoding("/library/metadata/(?P<item>.+)", "^.+$");
        let raw = format!("{START}?url={value}");
        assert_eq!(
            rules.decide("GET", &raw, &[], &[]),
            Decision::Refuse(REFUSED),
            "{raw}"
        );
    }
}

#[test]
fn an_item_from_a_query_value_holding_anything_but_unreserved_bytes_is_refused() {
    let rules = transcoding("/library/metadata/(?P<item>.+)", "^.+$");
    for value in [
        "/library/metadata/1%2F..%2Fx",
        "/library/metadata/1%3Fa",
        "/library/metadata/a%20b",
    ] {
        let raw = format!("{START}?url={value}");
        assert_eq!(
            rules.decide("GET", &raw, &[], &[]),
            Decision::Refuse(REFUSED),
            "{raw}"
        );
    }
    assert_eq!(
        check_of(rules.decide(
            "GET",
            &format!("{START}?url=/library/metadata/a-b_c.d~e"),
            &[],
            &[]
        )),
        question(&["/Items/a-b_c.d~e"])
    );
}

#[test]
fn an_item_from_a_query_value_out_of_its_shape_is_refused() {
    let rules = transcoding("/library/metadata/(?P<item>[^/]+)", "^[0-9]+$");
    assert_eq!(
        rules.decide(
            "GET",
            &format!("{START}?url=/library/metadata/abc"),
            &[],
            &[]
        ),
        Decision::Refuse(REFUSED)
    );
}

#[test]
fn a_query_pattern_that_does_not_compile_is_named() {
    let mut declared = fixtures::declared();
    declared.bytes.push(BytePath {
        pattern: format!("^{START}$"),
        query: Some(InQuery {
            parameter: "url".to_owned(),
            pattern: "(?P<item>".to_owned(),
        }),
        space: None,
        item: "^.+$".to_owned(),
        exempt: Vec::new(),
        naming: Vec::new(),
    });
    assert_eq!(
        Rules::of(fixtures::guarded(declared)).map(|_| ()),
        Err(super::Uncompiled("(?P<item>".to_owned()))
    );
}

#[test]
fn a_percent_left_beside_a_plain_item_once_decoded_is_refused() {
    let rules = transcoding(".*/library/metadata/(?P<item>[0-9]+)", "^[0-9]+$");
    assert_eq!(
        check_of(rules.decide(
            "GET",
            &format!("{START}?url=/x/library/metadata/1"),
            &[],
            &[]
        )),
        question(&["/Items/1"])
    );
    assert_eq!(
        rules.decide(
            "GET",
            &format!("{START}?url=%2525/library/metadata/1"),
            &[],
            &[]
        ),
        Decision::Refuse(REFUSED)
    );
}
