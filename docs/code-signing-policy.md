# Code signing policy

The Windows installers of djcast are **not code-signed yet**. We applied to the
[SignPath Foundation](https://signpath.org) program for free code signing in November 2026;
it is meant for projects that are already widely known, so we will apply again once djcast
has a larger community. This policy describes how releases are made now and how they will be
signed then.

Until then, every update the app installs is verified against the updater key (minisign),
so an update can only come from this project's release workflow.

## What gets signed
Only the files built from this repository by the
[release workflow](../.github/workflows/release.yml) on GitHub Actions: the NSIS installer
(`djcast_<version>_x64-setup.exe`), the MSI package and the `djcast.exe` they contain.
Nothing built on a personal computer is signed, and no third-party binaries are signed as if
they were ours.

## How a release is made
1. Changes reach `main` through pull requests; files that control builds and releases
   (`.github/`, `tauri.conf.json`, `capabilities/`) need the code owner's review
   ([CODEOWNERS](../.github/CODEOWNERS)).
2. A release starts when a `v*` tag is pushed. Tags matching `v*` are protected by a
   repository ruleset.
3. The release workflow runs in the protected `release` environment, which waits for a
   manual approval before it gets access to any signing secret.
4. The workflow builds the installers from the tagged commit, signs the update packages with
   the updater key (and, once available, the installers with the code signing certificate),
   and publishes them with the release notes from the [changelog](../CHANGELOG.md).

## Team roles
| Role | Who |
|---|---|
| Authors / committers | [Dan Marinescu](https://github.com/danmarinescu79) |
| Reviewers | [Dan Marinescu](https://github.com/danmarinescu79) |
| Approvers (release signing) | [Dan Marinescu](https://github.com/danmarinescu79) |

Every account with these roles uses multi-factor authentication on GitHub. Contributions
from others are reviewed before they are merged.

## Privacy
See [Privacy](../README.md#privacy) in the README: the app sends nothing to any server that
you have not set up yourself, apart from the update check, which you can turn off.
