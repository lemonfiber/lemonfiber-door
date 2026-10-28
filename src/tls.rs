//! The certificate the door presents on its encrypted port: the one the core writes
//! beside its configuration, read again whenever either file changes, and what a client
//! pins it by.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;

/// The certificate, as the core writes it.
pub(crate) const CERTIFICATE: &str = "certificate.pem";

/// Its private key.
pub(crate) const KEY: &str = "key.pem";

/// When each file was last changed, as last read.
type Stamped = (Option<SystemTime>, Option<SystemTime>);

/// The certificate in a configuration directory, read again whenever it changes.
#[derive(Debug)]
pub(crate) struct Certificates {
    directory: PathBuf,
    held: Mutex<Option<(Stamped, Arc<CertifiedKey>)>>,
}

impl Certificates {
    /// The certificate kept in `directory`.
    pub(crate) fn in_directory(directory: &Path) -> Self {
        Self {
            directory: directory.to_path_buf(),
            held: Mutex::new(None),
        }
    }

    /// The certificate and key as they stand, where both are there and go together.
    pub(crate) fn current(&self) -> Option<Arc<CertifiedKey>> {
        let stamped = (
            changed(&self.directory.join(CERTIFICATE)),
            changed(&self.directory.join(KEY)),
        );
        let mut held = self.held.lock().ok()?;
        if let Some((at, key)) = held.as_ref() {
            if *at == stamped {
                return Some(Arc::clone(key));
            }
        }
        let read = Arc::new(certified(&self.directory)?);
        *held = Some((stamped, Arc::clone(&read)));
        Some(read)
    }

    /// What a client pins the certificate presented now by: SHA-256 over its DER
    /// encoding, in lower-case hexadecimal.
    pub(crate) fn fingerprint(&self) -> Option<String> {
        let current = self.current()?;
        let leaf = current.cert.first()?;
        Some(hexadecimal(
            ring::digest::digest(&ring::digest::SHA256, leaf).as_ref(),
        ))
    }
}

impl ResolvesServerCert for Certificates {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.current()
    }
}

/// The server configuration that presents `certificates`, speaking HTTP/2 and HTTP/1.1.
///
/// # Errors
///
/// Where the crypto provider offers none of the default protocol versions.
pub(crate) fn served(
    certificates: Arc<Certificates>,
) -> Result<Arc<rustls::ServerConfig>, rustls::Error> {
    let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_cert_resolver(certificates);
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// The certificate chain and key written in `directory`.
fn certified(directory: &Path) -> Option<CertifiedKey> {
    let chain: Vec<CertificateDer<'static>> =
        CertificateDer::pem_file_iter(directory.join(CERTIFICATE))
            .ok()?
            .collect::<Result<_, _>>()
            .ok()?;
    let key = PrivateKeyDer::from_pem_file(directory.join(KEY)).ok()?;
    let signing = rustls::crypto::ring::sign::any_supported_type(&key).ok()?;
    let certified = CertifiedKey::new(chain, signing);
    certified.keys_match().ok()?;
    (!certified.cert.is_empty()).then_some(certified)
}

/// When `path` last changed, where it can be read.
fn changed(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

/// `bytes` in lower-case hexadecimal.
fn hexadecimal(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
pub(crate) mod tests;
