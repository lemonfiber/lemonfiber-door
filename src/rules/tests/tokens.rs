use lemonfiber_ports::service::Presentation;

use super::{check_of, Check, Decision, Rules, REFUSED};
use crate::fixtures::{self, ITEM, OTHER, TOKEN};

fn jellyfin() -> Rules {
    Rules::of(fixtures::guarded(fixtures::declared())).unwrap_or_else(|why| unreachable!("{why:?}"))
}

fn plex() -> Rules {
    let mut declared = fixtures::declared();
    declared.token.presented = Presentation::Query {
        parameter: "X-Plex-Token".to_owned(),
    };
    declared.token.parameters = Vec::new();
    declared.token.headers = vec!["X-Plex-Token".to_owned()];
    Rules::of(fixtures::guarded(declared)).unwrap_or_else(|why| unreachable!("{why:?}"))
}

fn stream() -> String {
    format!("/Videos/{ITEM}/stream")
}

fn goes(decision: &Decision) -> bool {
    matches!(decision, Decision::Forward(_))
}

#[test]
fn the_same_token_in_several_declared_places_is_one_token() {
    let bearer = format!("Bearer {TOKEN}");
    assert!(goes(&jellyfin().decide(
        "GET",
        &format!("/Users?ApiKey={TOKEN}&api_key={TOKEN}"),
        &[],
        &[]
    )));
    assert!(goes(&jellyfin().decide(
        "GET",
        &format!("/Users?ApiKey={TOKEN}"),
        &[bearer.as_bytes()],
        &[]
    )));
    assert!(goes(&plex().decide(
        "GET",
        &format!("/Users?X-Plex-Token={TOKEN}"),
        &[],
        &[("X-Plex-Token", TOKEN.as_bytes())]
    )));
}

#[test]
fn different_tokens_in_two_declared_places_are_refused() {
    let bearer = format!("Bearer {TOKEN}");
    for (raw, authorization) in [
        (format!("/Users?ApiKey={TOKEN}&api_key={OTHER}"), vec![]),
        (format!("/Users?ApiKey={OTHER}"), vec![bearer.as_bytes()]),
    ] {
        assert_eq!(
            jellyfin().decide("GET", &raw, &authorization, &[]),
            Decision::Refuse(REFUSED),
            "{raw}"
        );
    }
    assert_eq!(
        plex().decide(
            "GET",
            &format!("/Users?X-Plex-Token={OTHER}"),
            &[],
            &[("X-Plex-Token", TOKEN.as_bytes())]
        ),
        Decision::Refuse(REFUSED)
    );
}

#[test]
fn a_token_header_the_declaration_names_is_read_once_and_in_its_shape() {
    assert!(goes(&plex().decide(
        "GET",
        "/Users",
        &[],
        &[("x-plex-token", TOKEN.as_bytes())]
    )));
    for headers in [
        vec![
            ("X-Plex-Token", TOKEN.as_bytes()),
            ("X-Plex-Token", TOKEN.as_bytes()),
        ],
        vec![("X-Plex-Token", b"not-a-token".as_slice())],
        vec![("X-Plex-Token", b"\xff".as_slice())],
    ] {
        assert_eq!(
            plex().decide("GET", "/Users", &[], &headers),
            Decision::Refuse(REFUSED),
            "{headers:?}"
        );
    }
}

#[test]
fn a_client_header_the_template_does_not_hold_asks_as_itself_on_an_upstream_question() {
    let client = format!(r#"MediaBrowser Client="x", Device="y", Token="{TOKEN}""#);
    assert!(matches!(
        check_of(jellyfin().decide(
            "GET",
            &stream(),
            &[client.as_bytes()],
            &[("Authorization", client.as_bytes())]
        )),
        Check::Question { .. }
    ));
}
