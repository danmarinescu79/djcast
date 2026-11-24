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
capture f32 → stereo + gain  ──200──> LAME MP3       ──┬──64──> SourceConnection (PUT/SOURCE)
     │                               + silence on gaps │        automatic reconnect
     └─> Levels (atomics, VU meter)                    └──────> recording file (optional)

"metadata" thread: title → GET /admin/metadata (re-sent automatically after a reconnect)
```

Principles:
- **The audio thread never blocks** (`try_send`). If the encoder falls behind, buffers are dropped instead of causing glitches in rekordbox.
- **Live means no accumulated delay.** If the network is slow or down, encoded data is dropped, not queued.
- **Silence on capture gaps.** WASAPI loopback delivers nothing while the output is silent, and Icecast drops a source after ~10 s without data. The encoder inserts silence once no audio has arrived for 200 ms. It only reacts to real gaps, so clock drift never inserts silence mid-audio.
- **No HTTP dependency.** The Icecast source protocol is implemented directly on `TcpStream`, with a fallback to the legacy `SOURCE` method for old servers.

## Capture
- `mode = "loopback"`: any output device (Windows/WASAPI). Captures exactly what rekordbox plays, no virtual cable needed.
- `mode = "input"`: line-in, USB input from a mixer, a virtual cable.
- macOS: loopback requires BlackHole or ScreenCaptureKit (not implemented yet).
