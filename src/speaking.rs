//! The contract the door speaks to the core, `stream.guard@1`, on a port of its own,
//! for itself.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use lemonfiber_adapter_kit::Kit;
use lemonfiber_contract::adapter::About;
use lemonfiber_contract::capabilities::stream::guard;

use crate::holding::Holding;

/// What the door calls itself as the upstream it answers for.
pub(crate) const UPSTREAM: &str = "lemonfiber door";

/// What the door says it is: the one contract it speaks, for itself.
pub(crate) fn about() -> About {
    About {
        speaks: vec![lemonfiber_contract::spoken(guard::CAPABILITY, guard::MAJOR)],
        upstream: UPSTREAM.to_owned(),
        releases: Vec::new(),
    }
}

/// `stream.guard`, served under `key` from `holding`.
pub(crate) fn kit(key: String, holding: Arc<Holding>) -> Kit {
    Kit::new(key, about()).serving(guard::served(holding))
}

/// Answer the core on `at` under the plugin's key in `config`, for as long as the door
/// runs: it ends only where `at` cannot be served, with why.
///
/// Where the core has written no key, nothing answers there: a contract asked with no key
/// to check it against would answer anybody.
pub(crate) async fn serve(
    at: SocketAddr,
    config: &Path,
    holding: Arc<Holding>,
) -> std::io::Result<()> {
    let Some(key) = lemonfiber_adapter_kit::key_in(config)
        .ok()
        .filter(|key| !key.is_empty())
    else {
        eprintln!("door: the core has written no key, so stream.guard is not answered");
        return std::future::pending().await;
    };
    let listener = tokio::net::TcpListener::bind(at).await?;
    lemonfiber_adapter_kit::serving(kit(key, holding), listener, std::future::pending()).await
}

#[cfg(test)]
mod tests;
