# djcast

Audio streaming for DJs, in the spirit of BUTT, with automatic "now playing" detection from rekordbox.
Written in Rust, targeting Windows and macOS (the platforms rekordbox runs on).

> **Status:** early development. Streaming and instant now playing from rekordbox work on Windows; macOS is in progress. See the [roadmap](docs/roadmap.md).
> The name "djcast" is provisional.

## Features
- Capture what your DJ software plays (WASAPI loopback, no virtual cable needed) or any input device
- MP3, AAC and Opus, chosen per server: MP3 (LAME, 32–320 kbps) for every player,
  AAC-LC (the encoder built into Windows or macOS, 96–192 kbps) and Opus in Ogg (Icecast, titles inside the stream)
- Icecast 2 streaming (HTTP PUT with fallback to the legacy SOURCE method)
- Now-playing metadata, re-sent automatically after reconnects
- Automatic reconnect, silence padding so the server never drops you
- Optional local recording of your set
- Desktop app (Tauri): console and decks views, mini window, tray icon, server profiles
  (Icecast, Shoutcast v1/v2), passwords kept in Windows Credential Manager / macOS Keychain
- Instant now playing from rekordbox: the decks as shown in its window, plus your channel
  faders and crossfader over MIDI to pick the deck that is on air (no database key needed)
- Now playing from Serato DJ 5+ (the tracks loaded on its decks, plus your faders over MIDI)
- Now playing from a text file another DJ program keeps up to date (VirtualDJ and others)
- `nowplaying.txt` / `artist.txt` / `title.txt` for OBS, plus a timestamped tracklist per session

## Download
Website: https://danmarinescu79.github.io/djcast/

Installers are on the [releases page](https://github.com/danmarinescu79/djcast/releases/latest).
The app updates itself from there.

- **Windows 10/11 (64-bit):** `djcast_<version>_x64-setup.exe`, or the `.msi`. The installers
  are not code-signed yet, so Windows SmartScreen may say *Windows protected your PC* on first
  run: choose *More info → Run anyway*.
- **macOS 13 or newer (Apple Silicon and Intel):** `djcast_<version>_universal.dmg`. Open it,
  drag djcast to Applications, and allow it under System Settings → Privacy & Security →
  Accessibility when it asks (that is how it reads the decks). The app is signed with an Apple
  Developer ID and notarized.

Everything is built only by GitHub Actions from this repository, and the app verifies the
signature of every update it installs (see the [code signing policy](docs/code-signing-policy.md)).

## Build

Requires Rust (`rustup`) and CMake (for Opus). On Windows, also the Visual Studio Build Tools
(MSVC); on macOS, Xcode (`brew install cmake` for CMake).

```sh
cargo build --release
```

Desktop app (needs Node.js 22 too):

```sh
cd apps/desktop
npm install
npx tauri dev     # run in development
npx tauri build   # installers in target/release/bundle (Windows: msi, nsis; macOS: app, dmg)
```

## Usage

```sh
djcast devices                   # list audio devices
djcast init                      # create djcast.toml with defaults
djcast stream -t "Intro"         # start streaming
djcast meta "Artist - Title"     # send a title manually

djcast rekordbox watch           # print tracks as rekordbox plays them (no streaming)
```

While streaming:
- type a title and press Enter to send it as "now playing";
- type `q` and press Enter, or press Ctrl+C, to stop.

Example configuration: [djcast.example.toml](djcast.example.toml).

## Translations
The desktop app is available in English, Romanian, German, Spanish, French, Italian, Dutch,
Polish, Portuguese (Brazil), Turkish, Russian, Japanese and Chinese (Simplified); the language
follows the system or can be chosen in Settings → General.

Except English and Romanian, the translations have not yet been reviewed by native speakers —
corrections are very welcome (edit `apps/desktop/src/locales/<code>.js` and open a pull request).

To add a language:
1. copy `apps/desktop/src/locales/en.js` to `<code>.js` (e.g. `de.js`) and translate the values,
   keeping the `{placeholders}`;
2. register it in `apps/desktop/src/lib/i18n.svelte.js` (`LANGUAGES` and `DICTS`);
3. add the tray menu texts in `apps/desktop/src-tauri/src/tray.rs`;
4. run `npm run check-i18n` in `apps/desktop` — it reports missing keys and placeholder mismatches.

## Documentation
- [Now-playing research](docs/research-now-playing.md)
- [Architecture](docs/architecture.md)
- [Roadmap](docs/roadmap.md)
- [Releasing](docs/releasing.md)
- [Code signing policy](docs/code-signing-policy.md)
- [Changelog](CHANGELOG.md)
- [Contributing](CONTRIBUTING.md) · [Security policy](SECURITY.md) · [Code of conduct](CODE_OF_CONDUCT.md)

## Privacy
djcast has no accounts, analytics or telemetry. It only connects to:
- the streaming servers you add (Icecast / Shoutcast), to send your audio and track titles;
- GitHub, to check for and download updates (Settings → General; can be turned off);
- Discord, only if you add a webhook in Settings → Sharing, to post the messages you set up.

What djcast reads from rekordbox, MIDI and audio is used on your computer and never uploaded. Server
passwords and the Discord webhook are kept in the system credential store (Windows
Credential Manager / macOS Keychain). Logs stay in the settings folder and are only shared
if you attach them to a problem report yourself.

The [website](https://danmarinescu79.github.io/djcast/) counts visits and download clicks with
[GoatCounter](https://www.goatcounter.com), without cookies or personal data.

## Disclaimer
This project is not affiliated with, endorsed by, or connected to AlphaTheta Corporation or Pioneer DJ.
"rekordbox" is a trademark of AlphaTheta Corporation. djcast only reads what rekordbox shows in its
window (through the operating system's accessibility interface) and does not modify or redistribute
any part of rekordbox.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

If you build on djcast, a link back to the project is appreciated (see `NOTICE`).

The MP3 encoder links [LAME](https://lame.sourceforge.io/) (LGPL-2.0) via the `mp3lame-encoder` crate.
Opus uses [libopus](https://opus-codec.org/) (BSD-3-Clause). AAC uses the encoder that comes with the
operating system (Media Foundation on Windows, AudioToolbox on macOS); djcast ships no AAC code.

### Contribution
See [CONTRIBUTING.md](CONTRIBUTING.md) for how to report problems and send changes.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work,
as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
