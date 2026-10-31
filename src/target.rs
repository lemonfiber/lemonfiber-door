//! A request's target as its bytes arrived: its path and query, the path decoded once,
//! the query's parameters in the order they came, and the normal form every target is
//! held to before anything reads it.
//!
//! The normal form is the one every reader of the target agrees on: no dot segment, no
//! doubled slash, no `;` parameter, no encoded backslash, no percent-encoding of an
//! unreserved character or of `%`, no byte outside what a URL path is made of, and no
//! query parameter whose name holds a percent-encoding. A target in it decodes to one
//! path, so what the door checks and what the media server reads cannot differ but by
//! the reserved delimiters an encoding keeps.

/// A target split into its path and its query, both as their bytes arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Target<'a> {
    /// Everything before the first `?`.
    pub(crate) path: &'a str,
    /// Everything after it, where there is a `?`.
    pub(crate) query: Option<&'a str>,
}

/// One query parameter as it arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pair<'a> {
    /// The whole `name=value`, or the name alone.
    pub(crate) raw: &'a str,
    /// Its name.
    pub(crate) name: &'a str,
    /// Its value, where it carries an `=`.
    pub(crate) value: Option<&'a str>,
}

/// Why a target is not in the normal form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Abnormal {
    /// Not a path from the root, or carrying a fragment.
    NotOrigin,
    /// A `.` or `..` segment, a doubled slash, a `;` or an encoded backslash.
    Segment,
    /// A `%` not followed by two hexadecimal digits.
    BrokenEscape,
    /// A percent-encoding of an unreserved character or of `%` itself.
    NeedlessEscape,
    /// A byte a URL path is not made of.
    Character,
    /// A query parameter whose name holds a percent-encoding.
    EncodedName,
}

/// `raw` split into its path and query, where it is a path from the root carrying no
/// fragment.
pub(crate) fn split(raw: &str) -> Result<Target<'_>, Abnormal> {
    if !raw.starts_with('/') || raw.contains('#') {
        return Err(Abnormal::NotOrigin);
    }
    Ok(match raw.split_once('?') {
        Some((path, query)) => Target {
            path,
            query: Some(query),
        },
        None => Target {
            path: raw,
            query: None,
        },
    })
}

/// The parameters of `query`, in the order they came, leaving out empty ones.
pub(crate) fn pairs(query: Option<&str>) -> Vec<Pair<'_>> {
    query
        .unwrap_or_default()
        .split('&')
        .filter(|raw| !raw.is_empty())
        .map(|raw| match raw.split_once('=') {
            Some((name, value)) => Pair {
                raw,
                name,
                value: Some(value),
            },
            None => Pair {
                raw,
                name: raw,
                value: None,
            },
        })
        .collect()
}

/// `target` held to the normal form.
pub(crate) fn normal(target: Target<'_>) -> Result<(), Abnormal> {
    let path = target.path;
    if path.contains("//")
        || path.contains(';')
        || path
            .split('/')
            .any(|segment| segment == "." || segment == "..")
    {
        return Err(Abnormal::Segment);
    }
    if !path.bytes().all(pchar) {
        return Err(Abnormal::Character);
    }
    escapes(path)?;
    if pairs(target.query)
        .iter()
        .any(|pair| pair.name.contains('%'))
    {
        return Err(Abnormal::EncodedName);
    }
    Ok(())
}

/// `path` decoded once: every `%XX` the byte it names. A sequence that names no byte is
/// left as it came, which the normal form has refused before this is read.
pub(crate) fn decoded(path: &str) -> String {
    String::from_utf8_lossy(&unescaped(path)).into_owned()
}

/// `value`, a query value, decoded once, where what it decodes to is text: every `%XX`
/// the byte it names, and a sequence that names no byte left as it came.
pub(crate) fn decoded_value(value: &str) -> Option<String> {
    String::from_utf8(unescaped(value)).ok()
}

/// The bytes `text` names once every `%XX` in it is the byte it names.
fn unescaped(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        let escaped = (byte == b'%')
            .then(|| Some((hex(*bytes.get(at + 1)?)?, hex(*bytes.get(at + 2)?)?)))
            .flatten();
        if let Some((high, low)) = escaped {
            out.push(high << 4 | low);
            at += 3;
        } else {
            out.push(byte);
            at += 1;
        }
    }
    out
}

/// Every `%` in `path` an escape of a byte that needs one, and none of a backslash.
fn escapes(path: &str) -> Result<(), Abnormal> {
    let bytes = path.as_bytes();
    for (at, _) in path.match_indices('%') {
        let named = bytes
            .get(at + 1)
            .copied()
            .and_then(hex)
            .zip(bytes.get(at + 2).copied().and_then(hex))
            .map(|(high, low)| high << 4 | low)
            .ok_or(Abnormal::BrokenEscape)?;
        if named == b'\\' {
            return Err(Abnormal::Segment);
        }
        if unreserved(named) || named == b'%' {
            return Err(Abnormal::NeedlessEscape);
        }
    }
    Ok(())
}

/// The value of the hexadecimal digit `digit`.
fn hex(digit: u8) -> Option<u8> {
    char::from(digit)
        .to_digit(16)
        .and_then(|value| u8::try_from(value).ok())
}

/// Whether `byte` is unreserved: a letter, a digit, `-`, `.`, `_` or `~`.
pub(crate) const fn unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

/// Whether `byte` may stand in a path as it is: a path character, `/` or `%`.
const fn pchar(byte: u8) -> bool {
    unreserved(byte)
        || matches!(
            byte,
            b'!' | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
                | b'/'
                | b'%'
        )
}

#[cfg(test)]
mod tests;
