# Architecture

```
crates/
  core/        djcast-core: audio capture, encoding, Icecast client, engine
  nowplaying/  djcast-nowplaying: now-playing sources (rekordbox DB, memory, MIDI, Pro DJ Link)
  cli/         djcast: command-line app (a Tauri GUI will come later)
docs/          research, architecture, roadmap
```

## Data flow (core)

```
cpal (audio thread)                  "encoder" thread               "network" thread
capture f32 → stereo + gain  ──200──> AGC + limiter → encoder(s)     ──┬──64──> SourceConnection (PUT/SOURCE)
     │                               + silence on gaps │        automatic reconnect
     └─> Levels (atomics, VU meter)                    └──────> recording file (optional)

"metadata" thread: title → GET /admin/metadata (re-sent automatically after a reconnect)
```

Principles:
- **The audio thread never blocks** (`try_send`). If the encoder falls behind, buffers are dropped instead of causing glitches in rekordbox.
- **Live means no accumulated delay.** If the network is slow or down, encoded data is dropped, not queued.
- **Silence on capture gaps.** WASAPI loopback delivers nothing while the output is silent, and Icecast drops a source after ~10 s without data. The encoder inserts silence once no audio has arrived for 200 ms. It only reacts to real gaps, so clock drift never inserts silence mid-audio.
- **One encoder per format.** Servers with the same codec and bitrate share an encoder.
  MP3 (LAME) and AAC (the OS encoder: Media Foundation on Windows, ADTS frames) get their
  titles through the server's admin API. Opus is sent in Ogg at 48 kHz (resampled with
  `rubato` when the device runs at another rate); a title change ends the current Ogg
  logical stream and starts a new one whose `OpusTags` carry the title (chained Ogg), which
  is how Icecast and players learn titles for Ogg. Servers differ: Icecast 2.5 reads
  `OpusTags` into its status page; Icecast-KH (2.4.0-kh15) passes them to listeners but
  does not read them, and has no admin title update for Ogg mounts. A server that connects mid-stream gets
  nothing until the next logical stream begins (the network thread asks the encoder for one),
  because Ogg listeners need the header pages first.
- **No HTTP dependency.** The Icecast source protocol is implemented directly on `TcpStream`, with a fallback to the legacy `SOURCE` method for old servers.

## Loudness processing
DJ mixes typically leave a lot of headroom (peaks around -8 dBFS, about -20..-26 LUFS), so
they sound quiet next to processed streams in players that do not normalize (e.g. Second
Life). `crates/core/src/dsp.rs` adds, before encoding:
- a slow AGC driven by short-term loudness (EBU R128) towards a target (radio = -13 LUFS),
  rising 1 dB/s, falling 3 dB/s, gated below -40 LUFS so pauses are not amplified;
- a stereo-linked look-ahead limiter (5 ms, ceiling -1 dBFS): sliding minimum of the
  required gain + moving average, applied to delayed audio, so peaks never exceed the ceiling.

## Capture
- `mode = "loopback"`: any output device (Windows/WASAPI). Captures exactly what rekordbox plays, no virtual cable needed.
- `mode = "input"`: line-in, USB input from a mixer, a virtual cable.
- macOS: loopback requires BlackHole or ScreenCaptureKit (not implemented yet).
- Use `djcast meter -d <device> [--input]` to check levels before going live.

### DJ controllers and ASIO (Windows)
When rekordbox drives a controller's sound card through **ASIO**, the device is held
exclusively: WASAPI loopback and the controller's inputs fail with
`AUDCLNT_E_DEVICE_IN_USE`. No other app (BUTT included) can capture it. Options:
1. Switch rekordbox's audio driver for the controller from ASIO to the WASAPI device
   (e.g. "Line (PIONEER DJ DDJ-1000)") and loopback that device. Verified with a DDJ-1000.
2. Send rekordbox's "PC master out" to an unused output (e.g. S/PDIF) and loopback it.
3. Cable the controller's booth/master out into a line input (captures the hardware mic too).
