# Contributing to djcast

Thanks for helping. Bug reports, setup reports, translations and code are all welcome.

## Questions, bugs and ideas
- **Questions and help** (setup, which controller or server works): [Discussions](https://github.com/danmarinescu79/djcast/discussions).
- **Bugs and feature requests**: [open an issue](https://github.com/danmarinescu79/djcast/issues/new/choose)
  with the matching form. For problems with audio, MIDI or now playing, attach the
  **setup report** (Settings → About → Setup report): it lists devices and what djcast reads
  from rekordbox, without passwords, servers or track names.
- **Security problems**: please don't open an issue; see [SECURITY.md](SECURITY.md).

## Translations
Corrections by native speakers are very welcome: edit `apps/desktop/src/locales/<code>.js`
and open a pull request. Keep the `{placeholders}` as they are and run `npm run check-i18n` in
`apps/desktop`. Adding a language is described in the [README](README.md#translations).

## Code
Build instructions are in the [README](README.md#build); [docs/architecture.md](docs/architecture.md)
explains how the crates fit together.

Before you open a pull request:
```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd apps/desktop && npm run check-i18n && npm run build
```
CI runs the same checks on every pull request (Windows).

Please also:
- keep pull requests small and about one thing; describe what changed and how you tested it
  (which OS, DJ software and controller, if it matters);
- add every new user-visible text to **all** locale files (English is fine as a placeholder in
  the others; `check-i18n` reports what is missing);
- note user-visible changes in [CHANGELOG.md](CHANGELOG.md) under `## Unreleased`;
- use LF line endings;
- never commit server passwords, webhooks, keys or other secrets, and nothing taken from
  rekordbox or other DJ software itself (keys, binaries, databases).

Changes to `.github/`, `tauri.conf.json` and the app capabilities need the maintainer's review
([CODEOWNERS](.github/CODEOWNERS)).

## License
Contributions are dual licensed under MIT and Apache-2.0, like the project (see the
[README](README.md#contribution)).

By taking part you agree to follow the [code of conduct](CODE_OF_CONDUCT.md).
