# Security policy

## Supported versions
Only the latest release gets security fixes. The app updates itself (Settings → General), so
staying on the latest version is the easiest way to be covered.

## Reporting a vulnerability
Please report security problems **privately** through
[GitHub's private vulnerability reporting](https://github.com/danmarinescu79/djcast/security/advisories/new),
not in a public issue or discussion.

Include what you found, how to reproduce it, the djcast version and your operating system.
You can expect a first answer within a week. Once a fix is released, the advisory is
published and you are credited, unless you prefer not to be.

## What is in scope
For example:
- the update mechanism (signature checks, `latest.json`, download URLs);
- how server passwords and the Discord webhook are stored (Windows Credential Manager,
  macOS Keychain) or could leak (logs, setup report, tracklists);
- the connections to streaming servers (TLS certificate checks);
- the release workflow and anything that could publish a build not made from this repository.

Problems in rekordbox, Icecast, Shoutcast or other software djcast works with should be
reported to their makers.
