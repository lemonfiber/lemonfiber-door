//! The question a request on a byte path is asked about: the item it names, in its path
//! or in a query value, and what the question carries, to the media server or to its
//! adapter.

use std::fmt::Write as _;

use lemonfiber_ports::service::{
    spelled_as_a_space, Addressee, Presentation, Question, SPACE as SPACE_PLACE, TOKEN,
};

use super::reading::Read;
use super::{
    encoded, Bytes, Carrying, Check, InQuery, Naming, Rules, AUTHORIZATION, BEARING, ITEM,
    ITEM_PLACE, REFUSED,
};
use crate::target::{self, unreserved, Pair, Target};

impl Rules {
    /// The question a request on `bytes` is asked about, or the status it is refused
    /// with.
    pub(super) fn asked(
        &self,
        bytes: &Bytes,
        target: Target<'_>,
        pairs: &[Pair<'_>],
        read: Read<'_>,
    ) -> Result<Check, u16> {
        let path = target.path;
        if path.contains('%') || pairs.iter().any(|pair| !super::plain(pair.name)) {
            return Err(REFUSED);
        }
        let item = match &bytes.query {
            None => bytes
                .pattern
                .captures(path)
                .and_then(|captured| captured.name(ITEM))
                .map(|matched| matched.as_str().to_owned()),
            Some(query) => in_query(query, pairs),
        }
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
                    items.extend(pair.value.map(str::to_owned));
                }
                _ => return Err(REFUSED),
            }
        }
        if items.iter().any(|item| !item.bytes().all(unreserved)) {
            return Err(REFUSED);
        }
        let question: &Question = self.guarded.declared.question.as_ref().ok_or(REFUSED)?;
        let (to, asked, carrying) = match question.to {
            Addressee::Upstream => (
                self.guarded.upstream.clone(),
                self.upstream_asked(question, &items, pairs, read.bearer),
                Carrying::Request,
            ),
            Addressee::Adapter => {
                let to = self.guarded.adapter.clone().ok_or(REFUSED)?;
                let space = bytes.space.as_deref().ok_or(REFUSED)?;
                let (asked, carrying) = self.adapter_asked(question, space, &items, read)?;
                (to, asked, carrying)
            }
        };
        Ok(Check::Question {
            to,
            method: question.method,
            asked,
            answers: question.answers.clone(),
            carrying,
        })
    }

    /// The question to the media server about each of `items`: the declared path naming
    /// it, carrying the token parameters the request carried and the token put in the
    /// query where the declaration presents it there.
    fn upstream_asked(
        &self,
        question: &Question,
        items: &[String],
        pairs: &[Pair<'_>],
        bearer: Option<&str>,
    ) -> Vec<String> {
        let tokens: Vec<&str> = pairs
            .iter()
            .filter(|pair| {
                self.token_parameters()
                    .any(|declared| declared == pair.name)
            })
            .map(|pair| pair.raw)
            .collect();
        let put = self.put(bearer);
        items
            .iter()
            .map(|item| {
                let mut asked = question.path.replace(ITEM_PLACE, item);
                let mut joiner = if asked.contains('?') { '&' } else { '?' };
                for raw in &tokens {
                    let _ = write!(asked, "{joiner}{raw}");
                    joiner = '&';
                }
                if let Some((parameter, value)) = put {
                    let _ = write!(asked, "{joiner}{parameter}={}", encoded(value));
                }
                asked
            })
            .collect()
    }

    /// The question to the adapter about each of `items` in `space`, and what it carries:
    /// the member's token alone, presented as the declaration presents it to the media
    /// server; the status the request is refused with where an item is not plain, or the
    /// request carries a value in its `Authorization` header or the header the token is
    /// presented in that is not the token.
    fn adapter_asked(
        &self,
        question: &Question,
        space: &str,
        items: &[String],
        read: Read<'_>,
    ) -> Result<(Vec<String>, Carrying), u16> {
        if read.unread || !spelled_as_a_space(space) {
            return Err(REFUSED);
        }
        let (header, query) = match (&self.guarded.declared.token.presented, read.token) {
            (_, None) => (None, None),
            (Presentation::AsIs, Some(token)) => (
                Some((AUTHORIZATION.to_owned(), format!("{BEARING}{token}"))),
                None,
            ),
            (Presentation::Header { name, value }, Some(token)) => {
                (Some((name.clone(), value.replace(TOKEN, token))), None)
            }
            (Presentation::Query { parameter }, Some(token)) => {
                (None, Some(format!("&{parameter}={}", encoded(token))))
            }
        };
        let asked = items
            .iter()
            .map(|item| {
                let mut asked = question
                    .path
                    .replace(SPACE_PLACE, space)
                    .replace(ITEM_PLACE, item);
                asked.push_str(query.as_deref().unwrap_or_default());
                asked
            })
            .collect();
        Ok((asked, Carrying::Token(header)))
    }
}

/// The item `query` finds in the one parameter of its spelling, decoded once, where the
/// value holds no `+`, which a reader decoding it as a form takes for a space, no `%`
/// once decoded, is matched whole by its pattern, and the item it holds is plain.
fn in_query(query: &InQuery, pairs: &[Pair<'_>]) -> Option<String> {
    let named: Vec<&Pair<'_>> = pairs
        .iter()
        .filter(|pair| pair.name.eq_ignore_ascii_case(&query.parameter))
        .collect();
    let [pair] = named.as_slice() else {
        return None;
    };
    let value = (pair.name == query.parameter)
        .then_some(pair.value)
        .flatten()
        .filter(|raw| !raw.contains('+'))
        .and_then(target::decoded_value)
        .filter(|value| !value.contains('%') && walkable(value))?;
    query
        .pattern
        .captures(&value)
        .and_then(|captured| captured.name(ITEM))
        .map(|item| item.as_str().to_owned())
        .filter(|item| item.bytes().all(unreserved))
}

/// Whether `value`, a query value decoded once, names a path every reader reaches alike:
/// past any scheme and authority it opens with, no `.` or `..` segment, no doubled slash,
/// no `;` and no backslash, and nowhere a fragment or a control character.
fn walkable(value: &str) -> bool {
    let path = value
        .split_once("://")
        .filter(|(scheme, _)| {
            !scheme.is_empty()
                && scheme
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
        })
        .map_or(value, |(_, rest)| {
            rest.find('/')
                .and_then(|at| rest.get(at..))
                .unwrap_or_default()
        });
    let path = path.split('?').next().unwrap_or_default();
    !value
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte == b'#')
        && !path.contains("//")
        && !path.contains([';', '\\'])
        && !path.split('/').any(|segment| matches!(segment, "." | ".."))
}
