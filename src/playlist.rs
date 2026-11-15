//! A playlist served on a pass, with that pass, and on a live-stream pass the parameter
//! naming the live stream, written into every address it holds that leads back to the
//! door, and every parameter a member's token reaches the upstream in taken off it: each
//! line that is an address, and each quoted `URI` attribute of a tag, where it is a
//! relative reference or names the door's own origin. Nothing else of it changes, so a
//! player fetching its parts carries the pass unaltered, carries it nowhere else, and is
//! handed no token the upstream wrote into it.

use crate::target;

/// The media types an HLS playlist is served as, matched without regard to case.
const TYPES: [&str; 3] = [
    "application/vnd.apple.mpegurl",
    "application/x-mpegurl",
    "audio/mpegurl",
];

/// The most of a playlist the door writes a pass into, and the most it answers once
/// written, in bytes.
pub const MOST: usize = 16 << 20;

/// The media types whose bodies only media is served as, matched by their start without
/// regard to case, but for a type written in XML or JSON.
const MEDIA: [&str; 5] = [
    "video/",
    "audio/",
    "image/",
    "application/octet-stream",
    "application/mp4",
];

/// The suffixes of a media type whose body is text however it starts.
const TEXT_SUFFIXES: [&str; 2] = ["+xml", "+json"];

/// The most lines of a playlist the door writes a pass into.
pub const LINES_MOST: usize = 1 << 18;

/// The byte order mark a playlist may open with.
const BOM: char = '\u{feff}';

/// What a tag opens with; any other line opening with `#` is a comment.
const TAG: &str = "#EXT";

/// The attribute of a tag that holds an address.
const URI: &str = "URI";

/// The origin a request reached the door at: its scheme, and its authority where it
/// named one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Origin<'a> {
    /// `http` or `https`.
    pub scheme: &'a str,
    /// The host and port it was sent to.
    pub authority: Option<&'a str>,
}

/// Whether an answer of `content_type` is one only media is served as, which holds no text
/// a token could be written into.
#[must_use]
pub fn is_media(content_type: &str) -> bool {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    MEDIA.iter().any(|kind| essence.starts_with(kind))
        && !TEXT_SUFFIXES.iter().any(|suffix| essence.ends_with(suffix))
}

/// Whether `body` holds `secret` anywhere, matched without regard to case.
#[must_use]
pub fn holds(body: &[u8], secret: &str) -> bool {
    let secret = secret.as_bytes();
    secret.is_empty()
        || body
            .windows(secret.len())
            .any(|window| window.eq_ignore_ascii_case(secret))
}

/// Whether an answer of `content_type` is a playlist.
#[must_use]
pub fn is_playlist(content_type: &str) -> bool {
    let essence = content_type.split(';').next().unwrap_or_default().trim();
    TYPES.iter().any(|kind| kind.eq_ignore_ascii_case(essence))
}

/// `playlist` with each of `carried`, a parameter's name and value, written into, and
/// every parameter named in `stripped` taken off, every address it holds that leads back
/// to
/// `origin`; nothing where it holds more lines than [`LINES_MOST`] or would be longer
/// than [`MOST`] once written.
#[must_use]
pub fn written(
    playlist: &str,
    carried: &[(&str, &str)],
    stripped: &[&str],
    origin: Origin<'_>,
) -> Option<String> {
    let (mark, lines) = playlist.strip_prefix(BOM).map_or(("", playlist), |rest| {
        (playlist.split_at(BOM.len_utf8()).0, rest)
    });
    if lines.split_inclusive('\n').nth(LINES_MOST).is_some() {
        return None;
    }
    let address = |reference: &str| {
        if back_to(reference, origin) {
            addressed(reference, carried, stripped)
        } else {
            reference.to_owned()
        }
    };
    lines
        .split_inclusive('\n')
        .try_fold(mark.to_owned(), |mut out, line| {
            let content = line.trim_end_matches(['\r', '\n']);
            let end = line.get(content.len()..).unwrap_or_default();
            let trimmed = content.trim();
            if trimmed.starts_with(TAG) {
                out.push_str(&tagged(content, &address));
            } else if trimmed.is_empty() || trimmed.starts_with('#') {
                out.push_str(content);
            } else {
                let before = content.len() - content.trim_start().len();
                let after = before + trimmed.len();
                out.push_str(content.get(..before).unwrap_or_default());
                out.push_str(&address(trimmed));
                out.push_str(content.get(after..).unwrap_or_default());
            }
            out.push_str(end);
            (out.len() <= MOST).then_some(out)
        })
}

/// Whether `reference` leads back to `origin`: a relative reference, which names no
/// scheme and no authority, or an absolute one naming the scheme and the authority of
/// `origin`, each matched without regard to case.
fn back_to(reference: &str, origin: Origin<'_>) -> bool {
    let schemed = reference
        .find(':')
        .filter(|&at| {
            reference
                .get(..at)
                .is_some_and(|scheme| !scheme.contains(['/', '?', '#']))
        })
        .map(|at| reference.split_at(at));
    match schemed {
        None => !reference.starts_with("//"),
        Some((scheme, rest)) => {
            let authority = rest.strip_prefix("://").map(|rest| {
                rest.find(['/', '?', '#'])
                    .map_or(rest, |at| rest.split_at(at).0)
            });
            scheme.eq_ignore_ascii_case(origin.scheme)
                && authority
                    .zip(origin.authority)
                    .is_some_and(|(named, own)| named.eq_ignore_ascii_case(own))
        }
    }
}

/// `line`, a tag, with each quoted `URI` attribute it holds written as `address` writes
/// it.
fn tagged(line: &str, address: &impl Fn(&str) -> String) -> String {
    let Some((tag, attributes)) = line.split_once(':') else {
        return line.to_owned();
    };
    let mut out = format!("{tag}:");
    let mut rest = attributes;
    while let Some((name, value)) = rest.split_once('=') {
        out.push_str(name);
        out.push('=');
        let after = if let Some((inside, after)) = value
            .strip_prefix('"')
            .and_then(|quoted| quoted.split_once('"'))
        {
            out.push('"');
            if name == URI {
                out.push_str(&address(inside));
            } else {
                out.push_str(inside);
            }
            out.push('"');
            after
        } else if value.starts_with('"') {
            out.push_str(value);
            ""
        } else {
            let (held, after) = value.split_at(value.find(',').unwrap_or(value.len()));
            out.push_str(held);
            after
        };
        let (between, next) = after.split_at(after.find(',').map_or(after.len(), |at| at + 1));
        out.push_str(between);
        rest = next;
    }
    out.push_str(rest);
    out
}

/// `address` with each of `carried` as its one parameter of that name, put last in its
/// query in order and before any fragment, and no parameter named in `stripped`, each
/// name matched without regard to case once decoded; every other byte of it kept.
fn addressed(address: &str, carried: &[(&str, &str)], stripped: &[&str]) -> String {
    let (located, fragment) = address
        .find('#')
        .map_or((address, ""), |at| address.split_at(at));
    let (path, query) = located
        .split_once('?')
        .map_or((located, None), |(path, query)| (path, Some(query)));
    let written = |pair: &&str| {
        let name = target::decoded(pair.split('=').next().unwrap_or_default());
        carried
            .iter()
            .map(|(carried, _)| carried)
            .chain(stripped)
            .any(|named| named.eq_ignore_ascii_case(&name))
    };
    let mut kept = query
        .map(|query| {
            query
                .split('&')
                .filter(|pair| !written(pair))
                .collect::<Vec<_>>()
                .join("&")
        })
        .unwrap_or_default();
    for (name, value) in carried {
        if !kept.is_empty() && !kept.ends_with('&') {
            kept.push('&');
        }
        kept.push_str(name);
        kept.push('=');
        kept.push_str(value);
    }
    format!("{path}?{kept}{fragment}")
}

#[cfg(test)]
mod tests;
