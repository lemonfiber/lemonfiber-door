//! The declaration a guard enforces, compiled, and what it decides about one request.
//!
//! Every pattern is matched against the path as its bytes arrived and as it reads
//! decoded once, and holds where either matches. A byte or live-stream path carries no
//! `%` at all, so the item or the stream it names is the same in both. An item found in
//! a query value is read from that value decoded once, which then holds no `%`.

use std::fmt::Write as _;

use lemonfiber_ports::service::{
    Answers, Declared, Guarded, Held, Presentation, Statuses, STREAM_NAME, TOKEN,
};
use regex::{Regex, RegexBuilder};

use self::reading::Read;
use crate::target::{self, Pair, Target};

/// The group a byte path names its item with.
const ITEM: &str = "item";

/// Where the question names the item.
const ITEM_PLACE: &str = "{item}";

pub(crate) use lemonfiber_sidecar::pass::PARAMETER as PASS;

/// The longest pass the door reads, in bytes.
const PASS_MOST: usize = 1 << 10;

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

/// The status the adapter answers its question with where its upstream cannot be asked.
const UNASKED: u16 = 502;

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

/// A path that serves a live stream's bytes, compiled, and the query parameter it finds
/// the live stream in where its path does not hold it.
#[derive(Debug)]
struct Live {
    pattern: Regex,
    parameter: Option<String>,
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
    /// Where the pass service is asked whether the pass it carries is good, before its
    /// check is.
    pub(crate) pass_service: Option<String>,
    /// What is written into, and held back from, what it is answered with, where it goes
    /// on a pass.
    pub(crate) writing: Option<Writing>,
}

/// What the door writes into, and holds back from, the answer to a request it lets through
/// on a pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Writing {
    /// The parameters written into every address of a playlist it is answered with, each
    /// as the request carried it: its pass, and on a live-stream pass the parameter the
    /// live stream is named in where it is named in one.
    pub(crate) carried: Vec<(String, String)>,
    /// The parameters a member's token reaches the upstream in, taken off every address
    /// of a playlist it is answered with.
    pub(crate) stripped: Vec<String>,
    /// The session the request was presented to the upstream with, which nothing it is
    /// answered with may hold.
    pub(crate) session: Option<String>,
}

/// A request on a byte path carrying a pass, which goes on only once the pass service
/// hands the door the session that pass opens for the item its path names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Opening {
    /// Where the pass service is asked.
    pub(crate) at: String,
    /// The pass, as the request carried it.
    pub(crate) pass: String,
    /// The target forwarded, without the pass.
    pub(crate) target: String,
}

/// What a request is, once read against the declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Refused with this status, nothing asked.
    Refuse(u16),
    /// Goes on, once its check says it may.
    Forward(Box<Going>),
    /// Goes on as a request presenting the session its pass opens would.
    Open(Box<Opening>),
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
            .map(|pattern| any_case(pattern))
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
                    parameter: path.parameter.clone(),
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
        let forwarded = || forwarded(target, &pairs, self.put(bearer));
        let reading = READING.contains(&method);
        let covering = |live: &&Live| {
            either(&live.pattern)
                && live
                    .parameter
                    .as_deref()
                    .is_none_or(|parameter| carries(&pairs, parameter))
        };
        if let Some(live) = self.live.iter().find(covering) {
            if !reading {
                return Decision::Refuse(REFUSED);
            }
            return match self.lived(live, target, &decoded, &pairs, read) {
                Ok((check, carried)) => Decision::Forward(Box::new(Going {
                    target: forwarded(),
                    presenting,
                    check,
                    pass_service: Some(self.guarded.pass.clone()),
                    writing: Some(Writing {
                        carried,
                        stripped: self.token_parameters().map(str::to_owned).collect(),
                        session: None,
                    }),
                })),
                Err(status) => Decision::Refuse(status),
            };
        }
        if let Some(bytes) = self.bytes.iter().find(|bytes| either(&bytes.pattern)) {
            if bytes.exempt.iter().any(either) && !target.path.contains('%') {
                return Decision::Forward(Box::new(Going {
                    target: forwarded(),
                    presenting,
                    check: Check::Nothing,
                    pass_service: None,
                    writing: None,
                }));
            }
            if !reading {
                return Decision::Refuse(REFUSED);
            }
            let check = match self.asked(bytes, target, &pairs, read, None) {
                Ok(check) => check,
                Err(status) => return Decision::Refuse(status),
            };
            if !pairs
                .iter()
                .any(|pair| pair.name.eq_ignore_ascii_case(PASS))
            {
                return Decision::Forward(Box::new(Going {
                    target: forwarded(),
                    presenting,
                    check,
                    pass_service: None,
                    writing: None,
                }));
            }
            let presented =
                read.token.is_some() || !authorization.is_empty() || !headers.is_empty();
            return match (carried(&pairs), presented) {
                (Some(pass), false) => Decision::Open(Box::new(Opening {
                    at: self.guarded.pass.clone(),
                    pass: pass.to_owned(),
                    target: forwarded(),
                })),
                _ => Decision::Refuse(REFUSED),
            };
        }
        Decision::Forward(Box::new(Going {
            target: forwarded(),
            presenting,
            check: Check::Nothing,
            pass_service: None,
            writing: None,
        }))
    }

    /// What a request on `live`, whose path decoded once is `decoded`, is asked about
    /// besides its pass, and what is written into a playlist it is answered with; the
    /// status it is refused with where it does not go on. A live stream named in a query
    /// parameter is served on a route the upstream serves an item on too, so the item a
    /// byte path places in the request is asked about with the session it carries.
    fn lived(
        &self,
        live: &Live,
        target: Target<'_>,
        decoded: &str,
        pairs: &[Pair<'_>],
        read: Read<'_>,
    ) -> Result<(Check, Vec<(String, String)>), u16> {
        let either = |pattern: &Regex| pattern.is_match(target.path) || pattern.is_match(decoded);
        let own = |parameter: &str| {
            live.parameter
                .as_deref()
                .is_some_and(|own| own.eq_ignore_ascii_case(parameter))
        };
        let naming_another = self
            .bytes
            .iter()
            .filter(|bytes| either(&bytes.pattern))
            .flat_map(|bytes| &bytes.naming)
            .any(|(parameter, _)| !own(parameter) && carries(pairs, parameter));
        let written = Self::passable(live, target, pairs)
            .filter(|_| !naming_another)
            .ok_or(REFUSED)?;
        let Some(parameter) = &live.parameter else {
            return Ok((Check::Nothing, written));
        };
        let bytes = self
            .bytes
            .iter()
            .find(|bytes| either(&bytes.pattern))
            .filter(|bytes| !bytes.exempt.iter().any(either) && read.token.is_some())
            .ok_or(REFUSED)?;
        Ok((
            self.asked(bytes, target, pairs, read, Some(parameter))?,
            written,
        ))
    }

    /// What a request opened as `opening` says is, presenting `session`, the session its
    /// pass opens, in a header as a client presents a token there: asked about with it and
    /// forwarded presenting it, never in the query, where the upstream could write it into
    /// what it answers; refused where the session is not of the declared shape, the
    /// declaration names no header to present it in, or the item question would not be
    /// asked.
    pub(crate) fn opened(&self, method: &str, opening: &Opening, session: &str) -> Decision {
        let token = &self.guarded.declared.token;
        let header = match &token.presented {
            Presentation::AsIs => Some((AUTHORIZATION.to_owned(), format!("{BEARING}{session}"))),
            Presentation::Header { name, value } => {
                Some((name.clone(), value.replace(TOKEN, session)))
            }
            Presentation::Query { .. } => token
                .headers
                .first()
                .map(|name| (name.clone(), session.to_owned())),
        };
        let Some((name, value)) = header.filter(|_| self.shaped(session)) else {
            return Decision::Refuse(REFUSED);
        };
        let in_authorization = name.eq_ignore_ascii_case(AUTHORIZATION);
        let authorization: &[&[u8]] = if in_authorization {
            &[value.as_bytes()]
        } else {
            &[]
        };
        let headers = [(name.as_str(), value.as_bytes())];
        match self.decide(method, &opening.target, authorization, &headers) {
            Decision::Forward(mut going)
                if matches!(going.check, Check::Question { .. })
                    && going.presenting == Presenting::Unchanged =>
            {
                going.presenting = Presenting::Header { name, value };
                going.writing = Some(Writing {
                    carried: vec![(PASS.to_owned(), opening.pass.clone())],
                    stripped: self.token_parameters().map(str::to_owned).collect(),
                    session: Some(session.to_owned()),
                });
                Decision::Forward(going)
            }
            _ => Decision::Refuse(REFUSED),
        }
    }

    /// What a request on `live` writes into a playlist it is answered with, where it
    /// names a live stream of its declared shape, in its path or in its declared query
    /// parameter, and carries its pass once, in its one spelling, held to the bytes a pass
    /// is written in: that pass, and the parameter naming the live stream as it came.
    fn passable(
        live: &Live,
        target: Target<'_>,
        pairs: &[Pair<'_>],
    ) -> Option<Vec<(String, String)>> {
        let shaped = |stream: &str| live.stream.is_match(stream);
        let plain_names = !target.path.contains('%') && pairs.iter().all(|pair| plain(pair.name));
        let pass = carried(pairs).filter(|_| plain_names)?;
        let mut written = vec![(PASS.to_owned(), pass.to_owned())];
        match &live.parameter {
            None => live
                .pattern
                .captures(target.path)
                .and_then(|captured| captured.name(STREAM_NAME))
                .filter(|stream| shaped(stream.as_str()))
                .map(|_| written),
            Some(parameter) => {
                valued(parameter, pairs).filter(|stream| shaped(stream))?;
                let named = pairs.iter().find(|pair| pair.name == parameter)?;
                written.push((parameter.clone(), named.value?.to_owned()));
                Some(written)
            }
        }
    }
}

/// Whether `pairs` carry `parameter` under any spelling.
fn carries(pairs: &[Pair<'_>], parameter: &str) -> bool {
    pairs
        .iter()
        .any(|pair| pair.name.eq_ignore_ascii_case(parameter))
}

/// The value of the one parameter of `pairs` named `parameter` in any case, where it is
/// spelt as `parameter`, holds no `+`, which a reader decoding it as a form takes for a
/// space, and decoded once is text holding no `%`.
fn valued(parameter: &str, pairs: &[Pair<'_>]) -> Option<String> {
    let named: Vec<&Pair<'_>> = pairs
        .iter()
        .filter(|pair| pair.name.eq_ignore_ascii_case(parameter))
        .collect();
    let [pair] = named.as_slice() else {
        return None;
    };
    (pair.name == parameter)
        .then_some(pair.value)
        .flatten()
        .filter(|raw| !raw.contains('+'))
        .and_then(target::decoded_value)
        .filter(|value| !value.contains('%'))
}

/// The pass `pairs` carry once, in its one spelling, holding unreserved bytes alone and no
/// more than [`PASS_MOST`] of them.
fn carried<'a>(pairs: &[Pair<'a>]) -> Option<&'a str> {
    let passes: Vec<&Pair<'a>> = pairs
        .iter()
        .filter(|pair| pair.name.eq_ignore_ascii_case(PASS))
        .collect();
    let [pair] = passes.as_slice() else {
        return None;
    };
    (pair.name == PASS)
        .then_some(pair.value)
        .flatten()
        .filter(|value| {
            (1..=PASS_MOST).contains(&value.len()) && value.bytes().all(target::unreserved)
        })
}

/// What an answer to the question with `status`, carrying as `carrying` says, means:
/// `Ok` where the member may have the item, and the status the request is refused with
/// otherwise. The adapter's answer that its upstream could not be asked is a refusal,
/// whatever class the declaration puts it in.
pub(crate) fn answered(answers: &Answers, carrying: &Carrying, status: u16) -> Result<(), u16> {
    if matches!(carrying, Carrying::Token(_)) && status == UNASKED {
        return Err(REFUSED);
    }
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

/// The target a request goes on with: its path, and its query without a pass and with
/// the token put in the query where the declaration presents it there.
fn forwarded(target: Target<'_>, pairs: &[Pair<'_>], put: Option<(&str, &str)>) -> String {
    let passing = |pair: &&Pair<'_>| pair.name.eq_ignore_ascii_case(PASS);
    if put.is_none() && !pairs.iter().any(|pair| passing(&pair)) {
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
        .filter(|pair| !passing(pair))
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
/// start of a path and without regard to case, as a media server may route one.
fn from_start(pattern: &str) -> Result<Regex, Uncompiled> {
    bounded_as(&format!(r"\A(?:{pattern})"), pattern, true)
}

/// `pattern`, a path pattern, compiled within the bound a declaration is held to, to match
/// without regard to case, as a media server may route one.
fn any_case(pattern: &str) -> Result<Regex, Uncompiled> {
    bounded_as(pattern, pattern, true)
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

/// `built`, compiled within the bound a declaration is held to; where it does not
/// compile, the declared `pattern` named.
fn bounded(built: &str, pattern: &str) -> Result<Regex, Uncompiled> {
    bounded_as(built, pattern, false)
}

/// `built`, compiled as [`bounded`] compiles it, without regard to case where `any_case`
/// says.
fn bounded_as(built: &str, pattern: &str, any_case: bool) -> Result<Regex, Uncompiled> {
    RegexBuilder::new(built)
        .size_limit(COMPILED_MOST)
        .case_insensitive(any_case)
        .build()
        .map_err(|_| Uncompiled(pattern.to_owned()))
}

mod asking;
mod reading;

#[cfg(test)]
mod tests;
