//! The guarded door: the one process the household reaches the media server's bytes
//! through. It serves the household on 8096 and, encrypted, on 8920, and speaks
//! `stream.guard@1` to the core on 8081.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::process::ExitCode;

/// Where the core writes what the door holds, inside its container.
const CONFIG: &str = "/config";

/// Every address inside the container.
const ANY: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

#[tokio::main]
async fn main() -> ExitCode {
    let ports = lemonfiber_door::Ports {
        plain: SocketAddr::new(ANY, 8096),
        encrypted: SocketAddr::new(ANY, 8920),
        contract: SocketAddr::new(ANY, 8081),
    };
    let stopped = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    match lemonfiber_door::run(Path::new(CONFIG), ports, stopped).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("door: {why}");
            ExitCode::FAILURE
        }
    }
}
