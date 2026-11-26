"""Write notes.md and latest.json for a version, next to the release files.

Usage: python release_files.py <version> <dir>
<dir> holds what the build jobs collected: the Windows installers (setup .exe + .sig, .msi)
and the macOS app (universal .dmg, updater archive .app.tar.gz + .sig).
Run from the repository root (it reads CHANGELOG.md).
"""
import datetime
import json
import sys
from pathlib import Path

REPO = "danmarinescu79/djcast"


def changelog_section(version: str) -> str:
    """The bullet list under '## <version>' in CHANGELOG.md."""
    lines = Path("CHANGELOG.md").read_text(encoding="utf-8").splitlines()
    out, inside = [], False
    for line in lines:
        if line.startswith("## "):
            if inside:
                break
            inside = line[3:].strip() == version
            continue
        if inside:
            out.append(line)
    text = "\n".join(out).strip()
    if not text:
        sys.exit(f"CHANGELOG.md has no section for {version}")
    return text


def main() -> None:
    version, out_dir = sys.argv[1], Path(sys.argv[2])
    setup = f"djcast_{version}_x64-setup.exe"
    dmg = f"djcast_{version}_universal.dmg"
    mac_update = f"djcast_{version}_universal.app.tar.gz"
    for name in (setup, setup + ".sig", f"djcast_{version}_x64_en-US.msi", dmg, mac_update, mac_update + ".sig"):
        if not (out_dir / name).exists():
            sys.exit(f"missing build output: {out_dir / name}")

    changes = changelog_section(version)
    install = (
        "\n\n## Install\n"
        f"**Windows 10/11 (64-bit):** download **`{setup}`** (or the `.msi`) and run it. "
        "The installer is not code-signed yet: if SmartScreen says *Windows protected your PC*, "
        "choose **More info → Run anyway**.\n\n"
        f"**macOS 13 or newer (Apple Silicon and Intel):** download **`{dmg}`**, open it and drag "
        "djcast to Applications. To show the tracks, allow djcast under System Settings → "
        "Privacy & Security → Accessibility when it asks.\n\n"
        "Installed versions update themselves.\n\n"
        "Not affiliated with AlphaTheta, Pioneer DJ or Serato. rekordbox is a trademark of AlphaTheta "
        "Corporation; Serato is a trademark of Serato Limited."
    )
    (out_dir / "notes.md").write_text(f"## What's new\n{changes}{install}\n", encoding="utf-8")

    def platform(file: str) -> dict:
        return {
            "signature": (out_dir / (file + ".sig")).read_text(encoding="utf-8").strip(),
            "url": f"https://github.com/{REPO}/releases/download/v{version}/{file}",
        }

    mac = platform(mac_update)  # one universal app for both Mac architectures
    manifest = {
        "version": version,
        "notes": changes,
        "pub_date": datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
        "platforms": {
            "windows-x86_64": platform(setup),
            "darwin-aarch64": mac,
            "darwin-x86_64": mac,
        },
    }
    (out_dir / "latest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"release notes and latest.json for {version} in {out_dir}")


if __name__ == "__main__":
    main()
