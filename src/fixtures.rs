//! Envelopes the tests apply: one shaped as Jellyfin's filler declares its bytes, and
//! the same with the token presented another way.

use lemonfiber_ports::http::Method;
use lemonfiber_ports::service::{
    Addressee, Answers, BytePath, Declared, Guarded, Held, LivePath, Locations, Naming, Ports,
    Presentation, Question, Statuses, Token,
};

/// Where the tests' media server is.
pub(crate) const UPSTREAM: &str = "http://jellyfin:8096";

/// The one proxy the tests' guard believes.
pub(crate) const PROXY: &str = "10.80.96.3";

/// Where the tests' guard asks about a pass.
pub(crate) const PASS: &str = "http://10.80.96.34:8090/pass";

/// An item's id as Jellyfin writes it.
pub(crate) const ITEM: &str = "0123456789abcdef0123456789abcdef";

/// Another item's id.
pub(crate) const OTHER: &str = "fedcba9876543210fedcba9876543210";

/// A member's token as Jellyfin issues it.
pub(crate) const TOKEN: &str = "abcdefabcdefabcdefabcdefabcdef12";

const PREFIXED: &str = "(?i)^(?:/emby|/mediabrowser)?";

const ITEM_ID: &str = "(?i)^[0-9a-f]{8}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{12}$";

const HEX: &str = "^[0-9a-f]{32}$";

fn refusing() -> Vec<Naming> {
    ["itemId", "LiveStreamId"]
        .map(|parameter| Naming {
            parameter: parameter.to_owned(),
            held: Held::Refused,
        })
        .to_vec()
}

fn byte_path(route: &str, exempt: Vec<String>, naming: Vec<Naming>) -> BytePath {
    BytePath {
        pattern: format!("{PREFIXED}/{route}"),
        item: ITEM_ID.to_owned(),
        exempt,
        naming,
    }
}

/// A declaration shaped as Jellyfin's filler declares it.
pub(crate) fn declared() -> Declared {
    let mut trickplay = refusing();
    trickplay.push(Naming {
        parameter: "MediaSourceId".to_owned(),
        held: Held::Asked {
            shape: HEX.to_owned(),
        },
    });
    Declared {
        bytes: vec![
            byte_path("Videos/(?P<item>[^/]+)/Trickplay(?:/|$)", Vec::new(), trickplay),
            byte_path(
                "(?:Videos|Audio|LiveTv/LiveStreamFiles|LiveTv/LiveRecordings)/(?P<item>[^/]+)(?:/|$)",
                vec![format!("{PREFIXED}/Videos/(?:ActiveEncodings|MergeVersions)$")],
                refusing(),
            ),
            byte_path(
                "Items/(?P<item>[^/]+)/(?:Images|Download|File)(?:/|$)",
                Vec::new(),
                refusing(),
            ),
        ],
        live: vec![LivePath {
            pattern: format!("{PREFIXED}/LiveTv/LiveStreamFiles/(?P<stream>[^/]+)/stream\\."),
            stream: HEX.to_owned(),
        }],
        refused: vec![
            format!("{PREFIXED}/(?:Artists|Genres|MusicGenres|Persons|Studios)/[^/]+/Images(?:/|$)"),
            format!("{PREFIXED}/Branding/Splashscreen/?$"),
        ],
        question: Some(Question {
            method: Method::Get,
            path: "/Items/{item}".to_owned(),
            to: Addressee::Upstream,
            answers: Answers {
                may: vec![Statuses { from: 200, to: 299 }],
                may_not: vec![Statuses::only(404)],
                no_session: vec![Statuses::only(401)],
                not_from_there: vec![Statuses::only(403)],
            },
        }),
        token: Token {
            presented: Presentation::Header {
                name: "Authorization".to_owned(),
                value: r#"MediaBrowser Token="{token}""#.to_owned(),
            },
            shape: HEX.to_owned(),
            parameters: vec!["ApiKey".to_owned(), "api_key".to_owned()],
        },
        locations: Locations {
            stream: "{base}/Videos/{item}/master.m3u8".to_owned(),
            poster: "{base}/Items/{item}/Images/Primary".to_owned(),
            backdrop: "{base}/Items/{item}/Images/Backdrop".to_owned(),
        },
        ports: Ports {
            upstream: 8096,
            encrypted: Some(8920),
        },
    }
}

/// The envelope the tests apply, around `declared`.
pub(crate) fn guarded(declared: Declared) -> Guarded {
    Guarded {
        declared,
        upstream: UPSTREAM.to_owned(),
        adapter: None,
        proxy: PROXY.to_owned(),
        pass: PASS.to_owned(),
    }
}

/// The envelope shaped as Jellyfin's, with its token presented as `presented`.
pub(crate) fn presenting(presented: Presentation) -> Guarded {
    let mut declared = declared();
    declared.token.presented = presented;
    guarded(declared)
}

/// A directory of its own for one test, removed after it.
pub(crate) struct Scratch(std::path::PathBuf);

impl Scratch {
    /// A new, empty one.
    pub(crate) fn new() -> Self {
        let mut bytes = [0_u8; 16];
        let _ = ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes);
        let path = std::env::temp_dir().join(format!(
            "door-{}-{:x}",
            std::process::id(),
            u128::from_le_bytes(bytes)
        ));
        let _ = std::fs::create_dir_all(&path);
        Self(path)
    }

    /// Where it is.
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
