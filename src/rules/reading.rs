//! A member's token read from every place the declaration names the media server reading
//! one from: the bearer header, the header the token is presented in, each token query
//! parameter and each token header. The same token in several of them is one token;
//! different tokens, one place carrying a token twice, another spelling and another shape
//! are refused, so the media server can act only as the token the door read.

use lemonfiber_ports::service::{Presentation, TOKEN};

use super::{bearer, Presenting, Rules, AUTHORIZATION, DECODED_OTHERWISE};
use crate::target::Pair;

/// A request's member token, as the door read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Read<'a> {
    /// The one token the request carries, wherever it carries it.
    pub(super) token: Option<&'a str>,
    /// The bearer token its `Authorization` header presents, of the declared shape.
    pub(super) bearer: Option<&'a str>,
    /// Whether it carries, in its `Authorization` header or the header the token is
    /// presented in, a value the door did not read as the token.
    pub(super) unread: bool,
}

impl Rules {
    /// Every header a member's token is read from beside `Authorization`: the one it is
    /// presented in and each the declaration names the media server also reading one
    /// from.
    pub(crate) fn token_headers(&self) -> impl Iterator<Item = &str> {
        let token = &self.guarded.declared.token;
        let presented = match &token.presented {
            Presentation::Header { name, .. } => Some(name.as_str()),
            Presentation::AsIs | Presentation::Query { .. } => None,
        };
        presented
            .into_iter()
            .chain(token.headers.iter().map(String::as_str))
    }

    /// The member's token the request carries, and how a bearer token is presented to
    /// the media server; nothing where it carries different tokens, one place a token
    /// twice, a token parameter under another spelling, or a token in another shape.
    pub(super) fn read<'a>(
        &self,
        authorization: &[&'a [u8]],
        pairs: &[Pair<'a>],
        headers: &[(&str, &'a [u8])],
    ) -> Option<(Read<'a>, Presenting)> {
        let token = &self.guarded.declared.token;
        let mut tokens = Vec::new();
        let mut named: Vec<&str> = Vec::new();
        for pair in pairs {
            let Some(declared) = self
                .token_parameters()
                .find(|declared| declared.eq_ignore_ascii_case(pair.name))
            else {
                continue;
            };
            let value = pair
                .value
                .filter(|value| self.shaped(value) && !value.contains(DECODED_OTHERWISE))?;
            if declared != pair.name || named.contains(&pair.name) {
                return None;
            }
            named.push(pair.name);
            tokens.push(value);
        }
        let bearer = match authorization {
            [] => None,
            [one] => bearer(one).filter(|value| self.shaped(value)),
            _ => return None,
        };
        tokens.extend(bearer);
        for name in &token.headers {
            match values(headers, name).as_slice() {
                [] => {}
                [one] => tokens.push(
                    std::str::from_utf8(one)
                        .ok()
                        .filter(|value| self.shaped(value))?,
                ),
                _ => return None,
            }
        }
        let in_authorization = matches!(
            &token.presented,
            Presentation::Header { name, .. } if name.eq_ignore_ascii_case(AUTHORIZATION)
        );
        let mut unread = !authorization.is_empty() && bearer.is_none() && !in_authorization;
        if let Presentation::Header { name, value } = &token.presented {
            if bearer.is_none() || !name.eq_ignore_ascii_case(AUTHORIZATION) {
                match values(headers, name).as_slice() {
                    [] => {}
                    [one] => match self.unwrapped(one, value) {
                        Some(read) => tokens.push(read),
                        None => unread = true,
                    },
                    _ => return None,
                }
            }
        }
        tokens.sort_unstable();
        tokens.dedup();
        if tokens.len() > 1 {
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
        let read = Read {
            token: tokens.first().copied(),
            bearer,
            unread,
        };
        Some((read, presenting))
    }

    /// The token `header` carries, presented as `template` says, where it holds one of
    /// the declared shape.
    fn unwrapped<'a>(&self, header: &'a [u8], template: &str) -> Option<&'a str> {
        let (before, after) = template.split_once(TOKEN)?;
        std::str::from_utf8(header)
            .ok()?
            .strip_prefix(before)?
            .strip_suffix(after)
            .filter(|token| self.shaped(token))
    }

    /// Whether `value` is a token of the declared shape that every reader reads as one: no
    /// comma a list of values is split at, and no space or other control a reader trims.
    fn shaped(&self, value: &str) -> bool {
        self.token.is_match(value)
            && !value
                .bytes()
                .any(|byte| byte == b',' || byte.is_ascii_whitespace() || byte.is_ascii_control())
    }

    /// Every query parameter a member's token reaches the media server in: those the
    /// declaration says it also reads one from, and the one it is presented in.
    pub(super) fn token_parameters(&self) -> impl Iterator<Item = &str> {
        let token = &self.guarded.declared.token;
        let presented = match &token.presented {
            Presentation::Query { parameter } => Some(parameter.as_str()),
            Presentation::AsIs | Presentation::Header { .. } => None,
        };
        token.parameters.iter().map(String::as_str).chain(presented)
    }

    /// The query parameter a bearer token is put in, with it, where the declaration
    /// presents a token in the query.
    pub(super) fn put<'a>(&'a self, bearer: Option<&'a str>) -> Option<(&'a str, &'a str)> {
        match (&self.guarded.declared.token.presented, bearer) {
            (Presentation::Query { parameter }, Some(value)) => Some((parameter.as_str(), value)),
            _ => None,
        }
    }
}

/// The values `headers` holds under `name`, matched without regard to case.
fn values<'a>(headers: &[(&str, &'a [u8])], name: &str) -> Vec<&'a [u8]> {
    headers
        .iter()
        .filter(|(held, _)| held.eq_ignore_ascii_case(name))
        .map(|(_, value)| *value)
        .collect()
}
