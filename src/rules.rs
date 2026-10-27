//! The declaration a guard enforces, compiled, and what it decides about one request.
//!
//! Every pattern is matched against the path as its bytes arrived and as it reads
//! decoded once, and holds where either matches. A byte or live-stream path carries no
//! `%` at all, so the item or the stream it names is the same in both.

use std::fmt::Write as _;

use lemonfiber_ports::service::{
    Addressee, Answers, Declared, Guarded, Held, Presentation, Question, Statuses, Token,
    STREAM_NAME, TOKEN,
};
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

/// The status a request is refused with.
pub(crate) const REFUSED: u16 = 403;

/// A path that serves an item's bytes, compiled.
#[derive(Debug)]
struct Bytes {
    pattern: Regex,
    item: Regex,
    exempt: Vec<Regex>,
    naming: Vec<(String, Naming)>,
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
    },
    /// The pass service, whether the pass the request carries is good.
    Pass {
        /// Where it is asked.
        at: String,
    },
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
                    pattern: compiled(&path.pattern)?,
                    item: compiled(&path.item)?,
                    exempt: path
                        .exempt
                        .iter()
                        .map(|pattern| compiled(pattern))
                        .collect::<Result<_, _>>()?,
                    naming: path
                        .naming
                        .iter()
                        .map(|naming| {
                            Ok((
                                naming.parameter.clone(),
                                match &naming.held {
                                    Held::Refused => Naming::Refused,
                                    Held::Asked { shape } => Naming::Asked(compiled(shape)?),
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
                    pattern: compiled(&path.pattern)?,
                    stream: compiled(&path.stream)?,
                })
            })
            .collect::<Result<_, Uncompiled>>()?;
        let token = compiled(&declared.token.shape)?;
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

    /// What `raw`, a request's target as its bytes arrived, carrying the `Authorization`
    /// headers `authorization`, is.
    pub(crate) fn decide(&self, raw: &str, authorization: &[&[u8]]) -> Decision {
        let Ok(target) = target::split(raw) else {
            return Decision::Refuse(REFUSED);
        };
        if target::normal(target).is_err() {
            return Decision::Refuse(REFUSED);
        }
        let decoded = target::decoded(target.path);
        let pairs = target::pairs(target.query);
        let Some((bearer, presenting)) = self.presented(authorization, &pairs) else {
            return Decision::Refuse(REFUSED);
        };
        let either = |pattern: &Regex| pattern.is_match(target.path) || pattern.is_match(&decoded);
        if self.refused.iter().any(either) {
            return Decision::Refuse(REFUSED);
        }
        let forwarded =
            |leaving: Option<&str>| forwarded(target, &pairs, leaving, self.put(bearer));
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
            if naming_another || !Self::passable(live, target, &pairs) {
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
            return match self.asked(bytes, target, &pairs, bearer) {
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

    /// The bearer token the request presents, where it presents one of the declared
    /// shape, and how it is presented to the media server; nothing where the request
    /// presents a token more than once, or a token parameter under another spelling,
    /// twice or in another shape.
    fn presented<'a>(
        &self,
        authorization: &[&'a [u8]],
        pairs: &[Pair<'_>],
    ) -> Option<(Option<&'a str>, Presenting)> {
        let token: &Token = &self.guarded.declared.token;
        let mut carried = 0;
        for pair in pairs {
            let Some(declared) = token
                .parameters
                .iter()
                .find(|declared| declared.eq_ignore_ascii_case(pair.name))
            else {
                continue;
            };
            let shaped = pair.value.is_some_and(|value| self.token.is_match(value));
            if declared != pair.name || !shaped {
                return None;
            }
            carried += 1;
        }
        let bearer = match authorization {
            [] => None,
            [one] => bearer(one).filter(|value| self.token.is_match(value)),
            _ => return None,
        };
        if carried > 1 || (carried == 1 && bearer.is_some()) {
            return None;
        }
        let presenting = match (bearer, &token.presented) {
            (None, _) | (Some(_), Presentation::AsIs) => Presenting::Unchanged,
            (Some(value), Presentation::Header { name, value: shown }) => Presenting::Header {
                name: name.clone(),
                value: shown.replace(TOKEN, value),
            },
            (Some(_), Presentation::Query { .. }) => Presenting::Removed,
        };
        Some((bearer, presenting))
    }

    /// The query parameter a bearer token is put in, with it, where the declaration
    /// presents a token in the query.
    fn put<'a>(&'a self, bearer: Option<&'a str>) -> Option<(&'a str, &'a str)> {
        match (&self.guarded.declared.token.presented, bearer) {
            (Presentation::Query { parameter }, Some(value)) => Some((parameter.as_str(), value)),
            _ => None,
        }
    }

    /// The question a request on `bytes` is asked about, or the status it is refused
    /// with.
    fn asked(
        &self,
        bytes: &Bytes,
        target: Target<'_>,
        pairs: &[Pair<'_>],
        bearer: Option<&str>,
    ) -> Result<Check, u16> {
        let path = target.path;
        if path.contains('%') || pairs.iter().any(|pair| !plain(pair.name)) {
            return Err(REFUSED);
        }
        let item = bytes
            .pattern
            .captures(path)
            .and_then(|captured| captured.name(ITEM))
            .map(|matched| matched.as_str())
            .filter(|item| bytes.item.is_match(item))
            .ok_or(REFUSED)?;
        let mut items = vec![item];
        for (parameter, naming) in &bytes.naming {
            let named: Vec<&Pair<'_>> = pairs
                .iter()
                .filter(|pair| pair.name.eq_ignore_ascii_case(parameter))
                .collect();
            match (naming, named.as_slice()) {
                (_, []) => {}
                (Naming::Asked(shape), [pair])
                    if pair.name == parameter
                        && pair.value.is_some_and(|value| shape.is_match(value)) =>
                {
                    items.extend(pair.value);
                }
                _ => return Err(REFUSED),
            }
        }
        let question: &Question = self.guarded.declared.question.as_ref().ok_or(REFUSED)?;
        let to = match question.to {
            Addressee::Upstream => self.guarded.upstream.clone(),
            Addressee::Adapter => self.guarded.adapter.clone().ok_or(REFUSED)?,
        };
        let carried: Vec<&str> = pairs
            .iter()
            .filter(|pair| {
                self.guarded
                    .declared
                    .token
                    .parameters
                    .iter()
                    .any(|declared| declared == pair.name)
            })
            .map(|pair| pair.raw)
            .collect();
        let put = self.put(bearer);
        let asked = items
            .iter()
            .map(|item| {
                let mut asked = question.path.replace(ITEM_PLACE, item);
                let mut joiner = if asked.contains('?') { '&' } else { '?' };
                for raw in &carried {
                    let _ = write!(asked, "{joiner}{raw}");
                    joiner = '&';
                }
                if let Some((parameter, value)) = put {
                    let _ = write!(asked, "{joiner}{parameter}={value}");
                }
                asked
            })
            .collect();
        Ok(Check::Question {
            to,
            method: question.method,
            asked,
            answers: question.answers.clone(),
        })
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
        kept.push(format!("{parameter}={value}"));
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

/// `pattern`, compiled within the bound a declaration is held to.
fn compiled(pattern: &str) -> Result<Regex, Uncompiled> {
    RegexBuilder::new(pattern)
        .size_limit(COMPILED_MOST)
        .build()
        .map_err(|_| Uncompiled(pattern.to_owned()))
}

#[cfg(test)]
mod tests;
