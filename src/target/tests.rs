use super::{decoded, normal, pairs, split, Abnormal, Pair, Target};

fn held(raw: &str) -> Result<(), Abnormal> {
    normal(split(raw)?)
}

#[test]
fn a_target_splits_at_its_first_question_mark() {
    assert_eq!(
        split("/Videos/a/stream?static=true&x=?"),
        Ok(Target {
            path: "/Videos/a/stream",
            query: Some("static=true&x=?"),
        })
    );
    assert_eq!(
        split("/"),
        Ok(Target {
            path: "/",
            query: None
        })
    );
}

#[test]
fn only_a_path_from_the_root_without_a_fragment_is_a_target() {
    for raw in [
        "http://jellyfin/Videos",
        "*",
        "Videos/a",
        "",
        "/a#b",
        "/a?b#c",
    ] {
        assert_eq!(split(raw), Err(Abnormal::NotOrigin), "{raw}");
    }
}

#[test]
fn parameters_keep_their_order_their_bytes_and_whether_they_carry_a_value() {
    assert_eq!(
        pairs(Some("a=1&&b&c=x=y&")),
        vec![
            Pair {
                raw: "a=1",
                name: "a",
                value: Some("1")
            },
            Pair {
                raw: "b",
                name: "b",
                value: None
            },
            Pair {
                raw: "c=x=y",
                name: "c",
                value: Some("x=y")
            },
        ]
    );
    assert!(pairs(None).is_empty());
}

#[test]
fn a_path_in_the_normal_form_is_held() {
    for raw in [
        "/",
        "/Videos/0123456789abcdef0123456789abcdef/stream.mkv?static=true",
        "/Artists/AC%2FDC",
        "/Items/a/Images/Primary?tag=x&maxWidth=300",
        "/web/index.html",
        "/a/b:c@d!e$f&g'h(i)j*k+l,m=n~o_p-q.r",
        "/Users/x?searchTerm=%41",
    ] {
        assert_eq!(held(raw), Ok(()), "{raw}");
    }
}

#[test]
fn a_dot_segment_a_doubled_slash_a_parameter_or_an_encoded_backslash_is_refused() {
    for raw in [
        "/Videos/../Items",
        "/Videos/./a",
        "/Videos/..",
        "/.",
        "/Videos//a",
        "//Videos",
        "/Videos/a;b",
        "/Videos;/a",
        "/Videos/a%5cb",
        "/Videos/a%5Cb",
    ] {
        assert_eq!(held(raw), Err(Abnormal::Segment), "{raw}");
    }
}

#[test]
fn an_escape_of_an_unreserved_character_or_of_a_percent_is_refused() {
    for raw in [
        "/%56ideos/a",
        "/Videos/%2e%2e/Items",
        "/Videos/%2E",
        "/Videos/a%252fb",
        "/Videos/%7e",
        "/Videos/%5f",
        "/Videos/%2d",
        "/Videos/%30",
    ] {
        assert_eq!(held(raw), Err(Abnormal::NeedlessEscape), "{raw}");
    }
}

#[test]
fn an_escape_naming_no_byte_is_refused() {
    for raw in ["/Videos/a%", "/Videos/a%2", "/Videos/a%zz", "/Videos/a%2g"] {
        assert_eq!(held(raw), Err(Abnormal::BrokenEscape), "{raw}");
    }
}

#[test]
fn a_byte_a_path_is_not_made_of_is_refused() {
    for raw in [
        "/Videos/ö",
        "/Videos/a\"b",
        "/Videos/a<b",
        "/Videos/a`b",
        "/Videos/a^b",
        "/Videos/a\\b",
        "/Videos/a b",
        "/Videos/a{b}",
        "/Videos/a|b",
    ] {
        assert_eq!(held(raw), Err(Abnormal::Character), "{raw}");
    }
}

#[test]
fn a_query_parameter_whose_name_is_encoded_is_refused() {
    assert_eq!(held("/a?api%5Fkey=1"), Err(Abnormal::EncodedName));
    assert_eq!(held("/a?x=%41&y=%2F"), Ok(()));
}

#[test]
fn a_path_decodes_once() {
    assert_eq!(decoded("/Artists/AC%2FDC/Images"), "/Artists/AC/DC/Images");
    assert_eq!(decoded("/a%3fb%3Dc"), "/a?b=c");
    assert_eq!(decoded("/plain"), "/plain");
    assert_eq!(decoded("/a%"), "/a%");
    assert_eq!(decoded("/a%2"), "/a%2");
    assert_eq!(decoded("/a%zz"), "/a%zz");
    assert_eq!(decoded("/a%2525"), "/a%25");
}
