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
- [ ] Configurable MIDI mappings for other controllers

## Capture sources (ongoing)
- [x] WASAPI loopback of any output device (verified: DDJ-1000 with rekordbox on WASAPI)
- [x] Input devices (line-in, virtual cables)
- [x] Controller on ASIO + rekordbox "PC MASTER OUT" to a spare output (Realtek Digital Output), loopback that (verified with DDJ-1000)
- [ ] Process loopback: capture only rekordbox's audio (Windows 10 2004+ process loopback,
      macOS 14.2+ Core Audio taps). On Windows this only works when rekordbox uses WASAPI.
- [ ] Research: djcast virtual ASIO driver that forwards to the real device and taps the
      master (like ASIO Link Pro). Big effort; check Steinberg ASIO SDK licensing first.

## Stage 4: GUI (Tauri)
- [ ] Settings, server profiles, VU meter, Go Live button, status, track history
- [ ] Tray icon, autostart

## Stage 5: extensions
- [ ] Shoutcast v1/v2, TLS (HTTPS Icecast)
- [ ] Opus/Ogg (in-stream metadata), AAC
- [ ] Pro DJ Link (CDJs)
- [ ] macOS (loopback, builds, code signing)
- [ ] Tracklist export (Mixcloud/SoundCloud), webhooks (Discord)
- [ ] CI (GitHub Actions: build + test on Windows and macOS), release binaries
