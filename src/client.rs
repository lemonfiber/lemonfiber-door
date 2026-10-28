//! Which client a request came from: the address that connected, or, where that is the
//! one proxy the guard believes, the rightmost entry that proxy put in
//! `X-Forwarded-For`.

use std::net::{IpAddr, SocketAddr};

/// The client of a request that connected from `peer` carrying the `X-Forwarded-For`
/// headers `forwarded`, believing them only where `peer` is `proxy`.
pub(crate) fn client(peer: IpAddr, forwarded: &[&[u8]], proxy: &str) -> IpAddr {
    let peer = peer.to_canonical();
    if address(proxy) != Some(peer) {
        return peer;
    }
    forwarded
        .iter()
        .filter_map(|value| std::str::from_utf8(value).ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .rfind(|entry| !entry.is_empty())
        .and_then(address)
        .unwrap_or(peer)
}

/// The address `text` names: an address alone, or one with a port.
fn address(text: &str) -> Option<IpAddr> {
    text.parse::<IpAddr>()
        .or_else(|_| text.parse::<SocketAddr>().map(|at| at.ip()))
        .ok()
        .map(|ip| ip.to_canonical())
}

#[cfg(test)]
mod tests;
