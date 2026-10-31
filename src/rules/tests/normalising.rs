use lemonfiber_ports::service::{BytePath, Presentation};

use super::{check_of, Check, Decision, Rules, REFUSED};
use crate::fixtures::{self, ITEM, TOKEN};

fn rules_with(change: impl FnOnce(&mut lemonfiber_ports::service::Declared)) -> Rules {
    let mut declared = fixtures::declared();
    change(&mut declared);
    Rules::of(fixtures::guarded(declared)).unwrap_or_else(|why| unreachable!("{why:?}"))
}

fn asks(decision: &Decision) -> bool {
    matches!(decision, Decision::Forward(going) if matches!(going.check, Check::Question { .. }))
}

#[test]
fn a_token_holding_a_comma_or_a_space_is_refused_whatever_its_shape_allows() {
    let rules = rules_with(|declared| {
        declared.token.shape = "^.+$".to_owned();
        declared.token.presented = Presentation::Query {
            parameter: "X-Plex-Token".to_owned(),
        };
        declared.token.headers = vec!["X-Plex-Token".to_owned()];
    });
    for value in [
        format!("{TOKEN}, {TOKEN}x"),
        format!("{TOKEN},x"),
        format!("{TOKEN} x"),
    ] {
        assert_eq!(
            rules.decide("GET", "/Users", &[], &[("X-Plex-Token", value.as_bytes())]),
            Decision::Refuse(REFUSED),
            "{value}"
        );
    }
    let bearer = format!("Bearer {TOKEN} x");
    assert_eq!(
        rules.decide("GET", "/Users", &[bearer.as_bytes()], &[]),
        Decision::Forward(Box::new(super::Going {
            target: "/Users".to_owned(),
            presenting: super::Presenting::Unchanged,
            check: Check::Nothing,
        }))
    );
}

#[test]
fn a_query_another_reader_could_split_at_a_semicolon_is_refused_on_a_byte_path() {
    let rules = rules_with(|_| {});
    let raw = format!("/Videos/{ITEM}/stream?static=true;api_key={TOKEN}");
    assert_eq!(
        rules.decide("GET", &raw, &[], &[]),
        Decision::Refuse(REFUSED)
    );
    assert!(matches!(
        rules.decide("GET", "/web/index.html?a=1;b=2", &[], &[]),
        Decision::Forward(_)
    ));
}

#[test]
fn a_path_segment_ending_in_a_dot_or_a_space_is_refused() {
    let rules = rules_with(|_| {});
    for raw in [
        format!("/Items./{ITEM}/Download"),
        format!("/Items/{ITEM}./Download"),
        format!("/Items%20/{ITEM}/Download"),
        "/web/index.html.".to_owned(),
    ] {
        assert_eq!(
            rules.decide("GET", &raw, &[], &[]),
            Decision::Refuse(REFUSED),
            "{raw}"
        );
    }
}

#[test]
fn a_path_is_matched_without_regard_to_case_whatever_the_declaration_writes() {
    let rules = rules_with(|declared| {
        declared.bytes = vec![BytePath {
            pattern: "^/Items/(?P<item>[^/]+)/Download$".to_owned(),
            query: None,
            space: None,
            item: "^[0-9a-f]{32}$".to_owned(),
            exempt: Vec::new(),
            naming: Vec::new(),
        }];
        declared.refused = vec!["^/Branding/Splashscreen$".to_owned()];
    });
    assert!(asks(&rules.decide(
        "GET",
        &format!("/ITEMS/{ITEM}/download"),
        &[],
        &[]
    )));
    assert_eq!(
        rules.decide("GET", "/branding/splashscreen", &[], &[]),
        Decision::Refuse(REFUSED)
    );
    assert!(matches!(
        check_of(rules.decide("GET", &format!("/Items/{ITEM}/Download"), &[], &[])),
        Check::Question { .. }
    ));
}
