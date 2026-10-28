use std::os::unix::fs::PermissionsExt as _;
use std::sync::Arc;

use lemonfiber_ports::service::{Failure, Guarded, Guarding, Held, Naming};

use super::{Holding, ENVELOPE};
use crate::fixtures::{self, Scratch};
use crate::tls::Certificates;

fn holding(directory: &Scratch) -> Holding {
    Holding::in_directory(
        directory.path(),
        Arc::new(Certificates::in_directory(directory.path())),
    )
}

fn refusal(result: Result<(), Failure>) -> String {
    match result {
        Err(Failure::Refused { detail, .. }) => detail,
        other => unreachable!("refused, not {other:?}"),
    }
}

#[tokio::test]
async fn a_door_holding_nothing_guards_nothing() {
    let directory = Scratch::new();
    let held = holding(&directory);
    assert!(held.rules().is_none());
    assert_eq!(held.guarding().await.ok(), Some(None));
}

#[tokio::test]
async fn an_envelope_applied_is_enforced_kept_owner_only_and_read_back_whole() {
    let directory = Scratch::new();
    let held = holding(&directory);
    let guarded = fixtures::guarded(fixtures::declared());
    assert!(held.guard(&guarded).await.is_ok());
    assert_eq!(held.guarding().await.ok(), Some(Some(guarded.clone())));
    assert!(held.rules().is_some());
    let mode = std::fs::metadata(directory.path().join(ENVELOPE))
        .map(|meta| meta.permissions().mode() & 0o777)
        .ok();
    assert_eq!(mode, Some(0o600));
    assert_eq!(
        holding(&directory).guarding().await.ok(),
        Some(Some(guarded))
    );
}

#[tokio::test]
async fn a_copy_left_behind_is_made_afresh() {
    let directory = Scratch::new();
    let _ = std::fs::write(directory.path().join(format!("{ENVELOPE}.writing")), "old");
    let held = holding(&directory);
    assert!(held
        .guard(&fixtures::guarded(fixtures::declared()))
        .await
        .is_ok());
    let _ = std::fs::create_dir(directory.path().join(format!("{ENVELOPE}.writing")));
    let _ = std::fs::write(
        directory
            .path()
            .join(format!("{ENVELOPE}.writing"))
            .join("x"),
        "",
    );
    assert!(held
        .guard(&fixtures::guarded(fixtures::declared()))
        .await
        .is_err());
}

#[tokio::test]
async fn an_envelope_kept_that_cannot_be_read_guards_nothing() {
    for kept in ["not json", "{}"] {
        let directory = Scratch::new();
        let _ = std::fs::write(directory.path().join(ENVELOPE), kept);
        assert!(holding(&directory).rules().is_none(), "{kept}");
    }
}

#[tokio::test]
async fn a_declaration_no_guard_may_be_handed_is_refused_and_the_old_one_kept() {
    let directory = Scratch::new();
    let held = holding(&directory);
    let guarded = fixtures::guarded(fixtures::declared());
    assert!(held.guard(&guarded).await.is_ok());
    let mut unasked = fixtures::declared();
    unasked.question = None;
    assert!(refusal(held.guard(&fixtures::guarded(unasked)).await).contains("no question"));
    assert_eq!(held.guarding().await.ok(), Some(Some(guarded)));
}

#[tokio::test]
async fn an_address_the_door_cannot_reach_is_refused() {
    let directory = Scratch::new();
    let held = holding(&directory);
    let base = fixtures::guarded(fixtures::declared());
    let elsewhere: [fn(&mut Guarded); 9] = [
        |guarded| guarded.upstream = "https://jellyfin:8920".to_owned(),
        |guarded| guarded.upstream = "http://jellyfin:8096/web".to_owned(),
        |guarded| guarded.upstream = "http://user@jellyfin:8096".to_owned(),
        |guarded| guarded.upstream = "jellyfin:8096".to_owned(),
        |guarded| guarded.upstream = "http://jellyfin:8096?x=1".to_owned(),
        |guarded| guarded.adapter = Some("https://adapter:8080".to_owned()),
        |guarded| guarded.pass = "https://pass:8090/pass".to_owned(),
        |guarded| guarded.pass = "http://pass:8090/pass?x=1".to_owned(),
        |guarded| guarded.pass = "not an address".to_owned(),
    ];
    for (at, change) in elsewhere.iter().enumerate() {
        let mut guarded = base.clone();
        change(&mut guarded);
        assert!(
            refusal(held.guard(&guarded).await).contains("cannot reach"),
            "case {at}"
        );
    }
    let mut adapted = base;
    adapted.adapter = Some("http://adapter:8080".to_owned());
    assert!(held.guard(&adapted).await.is_ok());
}

#[tokio::test]
async fn a_shape_that_does_not_compile_is_refused() {
    let directory = Scratch::new();
    let held = holding(&directory);
    let mut declared = fixtures::declared();
    if let Some(path) = declared.bytes.first_mut() {
        path.naming.push(Naming {
            parameter: "x".to_owned(),
            held: Held::Asked {
                shape: "(unclosed".to_owned(),
            },
        });
    }
    assert!(held.guard(&fixtures::guarded(declared)).await.is_err());
}

#[tokio::test]
async fn an_envelope_that_cannot_be_written_is_refused_and_not_enforced() {
    let directory = Scratch::new();
    let held = Holding::in_directory(
        &directory.path().join("gone"),
        Arc::new(Certificates::in_directory(directory.path())),
    );
    assert!(
        refusal(held.guard(&fixtures::guarded(fixtures::declared())).await)
            .contains("could not be written")
    );
    assert!(held.rules().is_none());
}

#[tokio::test]
async fn the_certificate_presented_is_answered_by_its_digest() {
    let directory = Scratch::new();
    let held = holding(&directory);
    assert!(held.certificate().await.is_err());
    crate::tls::tests::written(directory.path());
    let presented = held.certificate().await.ok().map(|one| one.fingerprint);
    assert_eq!(
        presented.as_ref().map(String::len),
        Some(64),
        "{presented:?}"
    );
}
