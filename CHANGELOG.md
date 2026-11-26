# Changelog

## 0.6.0
- Now playing works with rekordbox and Serato DJ without changing any setting: the new
  "Automatic" source (the default) uses whichever program is open, rekordbox if both are,
  and follows you when you switch
- See your audience: while you're live, the console shows how many people are listening
  (all servers together) and the most there were during the set; each server shows its own
  count, and the mini window and the Decks view show the total. Works with Icecast and
  Shoutcast, no password needed
- No more wrong titles right after starting djcast: a deck that plays with its fader down
  no longer takes over the title while nothing can be heard
- Audio input "One program only": leave the name empty and djcast records your DJ program by
  itself, whichever one is open
- Long titles stay on one line, so the screen no longer jumps around; they end with "…" and
  scroll now and then to show the rest
- The label next to the title says rekordbox or Serato instead of "live detection"
- Fixed: saving the settings during a track listed that track twice in the tracklist
- Fixed: djcast kept checking rekordbox in the background after every settings change

## 0.5.0
- Now playing from Serato DJ 5 or newer. Serato doesn't say which deck is playing, so the
  title follows your controller's faders (MIDI); mixing with the mouse in Serato isn't
  supported. The decks show "Loaded", since Serato doesn't say whether they play
- The controller settings (MIDI input, mapping, learning the faders, crossfader) have their
  own tab in Settings: Controller (MIDI)
- Audio input "One program only": leave the name empty to record the DJ program the titles
  come from (rekordbox or Serato DJ)

## 0.4.0
- djcast for Mac (macOS 13 or newer): instant now playing from rekordbox and your
  controller's faders, like on Windows. djcast asks for the Accessibility permission it needs
  to read the rekordbox window, and opens that setting for you when it's missing
- Mac: AAC streaming, with the encoder built into macOS
- Mac: recording what the computer plays needs macOS 14.6 or newer; on older versions djcast
  says so and suggests your controller's audio input instead
- Now playing from a text file that another DJ program keeps up to date, such as the
  VirtualDJ tracklist. The last line is the current track; a time at the start of the line
  is ignored, and a file left over from an earlier set is not announced
- Controllers with several audio channels over USB: choose which pair to stream (Settings →
  Audio → Channels). For the DDJ-1000 the master is picked automatically; for others press
  Detect while a track plays. If the chosen pair stays silent while another one has sound,
  the console tells you and offers to switch
- Setup report (Settings → About): a summary of your devices, audio channels, MIDI and what
  djcast reads from rekordbox, without passwords, servers or track names, to send through
  the new "Setup report" form on GitHub
- The console shows how the sound is captured (the whole computer, an input or one program)
- Appearance "Like the system" instead of "Like Windows"
- The rekordbox history source and its database key are gone: the instant source gives the
  same titles without them. Settings that used it switch to the instant source by
  themselves; a key file saved earlier (rekordbox-db.key in the settings folder) can be
  deleted. For command-line users: `rekordbox set-key`, `info` and `history` are removed,
  `devices` shows the channel count and `meter --input --channel N` tests a channel pair

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
