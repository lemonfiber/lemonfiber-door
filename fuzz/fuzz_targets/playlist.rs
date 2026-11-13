#![no_main]

use arbitrary::Arbitrary;
use lemonfiber_door::playlist::{holds, is_media, is_playlist, written, Origin, LINES_MOST, MOST};
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct Input<'a> {
    playlist: &'a str,
    pass: &'a str,
    stream: Option<(&'a str, &'a str)>,
    stripped: Vec<&'a str>,
    session: &'a str,
    authority: Option<&'a str>,
    encrypted: bool,
    content_type: &'a str,
}

fuzz_target!(|input: Input<'_>| {
    let _ = is_playlist(input.content_type);
    let _ = is_media(input.content_type);
    let origin = Origin {
        scheme: if input.encrypted { "https" } else { "http" },
        authority: input.authority,
    };
    let mut carried = vec![("lfpass", input.pass)];
    carried.extend(input.stream);
    let Some(out) = written(input.playlist, &carried, &input.stripped, origin) else {
        return;
    };
    assert!(out.len() <= MOST);
    assert!(input.playlist.split_inclusive('\n').count() <= LINES_MOST);
    if !carried
        .iter()
        .any(|(name, value)| name.contains('\n') || value.contains('\n'))
    {
        assert_eq!(
            out.matches('\n').count(),
            input.playlist.matches('\n').count()
        );
    }
    let introduced = carried.iter().any(|(name, value)| {
        holds(name.as_bytes(), input.session) || holds(value.as_bytes(), input.session)
    });
    let token_shaped = !input.session.is_empty()
        && input
            .session
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric());
    if token_shaped && !introduced && !holds(input.playlist.as_bytes(), input.session) {
        assert!(!holds(out.as_bytes(), input.session));
    }
});
