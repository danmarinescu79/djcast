# Roadmap

## Stage 1: streaming core ✅
- [x] Rust workspace (core / nowplaying / cli)
- [x] cpal capture: WASAPI loopback + input, stereo conversion, gain, VU meter
- [x] MP3 encoder (LAME), 32–320 kbps
- [x] Icecast client: PUT + SOURCE fallback, ice-* headers, `updinfo` metadata
- [x] Automatic reconnect, silence on capture gaps, local recording
- [x] CLI: `devices`, `init`, `stream`, `meta`
- [ ] Test against a real Icecast server

## Stage 2: now playing from `master.db` ✅
- [x] Auto-detect DB path (`rekordboxAgent/storage/options.json` → `db-path`)
- [x] Pure-Rust SQLCipher 4 decryption in memory (+ committed WAL frames), no OpenSSL
- [x] Poll `djmdSongHistory` only when master.db / WAL change
- [x] Wire source → `Engine::set_title`, text files for OBS, timestamped tracklist
- [x] CLI: `rekordbox set-key | info | history | watch`
- [x] Live test while rekordbox 7.2.20 is running: files readable while open, new history
      arrives only in `master.db-wal` and is picked up correctly

## Stage 3: instant now playing
- [ ] Read rekordbox process memory (per-version offsets in an updatable file)
- [ ] "On-air deck" logic (faders + crossfader, hysteresis, confirmation delay)
- [ ] MIDI from the DDJ-1000 (if the port can be shared)

## Capture sources (ongoing)
- [x] WASAPI loopback of any output device (verified: DDJ-1000 with rekordbox on WASAPI)
- [x] Input devices (line-in, virtual cables)
- [ ] Guide + test: controller on ASIO, rekordbox "PC master out" to a spare output, loopback that
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
