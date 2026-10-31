use lemonfiber_ports::service::{Held, Naming, Presentation};

use super::{check_of, Check, Decision, Rules, REFUSED};
use crate::fixtures::{self, ITEM, TOKEN};

fn stream() -> String {
    format!("/Videos/{ITEM}/stream")
}

fn rules_with(change: impl FnOnce(&mut lemonfiber_ports::service::Declared)) -> Rules {
    let mut declared = fixtures::declared();
    change(&mut declared);
    Rules::of(fixtures::guarded(declared)).unwrap_or_else(|why| unreachable!("{why:?}"))
}

fn asked_of(decision: Decision) -> Vec<String> {
    match check_of(decision) {
        Check::Question { asked, .. } => asked,
        other => unreachable!("asks nothing: {other:?}"),
    }
}

#[test]
fn a_value_naming_another_item_is_held_to_its_whole_shape_whatever_the_declaration_anchors() {
    let rules = rules_with(|declared| {
        for path in &mut declared.bytes {
            path.naming = vec![Naming {
                parameter: "MediaSourceId".to_owned(),
                held: Held::Asked {
                    shape: "[0-9a-f]{32}".to_owned(),
                },
            }];
        }
    });
    for value in [
        format!("{ITEM}/../../System/Info/Public"),
        format!("{ITEM}zz"),
    ] {
        let raw = format!("{}?MediaSourceId={value}", stream());
        assert_eq!(
            rules.decide("GET", &raw, &[], &[]),
            Decision::Refuse(REFUSED),
            "{raw}"
        );
    }
}

#[test]
fn an_item_is_held_to_its_whole_shape_and_to_plain_bytes_whatever_the_question() {
    let unanchored = rules_with(|declared| {
        for path in &mut declared.bytes {
            path.item = "[0-9a-f]{32}".to_owned();
        }
    });
    assert_eq!(
        unanchored.decide("GET", &format!("/Videos/{ITEM}x/stream"), &[], &[]),
        Decision::Refuse(REFUSED)
    );
    let loose = rules_with(|declared| {
        for path in &mut declared.bytes {
            path.item = "^.+$".to_owned();
        }
    });
    for item in ["a&b", "a=b", "a+b", "a!b", "a@b", "a:b"] {
        let raw = format!("/Videos/{item}/stream");
        assert_eq!(
            loose.decide("GET", &raw, &[], &[]),
            Decision::Refuse(REFUSED),
            "{raw}"
        );
    }
}

#[test]
fn a_token_is_held_to_its_whole_shape_whatever_the_declaration_anchors() {
    let rules = rules_with(|declared| {
        declared.token.shape = "[0-9a-f]{32}".to_owned();
        declared.token.presented = Presentation::Query {
            parameter: "api_key".to_owned(),
        };
    });
    assert_eq!(
        rules.decide("GET", &format!("{}?api_key={TOKEN}x", stream()), &[], &[]),
        Decision::Refuse(REFUSED)
    );
    let unshaped = format!("Bearer {TOKEN}&item=7");
    let decision = rules.decide("GET", &stream(), &[unshaped.as_bytes()], &[]);
    let Decision::Forward(going) = decision.clone() else {
        unreachable!("refused: {decision:?}");
    };
    assert_eq!(going.target, stream());
    assert_eq!(asked_of(decision), vec![format!("/Items/{ITEM}")]);
}

#[test]
fn a_token_put_in_a_query_is_percent_encoded_so_it_adds_no_parameter() {
    let rules = rules_with(|declared| {
        declared.token.shape = "^[A-Za-z0-9+/=&]{8,64}$".to_owned();
        declared.token.presented = Presentation::Query {
            parameter: "api_key".to_owned(),
        };
    });
    let bearer = "Bearer abc+de/f=g&item=7";
    let decision = rules.decide("GET", &stream(), &[bearer.as_bytes()], &[]);
    let Decision::Forward(going) = decision.clone() else {
        unreachable!("refused: {decision:?}");
    };
    let encoded = "api_key=abc%2Bde%2Ff%3Dg%26item%3D7";
    assert_eq!(going.target, format!("{}?{encoded}", stream()));
    assert_eq!(asked_of(decision), vec![format!("/Items/{ITEM}?{encoded}")]);
}

#[test]
fn a_path_a_byte_path_exempts_is_matched_from_the_paths_start_whatever_the_declaration_anchors() {
    let rules = rules_with(|declared| {
        for path in &mut declared.bytes {
            path.exempt = vec!["Videos/ActiveEncodings".to_owned()];
        }
    });
    let raw = format!("/Videos/{ITEM}/Videos/ActiveEncodings");
    assert!(matches!(
        check_of(rules.decide("GET", &raw, &[], &[])),
        Check::Question { .. }
    ));
}
