#![no_main]

use arbitrary::Arbitrary;
use lemonfiber_door::playlist::{is_playlist, written, Origin, LINES_MOST, MOST};
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct Input<'a> {
    playlist: &'a str,
    pass: &'a str,
    authority: Option<&'a str>,
    encrypted: bool,
    content_type: &'a str,
}

fuzz_target!(|input: Input<'_>| {
    let _ = is_playlist(input.content_type);
    let origin = Origin {
        scheme: if input.encrypted { "https" } else { "http" },
        authority: input.authority,
    };
    let Some(out) = written(input.playlist, input.pass, origin) else {
        return;
    };
    assert!(out.len() <= MOST);
    assert!(input.playlist.split_inclusive('\n').count() <= LINES_MOST);
    if !input.pass.contains('\n') {
        assert_eq!(
            out.matches('\n').count(),
            input.playlist.matches('\n').count()
        );
    }
});
