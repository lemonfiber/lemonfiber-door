//! The declaration a guard enforces, compiled, and what it decides about one request.
//!
//! Every pattern is matched against the path as its bytes arrived and as it reads
//! decoded once, and holds where either matches. A byte or live-stream path carries no
//! `%` at all, so the item or the stream it names is the same in both. An item found in
//! a query value is read from that value decoded once, which then holds no `%`.

use std::fmt::Write as _;

use lemonfiber_ports::service::{Answers, Declared, Guarded, Held, Statuses, STREAM_NAME};
use regex::{Regex, RegexBuilder};

use crate::target::{self, Pair, Target};

/// The group a byte path names its item with.
const ITEM: &str = "item";

/// Where the question names the item.
const ITEM_PLACE: &str = "{item}";

/// The query parameter a live-stream pass travels in.
pub(crate) const PASS: &str = "lfpass";

/// The most a compiled pattern may take, in bytes.
const COMPILED_MOST: usize = 1 << 20;

/// The scheme a bearer token is presented under, matched without regard to case.
const BEARER: &str = "bearer ";

/// The scheme a bearer token is presented to the adapter under.
const BEARING: &str = "Bearer ";

/// The header a bearer token is presented in.
const AUTHORIZATION: &str = "authorization";

/// The status a request is refused with.
pub(crate) const REFUSED: u16 = 403;

/// The methods that read what a byte or live-stream path serves, the only ones a
/// question about it answers for.
const READING: [&str; 2] = ["GET", "HEAD"];

/// The bytes a query value reads otherwise once decoded, which a token in one never
/// holds.
const DECODED_OTHERWISE: [char; 2] = ['%', '+'];

/// A path that serves an item's bytes, compiled.
#[derive(Debug)]
struct Bytes {
    pattern: Regex,
    query: Option<InQuery>,
    space: Option<String>,
    item: Regex,
    exempt: Vec<Regex>,
    naming: Vec<(String, Naming)>,
}

/// The query parameter a byte path finds its item in, and the pattern its value, decoded
/// once, matches whole, compiled.
#[derive(Debug)]
struct InQuery {
    parameter: String,
    pattern: Regex,
}

/// What a parameter naming another item is held to, compiled.
#[derive(Debug)]
enum Naming {
    Refused,
    Asked(Regex),
}

/// A path that serves a live stream's bytes, compiled.
#[derive(Debug)]
struct Live {
    pattern: Regex,
    stream: Regex,
}

/// A guard's envelope, compiled into what it decides with.
#[derive(Debug)]
pub(crate) struct Rules {
    guarded: Guarded,
    refused: Vec<Regex>,
    bytes: Vec<Bytes>,
    live: Vec<Live>,
    token: Regex,
}

/// Where a request's member token is presented to the media server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Presenting {
    /// Its headers as they came.
    Unchanged,
    /// Its `Authorization` header replaced with this one.
    Header {
        /// The header's name.
        name: String,
        /// Its value.
        value: String,
    },
    /// Its `Authorization` header taken off, and the token put in the query.
    Removed,
}

/// What must be asked before a request goes on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Check {
    /// Nothing.
    Nothing,
    /// The declared question, about each of these items, at `to`.
    Question {
        /// Where the question is sent: its scheme, host and port.
        to: String,
        /// The question's method.
        method: lemonfiber_ports::http::Method,
        /// The path and query of the question about each item, in order.
        asked: Vec<String>,
        /// What each answer means.
        answers: Answers,
        /// What the question carries besides its path.
        carrying: Carrying,
    },
    /// The pass service, whether the pass the request carries is good.
    Pass {
        /// Where it is asked.
        at: String,
    },
}

/// What a question carries besides its path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Carrying {
    /// The request's own headers, as the media server is sent them.
    Request,
    /// The member's token alone, in this header where it is presented in one, and no
    /// other header that could carry a credential.
    Token(Option<(String, String)>),
}

/// A request that goes on, once its check says it may.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Going {
    /// The target forwarded: the one that arrived, with what the door takes off or puts
    /// in.
    pub(crate) target: String,
    /// How the member's token is presented.
    pub(crate) presenting: Presenting,
    /// What is asked first.
    pub(crate) check: Check,
}

/// What a request is, once read against the declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Refused with this status, nothing asked.
    Refuse(u16),
    /// Goes on, once its check says it may.
    Forward(Box<Going>),
}

/// Why an envelope cannot be compiled: the pattern that does not compile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Uncompiled(pub(crate) String);

impl From<Uncompiled> for String {
    fn from(uncompiled: Uncompiled) -> Self {
        format!("the pattern {} does not compile", uncompiled.0)
    }
}

impl Rules {
    /// `guarded`, compiled.
    pub(crate) fn of(guarded: Guarded) -> Result<Self, Uncompiled> {
        let declared: &Declared = &guarded.declared;
        let refused = declared
            .refused
            .iter()
            .map(|pattern| compiled(pattern))
            .collect::<Result<_, _>>()?;
        let bytes = declared
            .bytes
            .iter()
            .map(|path| {
                Ok(Bytes {
                    pattern: from_start(&path.pattern)?,
                    query: path
                        .query
                        .as_ref()
                        .map(|query| {
                            Ok(InQuery {
                                parameter: query.parameter.clone(),
                                pattern: whole(&query.pattern)?,
                            })
                        })
                        .transpose()?,
                    space: path.space.clone(),
                    item: whole(&path.item)?,
                    exempt: path
                        .exempt
                        .iter()
                        .map(|pattern| from_start(pattern))
                        .collect::<Result<_, _>>()?,
                    naming: path
                        .naming
                        .iter()
                        .map(|naming| {
                            Ok((
                                naming.parameter.clone(),
                                match &naming.held {
                                    Held::Refused => Naming::Refused,
                                    Held::Asked { shape } => Naming::Asked(whole(shape)?),
                                },
                            ))
                        })
                        .collect::<Result<_, Uncompiled>>()?,
                })
            })
            .collect::<Result<_, Uncompiled>>()?;
        let live = declared
            .live
            .iter()
            .map(|path| {
                Ok(Live {
                    pattern: from_start(&path.pattern)?,
                    stream: whole(&path.stream)?,
                })
            })
            .collect::<Result<_, Uncompiled>>()?;
        let token = whole(&declared.token.shape)?;
        Ok(Self {
            guarded,
            refused,
            bytes,
            live,
            token,
        })
    }

    /// The envelope these rules were compiled from.
    pub(crate) const fn guarded(&self) -> &Guarded {
        &self.guarded
    }

    /// What a request with `method` on `raw`, its target as its bytes arrived, carrying
    /// the `Authorization` headers `authorization` and, of the headers
    /// [`Self::token_headers`] names, `headers`, each with its name, is.
    pub(crate) fn decide(
        &self,
        method: &str,
        raw: &str,
        authorization: &[&[u8]],
        headers: &[(&str, &[u8])],
    ) -> Decision {
        let Ok(target) = target::split(raw) else {
            return Decision::Refuse(REFUSED);
        };
        if target::normal(target).is_err() {
            return Decision::Refuse(REFUSED);
        }
        let decoded = target::decoded(target.path);
        let pairs = target::pairs(target.query);
        let Some((read, presenting)) = self.read(authorization, &pairs, headers) else {
            return Decision::Refuse(REFUSED);
        };
        let bearer = read.bearer;
        let either = |pattern: &Regex| pattern.is_match(target.path) || pattern.is_match(&decoded);
        if self.refused.iter().any(either) {
            return Decision::Refuse(REFUSED);
        }
        let forwarded =
            |leaving: Option<&str>| forwarded(target, &pairs, leaving, self.put(bearer));
        let reading = READING.contains(&method);
        if let Some(live) = self.live.iter().find(|live| either(&live.pattern)) {
            let naming_another = self
                .bytes
                .iter()
                .filter(|bytes| either(&bytes.pattern))
                .flat_map(|bytes| &bytes.naming)
                .any(|(parameter, _)| {
                    pairs
                        .iter()
                        .any(|pair| pair.name.eq_ignore_ascii_case(parameter))
                });
            if naming_another || !reading || !Self::passable(live, target, &pairs) {
                return Decision::Refuse(REFUSED);
            }
            return Decision::Forward(Box::new(Going {
                target: forwarded(Some(PASS)),
                presenting,
                check: Check::Pass {
                    at: self.guarded.pass.clone(),
                },
            }));
        }
        if let Some(bytes) = self.bytes.iter().find(|bytes| either(&bytes.pattern)) {
            if bytes.exempt.iter().any(either) && !target.path.contains('%') {
                return Decision::Forward(Box::new(Going {
                    target: forwarded(None),
                    presenting,
                    check: Check::Nothing,
                }));
            }
            if !reading {
                return Decision::Refuse(REFUSED);
            }
            return match self.asked(bytes, target, &pairs, read) {
                Ok(check) => Decision::Forward(Box::new(Going {
                    target: forwarded(None),
                    presenting,
                    check,
                })),
                Err(status) => Decision::Refuse(status),
            };
        }
        Decision::Forward(Box::new(Going {
            target: forwarded(None),
            presenting,
            check: Check::Nothing,
        }))
    }

    /// Whether a request on `live` names a live stream of its declared shape and carries
    /// its pass once, in its one spelling, holding something.
    fn passable(live: &Live, target: Target<'_>, pairs: &[Pair<'_>]) -> bool {
        let named = !target.path.contains('%')
            && pairs.iter().all(|pair| plain(pair.name))
            && live
                .pattern
                .captures(target.path)
                .and_then(|captured| captured.name(STREAM_NAME))
                .is_some_and(|stream| live.stream.is_match(stream.as_str()));
        let passes: Vec<&Pair<'_>> = pairs
            .iter()
            .filter(|pair| pair.name.eq_ignore_ascii_case(PASS))
            .collect();
        named
            && matches!(
                passes.as_slice(),
                [pair] if pair.name == PASS && pair.value.is_some_and(|value| !value.is_empty())
            )
    }
}

/// What an answer to the question with `status` means: `Ok` where the member may have
/// the item, and the status the request is refused with otherwise.
pub(crate) fn answered(answers: &Answers, status: u16) -> Result<(), u16> {
    let within = |classes: &[Statuses]| {
        classes
            .iter()
            .any(|class| (class.from..=class.to).contains(&status))
    };
    if within(&answers.may) {
        Ok(())
    } else if within(&answers.may_not)
        || within(&answers.no_session)
        || within(&answers.not_from_there)
    {
        Err(status)
    } else {
        Err(REFUSED)
    }
}

/// The target a request goes on with: its path, and its query without the parameter
/// `leaving` and with the token put in the query where the declaration presents it
/// there.
fn forwarded(
    target: Target<'_>,
    pairs: &[Pair<'_>],
    leaving: Option<&str>,
    put: Option<(&str, &str)>,
) -> String {
    if leaving.is_none() && put.is_none() {
        return format!(
            "{}{}",
            target.path,
            target
                .query
                .map(|query| format!("?{query}"))
                .unwrap_or_default()
        );
    }
    let mut kept: Vec<String> = pairs
        .iter()
        .filter(|pair| Some(pair.name) != leaving)
        .map(|pair| pair.raw.to_owned())
        .collect();
    if let Some((parameter, value)) = put {
        kept.push(format!("{parameter}={}", encoded(value)));
    }
    if kept.is_empty() {
        target.path.to_owned()
    } else {
        format!("{}?{}", target.path, kept.join("&"))
    }
}

/// The token an `Authorization` header presents as a bearer, where it does.
fn bearer(header: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(header).ok()?;
    let scheme = text.get(..BEARER.len())?;
    scheme
        .eq_ignore_ascii_case(BEARER)
        .then(|| text.get(BEARER.len()..))
        .flatten()
}

/// Whether `name` is a plain parameter name: ASCII letters, digits, `_`, `-` and `.`.
fn plain(name: &str) -> bool {
    name.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

/// `pattern`, compiled within the bound a declaration is held to, to match only a whole
/// value.
fn whole(pattern: &str) -> Result<Regex, Uncompiled> {
    bounded(&format!(r"\A(?:{pattern})\z"), pattern)
}

/// `pattern`, compiled within the bound a declaration is held to, to match only from the
/// start of a path.
fn from_start(pattern: &str) -> Result<Regex, Uncompiled> {
    bounded(&format!(r"\A(?:{pattern})"), pattern)
}

/// `value`, written into a query: every byte but an unreserved one percent-encoded, so it
/// reads as one value and adds no parameter.
fn encoded(value: &str) -> String {
    value.bytes().fold(String::new(), |mut out, byte| {
        if target::unreserved(byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
        out
    })
}

/// `pattern`, compiled within the bound a declaration is held to.
fn compiled(pattern: &str) -> Result<Regex, Uncompiled> {
    bounded(pattern, pattern)
}

/// `built`, compiled within the bound a declaration is held to; where it does not
/// compile, the declared `pattern` named.
fn bounded(built: &str, pattern: &str) -> Result<Regex, Uncompiled> {
    RegexBuilder::new(built)
        .size_limit(COMPILED_MOST)
        .build()
        .map_err(|_| Uncompiled(pattern.to_owned()))
}

mod asking;
mod reading;

#[cfg(test)]
mod tests;
