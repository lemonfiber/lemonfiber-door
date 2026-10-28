//! The guarded door: the one service the household reaches the media server's bytes
//! through, holding each request to the declaration the core applies over
//! `stream.guard@1`.

pub(crate) mod client;
mod door;
pub(crate) mod holding;
pub(crate) mod listening;
pub(crate) mod rules;
pub(crate) mod serving;
pub(crate) mod speaking;
pub(crate) mod target;
pub(crate) mod tls;

pub use door::{run, Ports};

#[cfg(test)]
pub(crate) mod fake;
#[cfg(test)]
pub(crate) mod fixtures;
