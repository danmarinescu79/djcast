# djcast

Audio streaming for DJs, in the spirit of BUTT, with automatic "now playing" detection from rekordbox.
Written in Rust, targeting Windows and macOS (the platforms rekordbox runs on).

> **Status:** early development. The streaming core works; rekordbox detection is in progress. See the [roadmap](docs/roadmap.md).
> The name "djcast" is provisional.

## Features
- Capture what your DJ software plays (WASAPI loopback, no virtual cable needed) or any input device
- MP3 encoding (32–320 kbps)
- Icecast 2 streaming (HTTP PUT with fallback to the legacy SOURCE method)
- Now-playing metadata, re-sent automatically after reconnects
- Automatic reconnect, silence padding so the server never drops you
- Optional local recording of your set

## Build

Requires Rust (`rustup`). On Windows, also the Visual Studio Build Tools (MSVC).

```sh
cargo build --release
```

## Usage

```sh
djcast devices                   # list audio devices
djcast init                      # create djcast.toml with defaults
djcast stream -t "Intro"         # start streaming
djcast meta "Artist - Title"     # send a title manually
```

While streaming:
- type a title and press Enter to send it as "now playing";
- type `q` and press Enter, or press Ctrl+C, to stop.

Example configuration: [djcast.example.toml](djcast.example.toml).

## Documentation
- [Now-playing research](docs/research-now-playing.md)
- [Architecture](docs/architecture.md)
- [Roadmap](docs/roadmap.md)

## Disclaimer
This project is not affiliated with, endorsed by, or connected to AlphaTheta Corporation or Pioneer DJ.
"rekordbox" is a trademark of AlphaTheta Corporation. djcast only reads local rekordbox files read-only
and does not modify or redistribute any part of rekordbox.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

The MP3 encoder links [LAME](https://lame.sourceforge.io/) (LGPL-2.0) via the `mp3lame-encoder` crate.

### Contribution
Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work,
as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
