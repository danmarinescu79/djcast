# Changelog

## 0.6.0
- Now playing "Automatic", the new default: djcast reads rekordbox or Serato DJ, whichever is
  running (rekordbox if both), and switches by itself when you change programs
- Capturing one program with no name: when the program the titles come from isn't running,
  the first DJ program that is gets captured; rekordbox's helper process is never picked
- Saving the settings no longer leaves the previous rekordbox reader running in the background
- Listeners: while live, the console shows how many people listen on all servers together and
  the session's peak, each server its own count, and the mini window the total. djcast reads
  the servers' public statistics every 20 s (Icecast status-json.xsl, Shoutcast 7.html or
  stats), no password needed; the Decks view shows them too
- While the captured audio is silent, the title doesn't move to another deck. Right after
  start djcast doesn't know the faders until they move, and a playing deck behind a closed
  fader could take over the title
- Saving the settings while a track plays no longer adds it to the tracklist a second time
- Long titles stay on one line (console, mini window, Decks) so the layout no longer jumps:
  they end in "…" and every few seconds scroll slowly to show the rest (not with reduced
  motion turned on in the system)
- Now playing shows the program's name (rekordbox, Serato) instead of "live detection", and
  the input "rekordbox (program audio)" without ".exe"

## 0.5.0
- Now playing from Serato DJ 5 and newer: djcast reads which track Serato has loaded on each
  deck (its library database, read-only). Serato doesn't say which deck is playing, so the
  title follows the controller's faders over MIDI; mixing with the mouse in Serato is not
  supported. The decks show "Loaded" instead of playing or stopped, since Serato doesn't tell
- Settings: the controller (MIDI input, mapping, learning the faders, crossfader sides, switch
  delay) has its own tab, Controller (MIDI); Now playing links to it
- Capturing one program with no name given is automatic: the program the titles come from
  (rekordbox or Serato DJ), otherwise the first DJ program running (rekordbox, Serato DJ,
  VirtualDJ, Traktor, djay). "rekordbox" now finds rekordbox itself, not its helper process
- About, the setup report and the README mention Serato DJ next to rekordbox

## 0.4.0
- macOS: instant now playing from the rekordbox window (Accessibility API) and the controller's
  faders over MIDI, like on Windows; djcast asks for the Accessibility permission it needs
- macOS: djcast runs on macOS 13 or newer; capturing an output (loopback) needs 14.6 or newer,
  and on older versions djcast says so and points to the controller's input
- macOS: AAC with the encoder built into macOS (AudioToolbox), constant bitrate, same ADTS
  stream as on Windows
- macOS: when djcast may not read the rekordbox window yet, the console says so and opens the
  Accessibility settings
- Settings → About → Setup report: lists the devices, which input channels have sound, MIDI and
  what djcast reads from rekordbox (no passwords, servers or track names), to copy or send
  through the new "Setup report" form on GitHub
- Appearance "Like the system" instead of "Like Windows"; on a Mac, program capture (Windows
  only) is hidden
- Now playing from a text file: follows a file another DJ program keeps up to date (such as
  the VirtualDJ tracklist), so titles work beyond rekordbox. The last line is the current
  track; a time at its start is ignored. A file left over from an earlier set is not announced
- The switch delay, controller and crossfader settings only show for the rekordbox sources
- Inputs with more than two channels (such as a DJ controller's USB input): choose which pair
  to capture (Settings → Audio → Channels). The DDJ-1000's master (11–12) is picked
  automatically, or press Detect while a track plays. When the chosen pair stays silent while
  another one has sound, the console says so and offers to switch. `djcast devices` shows the
  channel count, `meter --input --channel N` tests a pair
- The console shows how the input is captured (loopback, input or program) instead of always
  "Loopback capture"
- Removed the rekordbox play history source and its database key: the instant source gives
  the same titles without it. Settings using "Automatic" or "rekordbox history" switch to
  "rekordbox, instant". The `rekordbox set-key`, `info` and `history` commands are gone; a
  key file saved earlier (`rekordbox-db.key` in the djcast settings folder) can be deleted

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
