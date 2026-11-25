# Changelog

## Unreleased
- Now playing from a text file: follows a file another DJ program keeps up to date (such as
  the VirtualDJ tracklist), so titles work beyond rekordbox. The last line is the current
  track; a time at its start is ignored. A file left over from an earlier set is not announced
- The switch delay, controller and crossfader settings only show for the rekordbox sources

## 0.3.1
- Now playing no longer repeats titles: a title is sent only when its text changes, and a
  short pause in reading rekordbox (common while it is busy) keeps the current track for up to
  30 s instead of switching to the play history, which lags a track behind
- Reading rekordbox is much lighter: only the text of the deck panels (~95 elements instead
  of ~770), which takes about 0.1 s instead of up to several seconds

## 0.3.0
- Formats per server: MP3, AAC (96–192 kbps, the encoder built into Windows) and Opus
  (Icecast only; titles travel inside the stream). Servers with the same format share one
  encoder; recordings use the main server's format (.mp3, .aac or .opus)
- Capture only one program, e.g. rekordbox: browser and system sounds stay out of the stream
  (Windows 10 2004+; works with a controller on ASIO when PC MASTER OUT goes to a Windows output)
- Other DJ controllers: learn the channel faders and crossfader by moving them (7- or 14-bit),
  reversed crossfader, crossfader side per channel
- Secure Icecast connections (TLS / HTTPS), certificate checked by Windows
- Tone controls: bass and presence, before the loudness processing
- Light, dark or "like Windows" appearance (Settings → General)
- Sharing tab: Discord messages when you go live, on every track and when you stop (with the
  tracklist); editable templates; webhook kept in the credential store
- Tracklist: Save… as a text file; a .cue sheet next to each recording
- Command line: `djcast meter --program rekordbox` to check the program capture

## 0.2.0
- Settings: General first, with start-up options (start with the computer, stay in the tray,
  close to the tray, go live automatically) and updates (check on start, check now, install
  and restart, what's new after an update)
- New About tab: version, project page, release notes, report a problem, logs and settings folders
- Log file in the settings folder (`logs/djcast.log`, rotated), for problem reports
- 13 UI languages: English, Romanian, German, Spanish, French, Italian, Dutch, Polish,
  Portuguese (Brazil), Turkish, Russian, Japanese, Chinese (Simplified)

## 0.1.1
- Stream to several servers at once (one encoder per bitrate, independent reconnects)
- Multiband processing (4 bands) between the AGC and the limiter, with band meters
- Track position on each deck (elapsed / remaining, last 30 s highlighted)
- New app icon; taller window; meters in the decks view and the mini window
- Icecast login is checked correctly with servers that send "100 Continue" first
- The window never freezes while connecting; server fields are validated

## 0.1.0
- First desktop version: console and decks views, mini window, tray icon
- Icecast and Shoutcast v1/v2, server profiles, passwords in the system credential store
- Loudness processing (AGC + limiter) with presets
- Now playing from rekordbox: instant (window + controller MIDI) with the history as fallback
- OBS text files and a timestamped tracklist
