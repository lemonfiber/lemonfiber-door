use std::net::SocketAddr;
use std::time::Duration;

use super::{run, Ports};
use crate::fake;
use crate::fixtures::Scratch;

async fn free() -> SocketAddr {
    fake::nobody().await
}

#[tokio::test]
async fn the_door_serves_until_stopped() {
    let scratch = Scratch::new();
    let ports = Ports {
        plain: free().await,
        encrypted: free().await,
        contract: free().await,
    };
    let ended = tokio::time::timeout(
        Duration::from_secs(5),
        run(
            scratch.path(),
            ports,
            tokio::time::sleep(Duration::from_millis(100)),
        ),
    )
    .await;
    assert!(matches!(ended, Ok(Ok(()))));
}

#[tokio::test]
async fn a_port_that_cannot_be_listened_on_ends_the_door_with_why() {
    let scratch = Scratch::new();
    let taken = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|why| unreachable!("{why}"));
    let at = taken
        .local_addr()
        .unwrap_or_else(|why| unreachable!("{why}"));
    for ports in [
        Ports {
            plain: at,
            encrypted: free().await,
            contract: free().await,
        },
        Ports {
            plain: free().await,
            encrypted: at,
            contract: free().await,
        },
    ] {
        assert!(run(scratch.path(), ports, std::future::pending())
            .await
            .is_err());
    }
    let _ = std::fs::write(
        scratch.path().join(lemonfiber_contract::adapter::KEY_FILE),
        "key",
    );
    let ports = Ports {
        plain: free().await,
        encrypted: free().await,
        contract: at,
    };
    assert!(run(scratch.path(), ports, std::future::pending())
        .await
        .is_err());
}
