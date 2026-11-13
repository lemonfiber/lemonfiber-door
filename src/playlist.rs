//! A playlist served on a pass, with that pass written into every address it holds: each
//! line that is an address, and each quoted `URI` attribute of a tag. Nothing else of it
//! changes, so a player fetching its parts carries the pass unaltered.

use crate::rules::PASS;

/// The media types an HLS playlist is served as, matched without regard to case.
const TYPES: [&str; 3] = [
    "application/vnd.apple.mpegurl",
    "application/x-mpegurl",
    "audio/mpegurl",
];

/// The most of a playlist the door writes a pass into, in bytes.
pub(crate) const MOST: usize = 16 << 20;

/// The byte order mark a playlist may open with.
const BOM: char = '\u{feff}';

/// What a tag opens with; any other line opening with `#` is a comment.
const TAG: &str = "#EXT";

/// The attribute of a tag that holds an address.
const URI: &str = "URI";

/// Whether an answer of `content_type` is a playlist.
pub(crate) fn is_playlist(content_type: &str) -> bool {
    let essence = content_type.split(';').next().unwrap_or_default().trim();
    TYPES.iter().any(|kind| kind.eq_ignore_ascii_case(essence))
}

/// `playlist` with `pass` written into every address it holds.
pub(crate) fn written(playlist: &str, pass: &str) -> String {
    let (mark, lines) = playlist.strip_prefix(BOM).map_or(("", playlist), |rest| {
        (playlist.split_at(BOM.len_utf8()).0, rest)
    });
    lines
        .split_inclusive('\n')
        .fold(mark.to_owned(), |mut out, line| {
            let content = line.trim_end_matches(['\r', '\n']);
            let end = line.get(content.len()..).unwrap_or_default();
            let trimmed = content.trim();
            if trimmed.starts_with(TAG) {
                out.push_str(&tagged(content, pass));
            } else if trimmed.is_empty() || trimmed.starts_with('#') {
                out.push_str(content);
            } else {
                let before = content.len() - content.trim_start().len();
                let after = before + trimmed.len();
                out.push_str(content.get(..before).unwrap_or_default());
                out.push_str(&addressed(trimmed, pass));
                out.push_str(content.get(after..).unwrap_or_default());
            }
            out.push_str(end);
            out
        })
}

/// `line`, a tag, with `pass` written into each quoted `URI` attribute it holds.
fn tagged(line: &str, pass: &str) -> String {
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
                out.push_str(&addressed(inside, pass));
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

/// `address` with `pass` as its one pass, put last in its query and before any fragment,
/// every other byte of it kept.
fn addressed(address: &str, pass: &str) -> String {
    let (located, fragment) = address
        .find('#')
        .map_or((address, ""), |at| address.split_at(at));
    let (path, query) = located
        .split_once('?')
        .map_or((located, None), |(path, query)| (path, Some(query)));
    let mut kept = query
        .map(|query| {
            query
                .split('&')
                .filter(|pair| {
                    !pair
                        .split('=')
                        .next()
                        .unwrap_or_default()
                        .eq_ignore_ascii_case(PASS)
                })
                .collect::<Vec<_>>()
                .join("&")
        })
        .unwrap_or_default();
    if !kept.is_empty() && !kept.ends_with('&') {
        kept.push('&');
    }
    format!("{path}?{kept}{PASS}={pass}{fragment}")
}

#[cfg(test)]
mod tests;
