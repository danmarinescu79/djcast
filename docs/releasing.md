# Releasing

## Update signing key

The desktop app updates itself from GitHub releases. Every update is signed with a private
key and checked against the public key in `apps/desktop/src-tauri/tauri.conf.json`
(`plugins.updater.pubkey`) before it is installed.

- The private key lives **outside the repository**: `%USERPROFILE%\.tauri\djcast-updater.key`.
  Never commit it. Keep a backup somewhere safe (password manager, encrypted drive): if it is
  lost, already-installed apps can no longer verify updates and users must reinstall by hand.
- It was generated with `npx tauri signer generate --ci -w %USERPROFILE%\.tauri\djcast-updater.key`
  (no password).
- For CI it is stored as the secret `TAURI_SIGNING_PRIVATE_KEY` of the protected **`release`
  environment** (not a repository secret): only jobs started by a `v*` tag can request it, and
  each such job waits for the owner's approval.

## Automatic release (recommended)

1. Bump the version in `Cargo.toml` (`[workspace.package]`), `apps/desktop/src-tauri/tauri.conf.json`
   and `apps/desktop/package.json`; add a `## <version>` section to `CHANGELOG.md`.
2. Commit, then tag and push: `git tag -s v<version> -m "djcast <version>"` and `git push origin v<version>`.
3. GitHub Actions → *Release* → the job waits for approval of the `release` environment:
   **Review deployments → Approve**.
4. The workflow checks that the tag matches the app version, builds the signed installers, writes
   `notes.md` and `latest.json` from the CHANGELOG section (`.github/scripts/release_files.py`) and
   publishes the release. Installed apps see the update on their next check.

## Repository protections
- `CODEOWNERS`: changes to `.github/`, `tauri.conf.json` and the capabilities need the owner's review.
- Ruleset *Protect main*: changes through pull requests with code-owner approval; no force pushes
  or deletion (repository admins can bypass).
- Ruleset *Protect release tags*: only admins create, move or delete `v*` tags.
- Pull requests from forks never receive secrets; the CI workflow uses none.

## Manual release (Windows)

1. Bump the version in `Cargo.toml` (`[workspace.package]`), `apps/desktop/src-tauri/tauri.conf.json`
   and `apps/desktop/package.json`; describe the changes in `CHANGELOG.md`.
2. Build with the signing key available:
   ```powershell
   $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content "$env:USERPROFILE\.tauri\djcast-updater.key" -Raw
   $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
   cd apps/desktop
   npx tauri build
   ```
   This produces, in `target/release/bundle/`:
   - `msi/djcast_<version>_x64_en-US.msi` and `nsis/djcast_<version>_x64-setup.exe` (installers)
   - `nsis/djcast_<version>_x64-setup.exe.sig` (update signature)
3. Write `latest.json` (the updater reads it from the latest release):
   ```json
   {
     "version": "<version>",
     "notes": "<what's new — shown to users before they update>",
     "pub_date": "<RFC 3339 date, e.g. 2026-11-24T16:00:00Z>",
     "platforms": {
       "windows-x86_64": {
         "signature": "<content of djcast_<version>_x64-setup.exe.sig>",
         "url": "https://github.com/danmarinescu79/djcast/releases/download/v<version>/djcast_<version>_x64-setup.exe"
       }
     }
   }
   ```
4. Create the GitHub release `v<version>` and attach the two installers, the `.sig` file and
   `latest.json`. The app's update check uses
   `https://github.com/danmarinescu79/djcast/releases/latest/download/latest.json`.

## Code signing

The installers are not code-signed yet, so Windows SmartScreen warns on first run. See the
roadmap (SignPath Foundation for open-source projects).
