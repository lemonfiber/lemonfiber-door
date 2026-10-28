use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::{served, Certificates, CERTIFICATE, KEY};
use crate::fixtures::Scratch;

/// A certificate and its key, made for one test, written into `directory`.
pub(crate) fn written(directory: &Path) -> Vec<u8> {
    let made = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])
        .unwrap_or_else(|why| unreachable!("a certificate is made: {why}"));
    let _ = std::fs::write(directory.join(CERTIFICATE), made.cert.pem());
    let _ = std::fs::write(directory.join(KEY), made.signing_key.serialize_pem());
    made.cert.der().to_vec()
}

fn hex_sha256(bytes: &[u8]) -> String {
    super::hexadecimal(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
}

#[test]
fn the_certificate_written_is_presented_and_pinned_by_its_digest() {
    let directory = Scratch::new();
    let der = written(directory.path());
    let certificates = Certificates::in_directory(directory.path());
    let current = certificates.current();
    assert_eq!(
        current.and_then(|key| key.cert.first().map(|leaf| leaf.to_vec())),
        Some(der.clone())
    );
    assert_eq!(certificates.fingerprint(), Some(hex_sha256(&der)));
}

#[test]
fn a_certificate_replaced_is_read_again_and_one_unchanged_is_kept() {
    let directory = Scratch::new();
    written(directory.path());
    let certificates = Certificates::in_directory(directory.path());
    let first = certificates.current();
    let again = certificates.current();
    assert!(matches!((&first, &again), (Some(one), Some(two)) if Arc::ptr_eq(one, two)));
    let der = written(directory.path());
    let later = SystemTime::now() + Duration::from_secs(60);
    for file in [CERTIFICATE, KEY] {
        let _ = std::fs::File::options()
            .write(true)
            .open(directory.path().join(file))
            .and_then(|handle| handle.set_modified(later));
    }
    assert_eq!(certificates.fingerprint(), Some(hex_sha256(&der)));
}

#[test]
fn nothing_is_presented_where_a_file_is_missing_empty_or_the_key_is_another() {
    let directory = Scratch::new();
    let certificates = Certificates::in_directory(directory.path());
    assert!(certificates.current().is_none());
    assert!(certificates.fingerprint().is_none());
    written(directory.path());
    let _ = std::fs::write(directory.path().join(CERTIFICATE), "");
    assert!(certificates.current().is_none());
    let other = Scratch::new();
    written(other.path());
    written(directory.path());
    let _ = std::fs::copy(other.path().join(KEY), directory.path().join(KEY));
    assert!(Certificates::in_directory(directory.path())
        .current()
        .is_none());
    let _ = std::fs::write(directory.path().join(KEY), "not a key");
    assert!(Certificates::in_directory(directory.path())
        .current()
        .is_none());
    let _ = std::fs::write(
        directory.path().join(CERTIFICATE),
        "-----BEGIN CERTIFICATE-----\n!!!!\n-----END CERTIFICATE-----\n",
    );
    assert!(Certificates::in_directory(directory.path())
        .current()
        .is_none());
}

#[test]
fn the_server_speaks_http2_and_http11() {
    let directory = Scratch::new();
    let config = served(Arc::new(Certificates::in_directory(directory.path())));
    assert_eq!(
        config.map(|config| config.alpn_protocols.clone()).ok(),
        Some(vec![b"h2".to_vec(), b"http/1.1".to_vec()])
    );
}
