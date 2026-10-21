"""Pack a full fixture set into the tracked archive.

    python -m parity.pack --fixtures .temp/fixtures --out tools/parity/fixtures

Output: OUT/fixtures-<version>.tar.zst, holding the top directory `<version>/` (the upstream release
pinned in Cargo.toml), so extracting it into `.temp/fixtures/` yields `.temp/fixtures/<version>/`.
An existing archive is left alone.
"""

import argparse
import json
import tarfile
from pathlib import Path

from backports import zstd

from .generate import pinned_commit, pinned_version

LEVEL = 19


def archive_name(version: str) -> str:
    return f"fixtures-{version}.tar.zst"


def validate(pin_dir: Path, pin: str, version: str) -> None:
    "Exit unless pin_dir is a full set for the pinned commit and version: a filtered or half-written set must never be archived."
    try:
        index = json.loads((pin_dir / "index.json").read_text(encoding="utf-8"))
    except (OSError, ValueError) as err:
        raise SystemExit(f"{pin_dir}: no readable index.json ({err})")
    if index.get("pin") != pin:
        raise SystemExit(f"{pin_dir}: index.json is for pin {index.get('pin')}, Cargo.toml pins {pin}")
    if index.get("upstream_version") != version:
        raise SystemExit(f"{pin_dir}: index.json is for upstream version {index.get('upstream_version')}, Cargo.toml has {version}")
    if not (pin_dir / "f1.json").is_file():
        raise SystemExit(f"{pin_dir}: no f1.json, this is a filtered set; run the generator without --only")
    non_ascii = [p.name for p in pin_dir.rglob("*") if not p.name.isascii()]
    if non_ascii:
        raise SystemExit(f"{pin_dir}: non-ASCII file names would not survive bsdtar on Windows: {non_ascii[:3]}")
    pages = sum(1 for _ in (pin_dir / "pages").glob("*.json"))
    if pages != len(index["pages"]):
        raise SystemExit(f"{pin_dir}: {pages} page files but index.json lists {len(index['pages'])}")


def pack(pin_dir: Path, out_file: Path) -> None:
    out_file.parent.mkdir(parents=True, exist_ok=True)
    with zstd.ZstdFile(out_file, "wb", level=LEVEL) as compressed:
        with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as tar:
            tar.add(pin_dir, arcname=pin_dir.name)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--fixtures", type=Path, required=True, help="directory holding <version>/")
    parser.add_argument("--out", type=Path, required=True, help="directory for the archive")
    args = parser.parse_args(argv)

    pin, version = pinned_commit(), pinned_version()
    out_file = args.out / archive_name(version)
    if out_file.exists():
        print(f"archive exists, delete it to pack again: {out_file}")
        return 0
    pin_dir = args.fixtures / version
    validate(pin_dir, pin, version)
    pack(pin_dir, out_file)
    print(f"{out_file} ({out_file.stat().st_size} bytes)")
    for other in args.out.glob("fixtures-*.tar.zst"):
        if other != out_file:
            print(f"archive of another version, remove it with git rm: {other}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
