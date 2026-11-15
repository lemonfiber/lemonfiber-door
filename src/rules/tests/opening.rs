use lemonfiber_ports::service::Presentation;

use super::{
    check_of, decided, forward, jellyfin, question, Check, Decision, Opening, Presenting, Rules,
    REFUSED,
};
use crate::fixtures::{self, ITEM, OTHER, PASS, TOKEN};

fn opening(target: &str, pass: &str) -> Opening {
    Opening {
        at: PASS.to_owned(),
        pass: pass.to_owned(),
        target: target.to_owned(),
    }
}

#[test]
fn a_byte_path_carrying_a_pass_opens_once_the_pass_service_hands_its_session() {
    assert_eq!(
        decided(&format!("/Videos/{ITEM}/stream?static=true&lfpass=n.1.s")),
        Decision::Open(Box::new(opening(
            &format!("/Videos/{ITEM}/stream?static=true"),
            "n.1.s"
        )))
    );
}

#[test]
fn a_pass_beside_a_member_token_twice_respelt_or_on_an_item_out_of_shape_is_refused() {
    let rules = jellyfin();
    let target = format!("/Videos/{ITEM}/stream?lfpass=p");
    let bearer = format!("Bearer {TOKEN}");
    let client = r#"MediaBrowser Client="x", Device="y""#;
    for authorization in [bearer.as_str(), client, "Basic eA=="] {
        assert_eq!(
            rules.decide("GET", &target, &[authorization.as_bytes()], &[]),
            Decision::Refuse(REFUSED),
            "{authorization}"
        );
    }
    assert_eq!(
        rules.decide("GET", &target, &[], &[("Authorization", client.as_bytes())]),
        Decision::Refuse(REFUSED)
    );
    for raw in [
        format!("/Videos/{ITEM}/stream?lfpass=p&ApiKey={TOKEN}"),
        format!("/Videos/{ITEM}/stream?lfpass=p&lfpass=q"),
        format!("/Videos/{ITEM}/stream?LFPASS=p"),
        format!("/Videos/{ITEM}/stream?lfpass="),
        format!("/Videos/{ITEM}/stream?lfpass=p%2E"),
        format!("/Videos/{ITEM}/stream?lfpass=p&itemId={OTHER}"),
        "/Videos/notanitem/stream?lfpass=p".to_owned(),
    ] {
        assert_eq!(decided(&raw), Decision::Refuse(REFUSED), "{raw}");
    }
    assert_eq!(
        rules.decide("POST", &target, &[], &[]),
        Decision::Refuse(REFUSED)
    );
}

#[test]
fn a_pass_is_taken_off_every_request_that_goes_on() {
    assert_eq!(
        decided("/web/index.html?a=1&lfpass=p&LFPASS=q&b"),
        forward("/web/index.html?a=1&b", Check::Nothing)
    );
    assert_eq!(
        decided("/Videos/ActiveEncodings?lfpass=p"),
        forward("/Videos/ActiveEncodings", Check::Nothing)
    );
}

#[test]
fn the_session_a_pass_opens_is_asked_with_and_presented_as_a_clients_token() {
    let target = format!("/Videos/{ITEM}/stream?static=true");
    let opened = jellyfin().opened("GET", &opening(&target, "n.1.s"), TOKEN);
    let Decision::Forward(going) = opened else {
        unreachable!("forwarded: {opened:?}");
    };
    assert_eq!(going.target, target);
    assert_eq!(going.check, question(&[&format!("/Items/{ITEM}")]));
    assert_eq!(
        going.presenting,
        Presenting::Header {
            name: "Authorization".to_owned(),
            value: format!(r#"MediaBrowser Token="{TOKEN}""#),
        }
    );
    assert_eq!(
        going.writing,
        Some(super::writing(&[("lfpass", "n.1.s")], Some(TOKEN)))
    );
    let as_is = Rules::of(fixtures::presenting(Presentation::AsIs))
        .unwrap_or_else(|why| unreachable!("{why:?}"));
    let Decision::Forward(going) = as_is.opened("GET", &opening(&target, "p"), TOKEN) else {
        unreachable!("forwarded");
    };
    assert_eq!(
        going.presenting,
        Presenting::Header {
            name: "authorization".to_owned(),
            value: format!("Bearer {TOKEN}"),
        }
    );
}

#[test]
fn a_session_is_never_presented_in_the_query_where_the_upstream_could_write_it_back() {
    let target = format!("/Videos/{ITEM}/stream?static=true");
    let mut in_query = fixtures::presenting(Presentation::Query {
        parameter: "api_key".to_owned(),
    });
    let headerless = Rules::of(in_query.clone()).unwrap_or_else(|why| unreachable!("{why:?}"));
    assert_eq!(
        headerless.opened("GET", &opening(&target, "p"), TOKEN),
        Decision::Refuse(REFUSED)
    );
    in_query.declared.token.headers = vec!["X-Emby-Token".to_owned()];
    let in_header = Rules::of(in_query).unwrap_or_else(|why| unreachable!("{why:?}"));
    let Decision::Forward(going) = in_header.opened("GET", &opening(&target, "p"), TOKEN) else {
        unreachable!("forwarded");
    };
    assert_eq!(going.target, target);
    assert_eq!(going.check, question(&[&format!("/Items/{ITEM}")]));
    assert_eq!(
        going.presenting,
        Presenting::Header {
            name: "X-Emby-Token".to_owned(),
            value: TOKEN.to_owned(),
        }
    );
}

#[test]
fn a_query_parameter_name_spelt_otherwise_on_an_item_pass_is_refused() {
    for name in ["Live%53treamId", "lf%70ass", "Live+StreamId", "Api%4Bey"] {
        let raw = format!("/Videos/{ITEM}/stream?lfpass=p&{name}=1");
        assert_eq!(decided(&raw), Decision::Refuse(REFUSED), "{name}");
    }
}

#[test]
fn a_session_of_another_shape_or_off_the_byte_paths_is_refused() {
    let target = format!("/Videos/{ITEM}/stream");
    for session in ["not a token", "", "0123", &format!("{TOKEN},{TOKEN}")] {
        assert_eq!(
            jellyfin().opened("GET", &opening(&target, "p"), session),
            Decision::Refuse(REFUSED),
            "{session}"
        );
    }
    for target in ["/web/index.html", "/Videos/ActiveEncodings"] {
        assert_eq!(
            jellyfin().opened("GET", &opening(target, "p"), TOKEN),
            Decision::Refuse(REFUSED),
            "{target}"
        );
    }
    assert!(matches!(check_of(decided(&target)), Check::Question { .. }));
}
