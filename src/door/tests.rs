use std::net::SocketAddr;
use std::time::Duration;

use super::{run, Ports};
use crate::fake;
use crate::fixtures::{minted, Scratch};

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
        assert!(run(
            scratch.path(),
            scratch.path(),
            ports,
            std::future::pending()
        )
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
    assert!(run(
        scratch.path(),
        scratch.path(),
        ports,
        std::future::pending()
    )
    .await
    .is_err());
}

#[tokio::test]
async fn an_envelope_applied_over_the_contract_is_kept_under_state_and_nothing_under_config() {
    use std::os::unix::fs::PermissionsExt as _;

    let config = Scratch::new();
    let state = Scratch::new();
    let key = minted();
    let _ = std::fs::write(
        config.path().join(lemonfiber_contract::adapter::KEY_FILE),
        &key,
    );
    crate::tls::tests::written(config.path());
    let before = listed(config.path());
    let _ = std::fs::set_permissions(config.path(), std::fs::Permissions::from_mode(0o555));
    let ports = Ports {
        plain: free().await,
        encrypted: free().await,
        contract: free().await,
    };
    let (config_path, state_path) = (config.path().to_path_buf(), state.path().to_path_buf());
    let door =
        tokio::spawn(
            async move { run(&config_path, &state_path, ports, std::future::pending()).await },
        );
    let mut applied = false;
    for _ in 0..100 {
        if applied_over(ports.contract, &key).await {
            applied = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    door.abort();
    let _ = std::fs::set_permissions(config.path(), std::fs::Permissions::from_mode(0o755));
    assert!(applied);
    assert_eq!(listed(config.path()), before);
    assert_eq!(
        listed(state.path()),
        vec![crate::holding::ENVELOPE.to_owned()]
    );
}

/// The names of the files in `directory`, sorted.
fn listed(directory: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Whether the door at `contract` took the tests' envelope under `key`.
async fn applied_over(contract: SocketAddr, key: &str) -> bool {
    let body = serde_json::to_vec(&serde_json::json!({
        "guarded": crate::fixtures::guarded(crate::fixtures::declared()),
    }))
    .unwrap_or_default();
    let request = http::Request::builder()
        .method("POST")
        .uri(format!(
            "http://{contract}{}",
            lemonfiber_contract::path("stream.guard", 1, "guard")
        ))
        .header("authorization", format!("Bearer {key}"))
        .header("content-type", "application/json")
        .body(http_body_util::Full::new(bytes::Bytes::from(body)));
    let Ok(request) = request else {
        return false;
    };
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();
    client
        .request(request)
        .await
        .is_ok_and(|answer| answer.status().is_success())
}
