# Research: detecting "now playing" in rekordbox

*Date: 2026-11-24. Reference setup: rekordbox 7 on Windows, DDJ-1000.*

rekordbox has **no official API** for the current track (confirmed by Pioneer DJ on their forum). Every existing tool uses one of the methods below.

## Methods

| # | Method | Latency | Robustness | Platforms | Existing projects |
|---|---|---|---|---|---|
| 1 | History in `master.db` (SQLCipher-encrypted SQLite) | ~1 min (rekordbox writes history after ~60 s of playback) | Stable across versions | Win + Mac | supbox, legend, Now Playing app |
| 2 | Reading rekordbox process memory (title/artist/play state per deck) | Instant | Fragile: offsets change with every rekordbox update | Win (Mac possible, harder) | rkbx_link (**Rust**), rkbx_os2l, RekordboxReader |
| 3 | UI Automation / accessibility tree | Low | Medium: breaks on UI layout changes | Win + Mac | PonAir |
| 4 | MIDI from the controller (faders, crossfader, play) | Instant | Good | All | rekordbox-nowplaying-mac (has a DDJ-1000 profile) |
| 5 | Pro DJ Link (network, CDJ/DJM) | Instant | Very good, well-documented protocol (Deep Symmetry) | All | Prolink Tools, Beat Link Trigger |

### Details

**1. `master.db`**
- The location is configurable: `%APPDATA%\Pioneer\rekordbox6\rekordbox3.settings` → `masterDbDirectory`.
- Relevant tables: `djmdSongHistory` + `djmdHistory` → `djmdContent` / `djmdArtist`.
- In Rust: `rusqlite` with the `bundled-sqlcipher` feature.
- Open the database **read-only**, because rekordbox keeps it open.
- ⚠ `rekordbox3.settings` also holds the user's account tokens. Read only `masterDbDirectory`, never log anything else from that file.
- The decryption key is not shipped in this repository; it is obtained at runtime (see "Legal considerations").

**2. Process memory**
- rkbx_link reads position, BPM and beat at ~10 Hz, and title/artist/album per deck at a lower rate.
- Offsets are specific to each rekordbox version and kept in a separate, updatable file.
- Risks: needs an update for every rekordbox release, and may trigger antivirus heuristics.

**3. UI Automation**
- PonAir: on Windows, audio levels are not exposed, but titles and fader positions are.
- Supports only the horizontal 2-deck layout.

**4. MIDI: deciding which deck is on air**
- A *loaded* track is not necessarily the track the audience *hears*.
- Effective volume of a deck = channel fader × crossfader curve (equal-power). The loudest deck is "on air".
- Hysteresis (e.g. 0.55 / 0.45) keeps the title from flickering mid-transition.
- A confirmation delay (a few seconds) filters out short cuts.
- Windows caveat: rekordbox owns the controller's MIDI port. Options: a multi-client MIDI driver, or read fader state from memory or the UI instead.

**5. Pro DJ Link**
- UDP protocol (ports 50000–50002), documented by Deep Symmetry ("dysentery").
- Provides per-player status (playing, on-air flag from the DJM, track id) and metadata via the dbserver protocol.
- Rust parsers for PDB/ANLZ files exist (`rekordcrate`).

## Legal considerations
- "What's Now Playing" (open source) dropped rekordbox support, citing AlphaTheta's licensing terms.
- Methods 1–3 rely on undocumented internals. Similar open-source projects exist (pyrekordbox, rkbx_link, supbox), but this remains a grey area.
- To reduce risk, this project:
  - does not redistribute any rekordbox code, assets or keys;
  - opens all rekordbox files read-only;
  - is not affiliated with AlphaTheta / Pioneer DJ.
- MIDI and Pro DJ Link are the safest methods.

## Decision
Pluggable sources (the `Source` trait in `crates/nowplaying`):
1. `rekordbox-db`: implemented first (stable); remains the fallback.
2. `rekordbox-live`: deck contents via UI Automation + mixer state via MIDI (instant).
3. `prodjlink`: for CDJ setups (planned).

### Why UI Automation instead of process memory (2026-11)
- rkbx_link is GPL-3.0, and its memory offsets for current Windows rekordbox versions are
  sold under a commercial license; only 7.2.2 offsets are public. Offsets also break with
  every rekordbox update.
- rekordbox 7 is built with JUCE and exposes its UI through UI Automation: each deck panel
  shows a deck-number label, title, artist, BPM, key, remaining and elapsed time. One
  `FindAllBuildCache` call returns ~780 elements in a few hundred ms.
- The deck MASTER button state and the on-screen faders are *not* exposed (the mixer panel is
  hidden in the 2-deck layout with a controller), so the mixer state comes from MIDI.
- On Windows 11 the controller's MIDI input can be opened while rekordbox uses it.

### Pioneer DDJ MIDI mapping (verified on DDJ-1000)
| Control | Message |
|---|---|
| Channel fader N (1–4) | MIDI ch N, CC 0x13 (MSB) + 0x33 (LSB) |
| Crossfader | MIDI ch 7, CC 0x1F (MSB) + 0x3F (LSB) |
| Crossfader assign, channel N | MIDI ch N, note 0x16 = A, 0x1D = THRU, 0x18 = B |
| PLAY, deck N | MIDI ch N, note 0x0B |
| DECK select (1/3, 2/4) | note 0x72; active deck reported with note 0x3C |

## References
- https://github.com/gabek/supbox
- https://github.com/tombell/legend
- https://github.com/grufkork/rkbx_link
- https://github.com/fjel/rkbx_os2l
- https://github.com/Kethsar/RekordboxReader
- https://irz.fr/en/articles/ponair-rekordbox-accessibility-en/
- https://github.com/NaoIchinose/rekordbox-nowplaying-mac
- https://selector.news/2021/02/05/prolink-tools/
- https://www.nowplayingapp.com/
- https://whatsnowplaying.com/faq
- https://forums.pioneerdj.com/hc/en-us/community/posts/360059165691
