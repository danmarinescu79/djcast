//! Turn the text elements of rekordbox's window into per-deck information.
//!
//! Each deck panel shows a small deck-number label ("1".."4") left of the track title,
//! the artist under the title, and a time row (`-03:37` remaining, `00:00` elapsed).
//! We identify decks by that label, so this works for any layout, including a 2-deck
//! layout where the controller switched a side to deck 3 or 4.

/// A text element with its screen rectangle.
#[derive(Debug, Clone, PartialEq)]
pub struct TextItem {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeckView {
    pub deck: u8,
    pub title: String,
    pub artist: String,
    /// Elapsed time text ("mm:ss"); changes every second while the deck plays.
    pub elapsed: Option<String>,
    /// Remaining time text ("-mm:ss").
    pub remaining: Option<String>,
}

const LABEL_MAX_W: i32 = 24;
const SAME_ROW: i32 = 8;
const TITLE_MAX_GAP: i32 = 150;
const TITLE_MIN_W: i32 = 80;

pub fn parse_decks(items: &[TextItem]) -> Vec<DeckView> {
    let mut decks: Vec<DeckView> = Vec::new();
    for label in items.iter().filter(|i| is_deck_label(i)) {
        let deck: u8 = label.name.trim().parse().unwrap();
        // Title: wide text on the same row, just right of the label.
        let Some(title) = items
            .iter()
            .filter(|t| {
                (t.y - label.y).abs() <= SAME_ROW
                    && t.x > label.x
                    && t.x - (label.x + label.w) <= TITLE_MAX_GAP
                    && t.w >= TITLE_MIN_W
            })
            .min_by_key(|t| t.x)
        else {
            continue;
        };
        // Artist: left-aligned with the title, one row below.
        let artist = items
            .iter()
            .filter(|a| (a.x - title.x).abs() <= 4 && (15..=45).contains(&(a.y - title.y)))
            .min_by_key(|a| a.y)
            .map(|a| a.name.trim().to_string())
            .unwrap_or_default();
        // Time row: "-mm:ss" remaining followed by "mm:ss" elapsed, inside the panel.
        let in_time_row = |t: &&TextItem| {
            (10..=45).contains(&(t.y - title.y)) && t.x >= title.x && t.x <= title.x + title.w + 200
        };
        let remaining = items.iter().filter(in_time_row).filter(|t| is_time(&t.name, true)).min_by_key(|t| t.x);
        let elapsed = remaining.and_then(|r| {
            items
                .iter()
                .filter(in_time_row)
                .filter(|t| t.x > r.x && is_time(&t.name, false))
                .min_by_key(|t| t.x)
                .map(|t| t.name.trim().to_string())
        });
        if decks.iter().all(|d| d.deck != deck) {
            let remaining = remaining.map(|r| r.name.trim().to_string());
            decks.push(DeckView { deck, title: title.name.trim().to_string(), artist, elapsed, remaining });
        }
    }
    decks.sort_by_key(|d| d.deck);
    decks
}

fn is_deck_label(i: &TextItem) -> bool {
    let n = i.name.trim();
    i.w <= LABEL_MAX_W && matches!(n, "1" | "2" | "3" | "4")
}

/// `mm:ss` (or `-mm:ss` when `negative`).
fn is_time(s: &str, negative: bool) -> bool {
    let s = s.trim();
    let s = match (negative, s.strip_prefix('-')) {
        (true, Some(rest)) => rest,
        (false, None) => s,
        _ => return false,
    };
    let b = s.as_bytes();
    b.len() == 5 && b[2] == b':' && b.iter().enumerate().all(|(i, c)| i == 2 || c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(name: &str, y: i32, x: i32, w: i32) -> TextItem {
        TextItem { name: name.into(), x, y, w, h: 20 }
    }

    /// Captured from rekordbox 7.2.20, "2Deck Horizontal", left side switched to deck 3.
    fn sample() -> Vec<TextItem> {
        vec![
            t("Collection", 682, 48, 502),
            t("13:41", 66, 3775, 48),
            t("Miami (Rowshay Edit)", 281, 80, 1606),
            t("Will Smith", 308, 80, 125),
            t("108.01", 308, 224, 55),
            t("3A", 308, 285, 50),
            t("-03:37", 305, 1540, 58),
            t(".1", 305, 1598, 15),
            t("00:00", 305, 1612, 54),
            t(".0", 305, 1666, 15),
            t("Shut Up And Dance (Short Edit)", 281, 2001, 1606),
            t("Walk The Moon ", 308, 2001, 125),
            t("128.15", 308, 2145, 55),
            t("3B", 308, 2206, 50),
            t("-02:26", 305, 3461, 58),
            t(".1", 305, 3519, 15),
            t("01:12", 305, 3534, 54),
            t(".0", 305, 3588, 15),
            t("3", 279, 10, 15),
            t("2", 279, 1931, 15),
            t("MUTE", 411, 1611, 50),
            t("A", 452, 51, 14),
            t("00:00:00", 619, 2009, 62),
            t("03:00:00", 621, 2085, 50),
            // A browser row further down must not be mistaken for a deck.
            t("04:12", 760, 900, 40),
        ]
    }

    #[test]
    fn parses_two_deck_layout_with_switched_deck() {
        let d = parse_decks(&sample());
        assert_eq!(
            d,
            vec![
                DeckView {
                    deck: 2,
                    title: "Shut Up And Dance (Short Edit)".into(),
                    artist: "Walk The Moon".into(),
                    elapsed: Some("01:12".into()),
                    remaining: Some("-02:26".into()),
                },
                DeckView {
                    deck: 3,
                    title: "Miami (Rowshay Edit)".into(),
                    artist: "Will Smith".into(),
                    elapsed: Some("00:00".into()),
                    remaining: Some("-03:37".into()),
                },
            ]
        );
    }

    #[test]
    fn time_format() {
        assert!(is_time("-03:37", true));
        assert!(is_time("00:00", false));
        assert!(!is_time("00:00", true));
        assert!(!is_time("13:41:00", false));
        assert!(!is_time("108.01", false));
    }
}
