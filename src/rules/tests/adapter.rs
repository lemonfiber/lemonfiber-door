use lemonfiber_ports::http::Method;
use lemonfiber_ports::service::{Addressee, Answers, Guarded, Presentation, Question, MAY};

use super::{check_of, Carrying, Check, Decision, Rules, REFUSED};
use crate::fixtures::{self, ITEM, OTHER, TOKEN};

const ADAPTER: &str = "http://jellyfin-adapter:8080";

/// The space every byte path of these declarations names.
const SPACE: &str = "item";

fn asked_of(item: &str) -> String {
    MAY.replace("{space}", SPACE).replace("{item}", item)
}

fn adapter_guarded() -> Guarded {
    let mut declared = fixtures::declared();
    declared.question = Some(Question {
        method: Method::Get,
        path: MAY.to_owned(),
        to: Addressee::Adapter,
        answers: Answers::from_adapter(),
    });
    for path in &mut declared.bytes {
        path.space = Some(SPACE.to_owned());
    }
    let mut guarded = fixtures::guarded(declared);
    guarded.adapter = Some(ADAPTER.to_owned());
    guarded
}

fn presenting(presented: Presentation) -> Rules {
    let mut guarded = adapter_guarded();
    guarded.declared.token.presented = presented;
    Rules::of(guarded).unwrap_or_else(|why| unreachable!("{why:?}"))
}

fn jellyfin() -> Rules {
    Rules::of(adapter_guarded()).unwrap_or_else(|why| unreachable!("{why:?}"))
}

fn asking(asked: &str, carrying: Carrying) -> Check {
    Check::Question {
        to: ADAPTER.to_owned(),
        method: Method::Get,
        asked: vec![asked.to_owned()],
        answers: Answers::from_adapter(),
        carrying,
    }
}

fn header(name: &str, value: &str) -> Carrying {
    Carrying::Token(Some((name.to_owned(), value.to_owned())))
}

fn shown() -> Carrying {
    header("Authorization", &format!(r#"MediaBrowser Token="{TOKEN}""#))
}

fn stream() -> String {
    format!("/Videos/{ITEM}/stream")
}

#[test]
fn the_adapter_is_asked_with_the_member_token_alone_however_the_request_carried_it() {
    let bearer = format!("Bearer {TOKEN}");
    let shown_header = format!(r#"MediaBrowser Token="{TOKEN}""#);
    for (raw, authorization, presented) in [
        (
            stream(),
            vec![bearer.as_bytes()],
            vec![("Authorization", bearer.as_bytes())],
        ),
        (
            format!("{}?ApiKey={TOKEN}&static=true", stream()),
            vec![],
            vec![],
        ),
        (
            stream(),
            vec![shown_header.as_bytes()],
            vec![("Authorization", shown_header.as_bytes())],
        ),
    ] {
        assert_eq!(
            check_of(jellyfin().decide("GET", &raw, &authorization, &presented)),
            asking(&asked_of(ITEM), shown()),
            "{raw}"
        );
    }
}

#[test]
fn a_request_carrying_no_token_asks_the_adapter_carrying_none() {
    assert_eq!(
        check_of(jellyfin().decide("GET", &stream(), &[], &[])),
        asking(&asked_of(ITEM), Carrying::Token(None))
    );
}

#[test]
fn the_token_goes_to_the_adapter_as_the_declaration_presents_it() {
    let bearer = format!("Bearer {TOKEN}");
    assert_eq!(
        check_of(
            presenting(Presentation::Query {
                parameter: "api_key".to_owned()
            })
            .decide("GET", &stream(), &[bearer.as_bytes()], &[])
        ),
        asking(
            &format!("{}&api_key={TOKEN}", asked_of(ITEM)),
            Carrying::Token(None)
        )
    );
    assert_eq!(
        check_of(presenting(Presentation::AsIs).decide(
            "GET",
            &format!("{}?api_key={TOKEN}", stream()),
            &[],
            &[]
        )),
        asking(&asked_of(ITEM), header("authorization", &bearer))
    );
}

#[test]
fn a_token_in_its_own_header_beside_another_way_or_out_of_shape_is_refused() {
    let plex = || {
        presenting(Presentation::Header {
            name: "X-Plex-Token".to_owned(),
            value: "{token}".to_owned(),
        })
    };
    let bearer = format!("Bearer {OTHER}");
    assert_eq!(
        check_of(plex().decide("GET", &stream(), &[], &[("X-Plex-Token", TOKEN.as_bytes())])),
        asking(&asked_of(ITEM), header("X-Plex-Token", TOKEN))
    );
    for (raw, authorization, presented) in [
        (
            stream(),
            vec![bearer.as_bytes()],
            vec![("X-Plex-Token", TOKEN.as_bytes())],
        ),
        (
            format!("{}?ApiKey={OTHER}", stream()),
            vec![],
            vec![("X-Plex-Token", TOKEN.as_bytes())],
        ),
        (
            stream(),
            vec![],
            vec![
                ("X-Plex-Token", TOKEN.as_bytes()),
                ("X-Plex-Token", TOKEN.as_bytes()),
            ],
        ),
        (
            stream(),
            vec![],
            vec![("X-Plex-Token", b"not-a-token".as_slice())],
        ),
        (stream(), vec![], vec![("X-Plex-Token", b"\xff".as_slice())]),
    ] {
        assert_eq!(
            plex().decide("GET", &raw, &authorization, &presented),
            Decision::Refuse(REFUSED),
            "{raw}"
        );
    }
}

#[test]
fn an_authorization_header_out_of_its_declared_presentation_is_refused_for_the_adapter() {
    let other = "MediaBrowser Client=\"x\"";
    assert_eq!(
        jellyfin().decide(
            "GET",
            &stream(),
            &[other.as_bytes()],
            &[("Authorization", other.as_bytes())]
        ),
        Decision::Refuse(REFUSED)
    );
}

#[test]
fn an_item_holding_anything_but_unreserved_bytes_is_put_in_no_question() {
    let mut guarded = adapter_guarded();
    for path in &mut guarded.declared.bytes {
        path.item = "^.+$".to_owned();
    }
    let upstream = {
        let mut upstream = guarded.clone();
        if let Some(question) = upstream.declared.question.as_mut() {
            question.to = Addressee::Upstream;
            question.path = "/Items/{item}".to_owned();
        }
        Rules::of(upstream).unwrap_or_else(|why| unreachable!("{why:?}"))
    };
    let rules = Rules::of(guarded).unwrap_or_else(|why| unreachable!("{why:?}"));
    for item in ["a&item=b", "a=b", "a+b", "a!b"] {
        let raw = format!("/Videos/{item}/stream");
        for asking in [&rules, &upstream] {
            assert_eq!(
                asking.decide("GET", &raw, &[], &[]),
                Decision::Refuse(REFUSED),
                "{raw}"
            );
        }
    }
}

#[test]
fn a_byte_path_naming_no_space_is_never_asked_about_at_the_adapter() {
    let mut guarded = adapter_guarded();
    for path in &mut guarded.declared.bytes {
        path.space = None;
    }
    let rules = Rules::of(guarded).unwrap_or_else(|why| unreachable!("{why:?}"));
    assert_eq!(
        rules.decide("GET", &stream(), &[], &[]),
        Decision::Refuse(REFUSED)
    );
}

#[test]
fn a_space_spelled_as_none_is_is_never_put_in_the_adapter_question() {
    for space in ["a&item=b", "Item", "", "a/b"] {
        let mut guarded = adapter_guarded();
        for path in &mut guarded.declared.bytes {
            path.space = Some(space.to_owned());
        }
        let rules = Rules::of(guarded).unwrap_or_else(|why| unreachable!("{why:?}"));
        assert_eq!(
            rules.decide("GET", &stream(), &[], &[]),
            Decision::Refuse(REFUSED),
            "{space}"
        );
    }
}
