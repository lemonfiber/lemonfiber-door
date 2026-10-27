//! Accepting the household's connections, plain or encrypted, each served by the door
//! over HTTP/1.1 with its upgrades or over HTTP/2, until the door is stopped.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use hyper::service::service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncWrite};

use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

use crate::serving::{Arrival, Door};

/// Serve every connection `listener` accepts with `door`, through `tls` where it is
/// given, until `stopped` resolves.
pub(crate) async fn serve(
    listener: TcpListener,
    door: Arc<Door>,
    tls: Option<TlsAcceptor>,
    mut stopped: Pin<Box<dyn Future<Output = ()> + Send>>,
) {
    loop {
        let accepted = tokio::select! {
            () = &mut stopped => return,
            accepted = listener.accept() => accepted,
        };
        taken(accepted, &door, tls.as_ref());
    }
}

/// Serve the connection `accepted` holds with `door`, through `tls` where it is given;
/// a connection that could not be accepted is left.
fn taken(
    accepted: std::io::Result<(TcpStream, SocketAddr)>,
    door: &Arc<Door>,
    tls: Option<&TlsAcceptor>,
) {
    let Ok((stream, peer)) = accepted else {
        return;
    };
    let _ = stream.set_nodelay(true);
    let door = Arc::clone(door);
    let tls = tls.cloned();
    tokio::spawn(async move {
        let arrival = Arrival {
            peer,
            encrypted: tls.is_some(),
        };
        match tls {
            Some(acceptor) => {
                if let Ok(encrypted) = acceptor.accept(stream).await {
                    connection(encrypted, door, arrival).await;
                }
            }
            None => connection(stream, door, arrival).await,
        }
    });
}

/// Serve one connection with `door` until it closes.
async fn connection<I>(io: I, door: Arc<Door>, arrival: Arrival)
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |request| {
        let door = Arc::clone(&door);
        async move { Ok::<_, std::convert::Infallible>(door.answer(request, arrival).await) }
    });
    let _ = Builder::new(TokioExecutor::new())
        .serve_connection_with_upgrades(TokioIo::new(io), service)
        .await;
}

#[cfg(test)]
mod tests;
