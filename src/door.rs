//! The door's process, as one call: the envelope it keeps and the certificate it reads, its plain
//! and encrypted household ports and its contract port, until it is stopped.

use std::future::Future;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use crate::holding::Holding;
use crate::listening;
use crate::serving::Door;
use crate::speaking;
use crate::tls::{self, Certificates};

/// Where the door listens.
#[derive(Debug, Clone, Copy)]
pub struct Ports {
    /// The household's plain port.
    pub plain: SocketAddr,
    /// The household's encrypted port.
    pub encrypted: SocketAddr,
    /// The contract's port.
    pub contract: SocketAddr,
}

/// Serve the door from what `config` holds, read-only, and the envelope kept in `state`,
/// on `ports` until `stopped` resolves, or until
/// a port cannot be served, with why.
///
/// # Errors
///
/// Where a port cannot be listened on, or the contract cannot be served.
pub async fn run(
    config: &Path,
    state: &Path,
    ports: Ports,
    stopped: impl Future<Output = ()> + Send,
) -> std::io::Result<()> {
    let certificates = Arc::new(Certificates::in_directory(config));
    let holding = Arc::new(Holding::in_directory(state, Arc::clone(&certificates)));
    let door = Arc::new(Door::new(Arc::clone(&holding)));
    let served = tls::served(certificates).map_err(std::io::Error::other)?;
    let plain = TcpListener::bind(ports.plain).await?;
    let encrypted = TcpListener::bind(ports.encrypted).await?;
    let listening = [
        tokio::spawn(listening::serve(
            plain,
            Arc::clone(&door),
            None,
            Box::pin(std::future::pending()),
        )),
        tokio::spawn(listening::serve(
            encrypted,
            door,
            Some(TlsAcceptor::from(served)),
            Box::pin(std::future::pending()),
        )),
    ];
    let ended = tokio::select! {
        () = stopped => Ok(()),
        contract = speaking::serve(ports.contract, config, holding) => contract,
    };
    for listener in listening {
        listener.abort();
    }
    ended
}

#[cfg(test)]
mod tests;
