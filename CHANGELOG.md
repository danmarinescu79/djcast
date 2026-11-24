# Changelog

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
