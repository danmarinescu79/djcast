"""Collect the release files for a version and write notes.md and latest.json.

Usage: python release_files.py <version> <out_dir>
Run from the repository root after `npx tauri build` with the updater signing key set.
"""
import datetime
import json
import shutil
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
    out_dir.mkdir(parents=True, exist_ok=True)
    bundle = Path("target/release/bundle")
    setup = bundle / "nsis" / f"djcast_{version}_x64-setup.exe"
    files = [setup, setup.with_name(setup.name + ".sig"), bundle / "msi" / f"djcast_{version}_x64_en-US.msi"]
    for f in files:
        if not f.exists():
            sys.exit(f"missing build output: {f}")
        shutil.copy2(f, out_dir / f.name)

    changes = changelog_section(version)
    install = (
        "\n\n## Install\n"
        f"Download **`{setup.name}`** (or the `.msi`) and run it (Windows 10/11, 64-bit). "
        "The installer is not code-signed yet: if SmartScreen says *Windows protected your PC*, "
        "choose **More info → Run anyway**. Installed versions update themselves.\n\n"
        "Not affiliated with AlphaTheta or Pioneer DJ. rekordbox is a trademark of AlphaTheta Corporation."
    )
    (out_dir / "notes.md").write_text(f"## What's new\n{changes}{install}\n", encoding="utf-8")

    manifest = {
        "version": version,
        "notes": changes,
        "pub_date": datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
        "platforms": {
            "windows-x86_64": {
                "signature": (out_dir / (setup.name + ".sig")).read_text(encoding="utf-8").strip(),
                "url": f"https://github.com/{REPO}/releases/download/v{version}/{setup.name}",
            }
        },
    }
    (out_dir / "latest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"release files for {version} in {out_dir}")


if __name__ == "__main__":
    main()
