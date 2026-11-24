# Roadmap

## Stage 1: streaming core ✅
- [x] Rust workspace (core / nowplaying / cli)
- [x] cpal capture: WASAPI loopback + input, stereo conversion, gain, VU meter
- [x] MP3 encoder (LAME), 32–320 kbps
- [x] Icecast client: PUT + SOURCE fallback, ice-* headers, `updinfo` metadata
- [x] Automatic reconnect, silence on capture gaps, local recording
- [x] CLI: `devices`, `init`, `stream`, `meta`
- [x] Tested against a real server (Icecast 2.4.0-kh15): audio, metadata, auto title changes

## Stage 2: now playing from `master.db` ✅
- [x] Auto-detect DB path (`rekordboxAgent/storage/options.json` → `db-path`)
- [x] Pure-Rust SQLCipher 4 decryption in memory (+ committed WAL frames), no OpenSSL
- [x] Poll `djmdSongHistory` only when master.db / WAL change
- [x] Wire source → `Engine::set_title`, text files for OBS, timestamped tracklist
- [x] CLI: `rekordbox set-key | info | history | watch`
- [x] Live test while rekordbox 7.2.20 is running: files readable while open, new history
      arrives only in `master.db-wal` and is picked up correctly

## Stage 3: instant now playing
- [x] Deck contents from the rekordbox window via Windows UI Automation (no process memory,
      no per-version offsets); decks identified by their number label, any layout
- [x] Playing state from the deck's running time display
- [x] MIDI mixer (Pioneer DDJ mapping, verified on DDJ-1000): channel faders, crossfader,
      crossfader assign; MIDI port shared with rekordbox (Windows 11 multi-client MIDI)
- [x] "On-air deck" logic: equal-power crossfader, hysteresis, confirmation delay
- [x] Sources `rekordbox-live` and `auto` (live, falling back to the history)
- [x] Live test with DDJ-1000 + rekordbox 7.2.20: fader moves switch the title after the
      4 s confirmation; short cuts and equal levels keep the current deck
- [ ] macOS: read the rekordbox window through the Accessibility API
- [x] Configurable MIDI mappings for other controllers: "learn" faders and crossfader
      (7/14-bit, reversed crossfader), crossfader assign per channel

## Loudness processing ✅
- [x] EBU R128 loudness metering (input/output short-term LUFS) in `meter` and the status line
- [x] Slow AGC towards a LUFS target (gated, so pauses are not pumped up)
- [x] Look-ahead stereo-linked peak limiter (5 ms, ceiling -1 dBFS) to protect the MP3 encoder
- [x] Presets off / gentle (-16) / radio (-13) / loud (-10) / custom
- [x] Listening test against other streams: sounds good with `radio`
- [x] Multiband processing: 4 bands (LR4 crossovers at 120 Hz / 600 Hz / 4 kHz with all-pass
      phase compensation, flat within 0.1 dB when idle), per-band stereo-linked compressors,
      between the AGC and the limiter; `multiband = true`, `meter --multiband`, band meters in the UI
- [x] Listening test of multiband on a real stream (sounds good with the defaults)
- [x] Optional bass/presence EQ (low shelf 100 Hz, bell 3.5 kHz, ±6 dB, before the AGC)

## Capture sources (ongoing)
- [x] WASAPI loopback of any output device (verified: DDJ-1000 with rekordbox on WASAPI)
- [x] Input devices (line-in, virtual cables)
- [x] Controller on ASIO + rekordbox "PC MASTER OUT" to a spare output (Realtek Digital Output), loopback that (verified with DDJ-1000)
- [x] Process loopback on Windows (10 2004+): capture only rekordbox's audio (and its child
      processes). Works with a controller on ASIO when PC MASTER OUT goes to a Windows output
      (verified with DDJ-1000)
- [ ] Process capture on macOS (14.2+ Core Audio taps)
- [ ] Research: djcast virtual ASIO driver that forwards to the real device and taps the
      master (like ASIO Link Pro). Big effort; check Steinberg ASIO SDK licensing first.

## Stage 4: GUI (Tauri)
- [x] Tauri v2 + Svelte 5 app sharing the Rust engine (`crates/app` Station service)
- [x] Console view: Go Live, stats, now playing, decks, tracklist, meters, LUFS/AGC/limiter
- [x] Settings: audio input, server profiles (Icecast / Shoutcast v1 / v2), connection test,
      show/hide password, processing presets, now-playing source + MIDI picker, folders with
      Browse… / Open
- [x] Decks view (mock-up B) and switching between views
- [x] Mini window (mock-up C), tray icon (Go Live/Stop, tooltip), hide to tray while live
- [x] Internationalization: 13 languages (en, ro, de, es, fr, it, nl, pl, pt-BR, tr, ru, ja, zh) in the UI and tray, language follows the system or
      Settings → General; `npm run check-i18n` validates keys and placeholders
- [ ] Native-speaker review of the translations (all except en/ro)
- [x] Settings → General: start with the computer, stay in the tray, close to tray, go live on start
- [x] Updates: Tauri updater (signed, GitHub releases), check on start / now, what's new after update
- [x] About tab; log file `logs/djcast.log` (rotated) next to the settings
- [x] Passwords in the OS credential store (Windows Credential Manager / macOS Keychain),
      plain-text passwords migrated automatically, removed with their profile
- [x] Windows installers: `npx tauri build` → .msi (WiX) and setup .exe (NSIS)
- [ ] Code signing to avoid the SmartScreen warning: applied to the SignPath Foundation
      program (2026-11-24); [policy](code-signing-policy.md) and privacy statement published
- [x] First public release: v0.2.0 on GitHub with installers, updater signature and latest.json
- [x] Project website on GitHub Pages (`site/`): screenshots, download of the latest
      installer, privacy and code signing notes

## Stage 5: extensions
- [x] Shoutcast v1 and v2 (legacy source protocol on port + 1, stream ID)
- [x] TLS (HTTPS Icecast) with rustls; certificates checked by the OS trust store
- [x] Stream to several servers at once: tick profiles as live; one encoder per format,
      independent connections, reconnects, titles and stats per server
- [x] Formats per server: MP3, AAC-LC (Windows Media Foundation, ADTS) and Opus in Ogg
      (resampled to 48 kHz; titles as chained Ogg streams; Icecast only); one encoder per format
- [ ] AAC on macOS (AudioToolbox)
- [ ] Pro DJ Link (CDJs) — low priority: needs CDJ hardware to test
- [ ] macOS (loopback, builds, code signing)
- [x] Tracklist: copy / save as text, CUE sheet next to recordings; Discord webhook posts (go live, each track, stop with tracklist), editable templates
- [x] CI (GitHub Actions, Windows) and automatic signed releases on `v*` tags, protected `release` environment
- [ ] CI and release builds for macOS
