use std::net::IpAddr;

use super::client;

const PROXY: &str = "10.80.96.3";

fn ip(text: &str) -> IpAddr {
    text.parse()
        .unwrap_or_else(|_| unreachable!("{text} is an address"))
}

#[test]
fn the_client_is_whoever_connected_where_that_is_not_the_proxy() {
    assert_eq!(
        client(ip("192.168.1.20"), &[b"1.2.3.4"], PROXY),
        ip("192.168.1.20")
    );
}

#[test]
fn the_proxy_is_believed_for_the_rightmost_entry_it_sent() {
    assert_eq!(
        client(ip(PROXY), &[b"6.6.6.6, 192.168.1.20"], PROXY),
        ip("192.168.1.20")
    );
    assert_eq!(
        client(ip(PROXY), &[b"6.6.6.6", b"192.168.1.21 , "], PROXY),
        ip("192.168.1.21")
    );
    assert_eq!(
        client(ip(PROXY), &[b"[fe80::1]:51000"], PROXY),
        ip("fe80::1")
    );
}

#[test]
fn the_proxy_naming_nobody_readable_leaves_the_proxy_as_the_client() {
    for forwarded in [&b""[..], b" , ", b"unknown", b"\xff"] {
        assert_eq!(client(ip(PROXY), &[forwarded], PROXY), ip(PROXY));
    }
    assert_eq!(client(ip(PROXY), &[], PROXY), ip(PROXY));
}

#[test]
fn an_address_mapped_into_ipv6_is_read_as_itself() {
    assert_eq!(
        client(ip("::ffff:10.80.96.3"), &[b"192.168.1.20"], PROXY),
        ip("192.168.1.20")
    );
    assert_eq!(
        client(ip("::ffff:192.168.1.9"), &[b"1.2.3.4"], PROXY),
        ip("192.168.1.9")
    );
}

#[test]
fn a_proxy_named_with_a_port_or_not_at_all_is_read_as_such() {
    assert_eq!(
        client(ip("10.80.96.3"), &[b"192.168.1.20"], "10.80.96.3:443"),
        ip("192.168.1.20")
    );
    assert_eq!(
        client(ip("10.80.96.3"), &[b"192.168.1.20"], "proxy"),
        ip("10.80.96.3")
    );
}
