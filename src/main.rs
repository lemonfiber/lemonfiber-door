//! The guarded door: the one process the household reaches the media server's bytes
//! through. It serves the household on 8096 and, encrypted, on 8920, and speaks
//! `stream.guard@1` to the core on 8081. Run as `door version`, it says which release
//! it is.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::process::ExitCode;

/// Where the core writes what the door reads, read-only inside its container.
const CONFIG: &str = "/config";

/// Where the door keeps the envelope the core applies, its one writable directory.
const STATE: &str = "/state";

/// Every address inside the container.
const ANY: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

#[tokio::main]
async fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None => serve().await,
        Some("version") => {
            println!("lemonfiber-door {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!(
                "door: no command called {other}; run it bare to serve, or `version` to say which release it is"
            );
            ExitCode::from(2)
        }
    }
}

async fn serve() -> ExitCode {
    let ports = lemonfiber_door::Ports {
        plain: SocketAddr::new(ANY, 8096),
        encrypted: SocketAddr::new(ANY, 8920),
        contract: SocketAddr::new(ANY, 8081),
    };
    let stopped = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    match lemonfiber_door::run(Path::new(CONFIG), Path::new(STATE), ports, stopped).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("door: {why}");
            ExitCode::FAILURE
        }
    }
}
