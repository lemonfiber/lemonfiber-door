use std::sync::Arc;
use std::time::Duration;

use super::{about, serve, UPSTREAM};
use crate::fixtures::Scratch;
use crate::holding::Holding;
use crate::tls::Certificates;

fn holding(scratch: &Scratch) -> Arc<Holding> {
    Arc::new(Holding::in_directory(
        scratch.path(),
        Arc::new(Certificates::in_directory(scratch.path())),
    ))
}

#[test]
fn the_door_speaks_stream_guard_for_itself() {
    let said = about();
    assert_eq!(said.speaks, vec!["stream.guard@1".to_owned()]);
    assert_eq!(said.upstream, UPSTREAM);
    assert!(said.releases.is_empty());
}

#[tokio::test]
async fn nothing_answers_the_contract_without_a_key() {
    let scratch = Scratch::new();
    let at = crate::fake::nobody().await;
    let answered = tokio::time::timeout(
        Duration::from_millis(200),
        serve(at, scratch.path(), holding(&scratch)),
    )
    .await;
    assert!(answered.is_err());
    assert!(tokio::net::TcpStream::connect(at).await.is_err());
}

#[tokio::test]
async fn the_contract_is_answered_under_the_key_the_core_wrote() {
    let scratch = Scratch::new();
    let _ = std::fs::write(
        scratch.path().join(lemonfiber_contract::adapter::KEY_FILE),
        "0123456789abcdef0123456789abcdef",
    );
    let at = crate::fake::nobody().await;
    let held = holding(&scratch);
    tokio::spawn(async move {
        let _ = serve(at, scratch.path(), held).await;
        drop(scratch);
    });
    let mut connected = false;
    for _ in 0..50 {
        if tokio::net::TcpStream::connect(at).await.is_ok() {
            connected = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(connected);
}

#[tokio::test]
async fn a_contract_port_that_cannot_be_listened_on_ends_serving_with_why() {
    let scratch = Scratch::new();
    let _ = std::fs::write(
        scratch.path().join(lemonfiber_contract::adapter::KEY_FILE),
        "key",
    );
    let taken = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|why| unreachable!("{why}"));
    let at = taken
        .local_addr()
        .unwrap_or_else(|why| unreachable!("{why}"));
    assert!(serve(at, scratch.path(), holding(&scratch)).await.is_err());
}
