//! X-Plane 11 plugin that shows the radio traffic on screen: a floating window with the last
//! controller and pilot transmissions and the PTT state.
//!
//! atc-client sends each console line as one UTF-8 datagram to `127.0.0.1:`[`PORT`]
//! (`"[Kraków Tower] …"`, `"[you] …"`, `"[ptt] transmitting on 118.105"`). The plugin polls
//! the socket from a flight loop; the command `atc/overlay/toggle` shows or hides the window.
//! Datagrams framed as vehicle commands (`atc_proto::OVERLAY_COMMAND_PREFIX`) show the
//! follow-me car ([`follow_me`]) instead. Everything here but [`plugin`] is plain text and
//! geometry, tested without X-Plane.

pub mod follow_me;
#[cfg(not(test))]
mod plugin;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// UDP port the overlay listens on (localhost only). Not 49010: X-Plane binds that for
/// RakNet after plugins load and refuses to network if it is taken.
pub const PORT: u16 = 48010;
/// Transmissions kept on screen.
const HISTORY: usize = 6;
/// Lines older than this are drawn dimmed.
const FRESH_S: f64 = 60.0;
/// A PTT status other than "transmitting" stays this long.
const STATUS_S: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// `[station] text`
    Atc,
    /// `[you] text`: what speech-to-text heard.
    Pilot,
    /// Connection and other client notices.
    Info,
    /// The PTT is pressed.
    Transmitting,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub kind: Kind,
    /// Drawn dimmed when false.
    pub fresh: bool,
    pub text: String,
}

struct Line {
    kind: Kind,
    text: String,
    at: f64,
}

/// What the window shows: the last [`HISTORY`] lines and the PTT status.
#[derive(Default)]
pub struct Board {
    lines: VecDeque<Line>,
    status: Option<Line>,
}

impl Board {
    /// One client line received at `now` (seconds).
    pub fn push(&mut self, raw: &str, now: f64) {
        let text = ascii_fold(raw.trim());
        if text.is_empty() {
            return;
        }
        if let Some(rest) = text.strip_prefix("[ptt] ") {
            let kind = if rest.starts_with("transmitting") {
                Kind::Transmitting
            } else {
                Kind::Info
            };
            let text = rest.to_string();
            self.status = Some(Line {
                kind,
                text,
                at: now,
            });
            return;
        }
        let kind = if text.starts_with("[you]") {
            Kind::Pilot
        } else if text.starts_with('[') {
            Kind::Atc
        } else {
            Kind::Info
        };
        self.lines.push_back(Line {
            kind,
            text,
            at: now,
        });
        if self.lines.len() > HISTORY {
            self.lines.pop_front();
        }
    }

    /// Rows top to bottom, wrapped at `width` characters; the PTT status, if any, last.
    pub fn rows(&self, now: f64, width: usize) -> Vec<Row> {
        let mut rows = Vec::new();
        let status = self
            .status
            .as_ref()
            .filter(|s| s.kind == Kind::Transmitting || now - s.at < STATUS_S);
        for l in self.lines.iter().chain(status) {
            let fresh = l.kind == Kind::Transmitting || now - l.at < FRESH_S;
            for text in wrap(&l.text, width) {
                rows.push(Row {
                    kind: l.kind,
                    fresh,
                    text,
                });
            }
        }
        rows
    }
}

/// Word wrap at `width` characters (at least 1); longer words are cut.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word = word;
        loop {
            let room = if line.is_empty() {
                width
            } else {
                width.saturating_sub(line.len() + 1)
            };
            if word.len() <= room {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
                break;
            }
            if line.is_empty() {
                let (head, tail) = word.split_at(width);
                out.push(head.to_string());
                word = tail;
            } else {
                out.push(std::mem::take(&mut line));
            }
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// The follow-me car's roof object (sign, beacon, turn signals) for the plugin binary at
/// `plugin_xpl` (`…/atc-overlay/64/lin.xpl`): `…/atc-overlay/objects/follow_me_roof.obj`,
/// where tools/install-xplane-plugin.sh puts it.
pub fn roof_object_path(plugin_xpl: &Path) -> PathBuf {
    let plugin_dir = plugin_xpl
        .parent()
        .and_then(Path::parent)
        .unwrap_or(Path::new(""));
    plugin_dir.join("objects").join("follow_me_roof.obj")
}

/// ASCII for the X-Plane font: Polish letters lose their diacritics, other non-ASCII
/// characters become `?`.
pub fn ascii_fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'ą' => 'a',
            'ć' => 'c',
            'ę' => 'e',
            'ł' => 'l',
            'ń' => 'n',
            'ó' => 'o',
            'ś' => 's',
            'ź' | 'ż' => 'z',
            'Ą' => 'A',
            'Ć' => 'C',
            'Ę' => 'E',
            'Ł' => 'L',
            'Ń' => 'N',
            'Ó' => 'O',
            'Ś' => 'S',
            'Ź' | 'Ż' => 'Z',
            '\t' => ' ',
            c if c.is_ascii() && !c.is_ascii_control() => c,
            _ => '?',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn texts(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn wrap_breaks_at_words_and_cuts_long_ones() {
        assert_eq!(
            wrap("cleared for take-off", 11),
            ["cleared for", "take-off"]
        );
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("a  b", 10), ["a b"]);
        assert!(wrap("   ", 10).is_empty());
    }

    #[test]
    fn fold_keeps_ascii() {
        assert_eq!(ascii_fold("[Kraków Tower] Łódź"), "[Krakow Tower] Lodz");
        assert_eq!(ascii_fold("a\tb→c"), "a b?c");
    }

    #[test]
    fn lines_are_classified_and_kept() {
        let mut b = Board::default();
        b.push("connected to the server", 0.0);
        b.push("[you] Kraków Ground, SP-ABC, request taxi", 1.0);
        b.push(
            "[Kraków Ground] Sierra Bravo Charlie, taxi to holding point",
            2.0,
        );
        let rows = b.rows(3.0, 100);
        let kinds: Vec<Kind> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, [Kind::Info, Kind::Pilot, Kind::Atc]);
        assert_eq!(rows[1].text, "[you] Krakow Ground, SP-ABC, request taxi");
        for i in 0..10 {
            b.push(&format!("[Kraków Tower] {i}"), 4.0 + f64::from(i));
        }
        let rows = b.rows(14.0, 100);
        assert_eq!(rows.len(), HISTORY);
        assert_eq!(rows[HISTORY - 1].text, "[Krakow Tower] 9");
    }

    #[test]
    fn old_lines_dim() {
        let mut b = Board::default();
        b.push("[Kraków Tower] wind calm", 0.0);
        assert!(b.rows(FRESH_S - 1.0, 100)[0].fresh);
        assert!(!b.rows(FRESH_S + 1.0, 100)[0].fresh);
    }

    #[test]
    fn ptt_status_is_the_last_row() {
        let mut b = Board::default();
        b.push("[Kraków Tower] hello", 0.0);
        b.push("[ptt] transmitting on 123.255", 1.0);
        let rows = b.rows(100.0, 100);
        assert_eq!(
            texts(&rows),
            ["[Krakow Tower] hello", "transmitting on 123.255"]
        );
        assert_eq!(rows[1].kind, Kind::Transmitting);
        assert!(rows[1].fresh);
        b.push("[ptt] off - sent 3.2 s of audio", 101.0);
        assert_eq!(b.rows(102.0, 100)[1].text, "off - sent 3.2 s of audio");
        assert_eq!(b.rows(101.0 + STATUS_S + 1.0, 100).len(), 1);
    }

    #[test]
    fn folds_polish_diacritics() {
        assert_eq!(
            ascii_fold("Kraków Wieża: łączność z Ground"),
            "Krakow Wieza: lacznosc z Ground"
        );
        assert_eq!(ascii_fold("Ä€"), "??");
        assert_eq!(ascii_fold("a\0b"), "a?b");
    }

    #[test]
    fn roof_object_path_is_next_to_the_plugin() {
        let xpl = Path::new("/xp/Resources/plugins/atc-overlay/64/lin.xpl");
        assert_eq!(
            roof_object_path(xpl),
            Path::new("/xp/Resources/plugins/atc-overlay/objects/follow_me_roof.obj")
        );
    }

    #[test]
    fn roof_object_is_obj8_with_its_texture() {
        let obj = include_str!("../assets/follow_me_roof.obj");
        assert!(obj.starts_with("I\n800\nOBJ\n"), "not an OBJ8 header");
        assert!(
            obj.lines()
                .any(|l| l.trim() == "TEXTURE follow_me_roof.png")
        );
        assert!(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets/follow_me_roof.png")
                .is_file()
        );
    }
}
