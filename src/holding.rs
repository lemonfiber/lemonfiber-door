//! What the core applies over `stream.guard@1`, held: the envelope written whole into
//! the configuration directory, by renaming a copy over it, readable by the door's user
//! alone, and compiled into the rules every request is read against. A door that holds
//! none, or holds one it cannot compile, guards nothing and serves nothing.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use lemonfiber_ports::service::{Failure, Guarded, Guarding, Presented};

use crate::rules::Rules;
use crate::tls::Certificates;

/// The file the envelope is kept in.
pub(crate) const ENVELOPE: &str = "guard.json";

/// The name the door gives itself in a failure.
const SERVICE: &str = "door";

/// Who may read what it keeps: its own user alone.
const OWNER_ONLY: u32 = 0o600;

/// The envelope the door enforces, and the certificate it presents.
pub(crate) struct Holding {
    config: PathBuf,
    rules: RwLock<Option<Arc<Rules>>>,
    writing: tokio::sync::Mutex<()>,
    certificates: Arc<Certificates>,
}

impl Holding {
    /// What is kept in `config`, the envelope compiled where it can be.
    pub(crate) fn in_directory(config: &Path, certificates: Arc<Certificates>) -> Self {
        let rules = std::fs::read_to_string(config.join(ENVELOPE))
            .ok()
            .and_then(|text| serde_json::from_str::<Guarded>(&text).ok())
            .and_then(|guarded| compiled(guarded).ok())
            .map(Arc::new);
        Self {
            config: config.to_path_buf(),
            rules: RwLock::new(rules),
            writing: tokio::sync::Mutex::new(()),
            certificates,
        }
    }

    /// What `rules` enforce, held in `config` without being written there.
    #[cfg(test)]
    pub(crate) fn enforcing(config: &Path, rules: Rules) -> Self {
        Self {
            config: config.to_path_buf(),
            rules: RwLock::new(Some(Arc::new(rules))),
            writing: tokio::sync::Mutex::new(()),
            certificates: Arc::new(Certificates::in_directory(config)),
        }
    }

    /// The rules every request is read against now, where an envelope is held.
    pub(crate) fn rules(&self) -> Option<Arc<Rules>> {
        self.rules.read().ok().and_then(|held| held.clone())
    }

    /// Write `guarded` to the envelope's file, whole, from a copy made afresh.
    async fn written(&self, guarded: Guarded) -> Result<(), Failure> {
        let record = self.config.join(ENVELOPE);
        let writing = self.config.join(format!("{ENVELOPE}.writing"));
        let done = tokio::task::spawn_blocking(move || {
            use std::io::Write as _;
            use std::os::unix::fs::OpenOptionsExt as _;
            match std::fs::remove_file(&writing) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
            let mut out = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(OWNER_ONLY)
                .open(&writing)?;
            serde_json::to_writer(&mut out, &guarded)?;
            out.flush()?;
            out.sync_all()?;
            std::fs::rename(&writing, &record)
        })
        .await;
        done.ok()
            .and_then(Result::ok)
            .ok_or_else(|| refused(&format!("{ENVELOPE} could not be written")))
    }
}

#[async_trait]
impl Guarding for Holding {
    async fn guard(&self, guarded: &Guarded) -> Result<(), Failure> {
        let rules = compiled(guarded.clone()).map_err(|why| refused(&why))?;
        let _writing = self.writing.lock().await;
        self.written(guarded.clone()).await?;
        let mut held = self
            .rules
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *held = Some(Arc::new(rules));
        Ok(())
    }

    async fn guarding(&self) -> Result<Option<Guarded>, Failure> {
        Ok(self.rules().map(|rules| rules.guarded().clone()))
    }

    async fn certificate(&self) -> Result<Presented, Failure> {
        self.certificates
            .fingerprint()
            .map(|fingerprint| Presented { fingerprint })
            .ok_or_else(|| refused("it holds no certificate to present"))
    }
}

/// `guarded` compiled, where its declaration is one a guard may be handed and every
/// address it names is one the door can reach, or why not.
fn compiled(guarded: Guarded) -> Result<Rules, String> {
    let declared = guarded
        .declared
        .clone()
        .checked()
        .map_err(|why| why.to_string())?;
    if !bare(&guarded.upstream) || !guarded.adapter.as_deref().is_none_or(bare) {
        return Err(
            "it names the media server or its adapter somewhere it cannot reach".to_owned(),
        );
    }
    if !located(&guarded.pass) {
        return Err("it names the pass service somewhere it cannot reach".to_owned());
    }
    Rules::of(Guarded {
        declared,
        ..guarded
    })
    .map_err(String::from)
}

/// Whether `address` is plain HTTP to a host and port, and nothing more.
fn bare(address: &str) -> bool {
    located(address)
        && address
            .parse::<http::Uri>()
            .is_ok_and(|uri| uri.path() == "/")
}

/// Whether `address` is plain HTTP to a host, a path at most, and no user or query.
fn located(address: &str) -> bool {
    address.parse::<http::Uri>().is_ok_and(|uri| {
        uri.scheme_str() == Some("http")
            && uri.query().is_none()
            && uri
                .authority()
                .is_some_and(|authority| !authority.as_str().contains('@'))
    })
}

/// A refusal saying `detail`.
fn refused(detail: &str) -> Failure {
    Failure::Refused {
        service: SERVICE.to_owned(),
        detail: detail.to_owned(),
    }
}

#[cfg(test)]
mod tests;
