#!/usr/bin/env python3
"""Build Terminal Studio's pinned Lucide icon-font subset.

Pinned release: lucide-font 0.468.0
Source: https://github.com/lucide-icons/lucide/releases/download/0.468.0/lucide-font-0.468.0.zip
"""

from __future__ import annotations

import io
import json
import re
import sys
import urllib.request
import zipfile
from pathlib import Path

try:
    from fontTools import subset
except ImportError:
    sys.exit("fontTools is required; install it with: python3 -m pip install fonttools")


VERSION = "0.468.0"
ARCHIVE_URL = (
    "https://github.com/lucide-icons/lucide/releases/download/"
    f"{VERSION}/lucide-font-{VERSION}.zip"
)
ROOT = Path(__file__).resolve().parents[1]
ICONS_RS = ROOT / "src" / "ui_kit" / "icons.rs"
OUTPUT_TTF = ROOT / "assets" / "fonts" / "lucide-subset.ttf"


def requested_icon_names(source: str) -> list[str]:
    try:
        all_source = source.split("pub const ALL:", 1)[1]
    except IndexError:
        sys.exit(f"could not find ALL in {ICONS_RS}")

    names = re.findall(
        r'\("([a-z0-9-]+)",\s*icon::[A-Z][A-Z0-9_]*\)',
        all_source,
    )
    if not names:
        sys.exit(f"could not find icon names in ALL in {ICONS_RS}")
    if len(names) != len(set(names)):
        sys.exit("ALL contains duplicate Lucide icon names")
    return names


def download_release() -> bytes:
    request = urllib.request.Request(
        ARCHIVE_URL,
        headers={"User-Agent": "terminal-studio-font-subsetter"},
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def release_files(archive: bytes) -> tuple[bytes, dict[str, dict[str, str]]]:
    with zipfile.ZipFile(io.BytesIO(archive)) as bundle:
        font = bundle.read("lucide-font/lucide.ttf")
        info = json.loads(bundle.read("lucide-font/info.json"))
    return font, info


def subset_font(font: bytes, codepoints: list[int]) -> None:
    options = subset.Options()
    options.flavor = None
    options.recalc_timestamp = False
    options.canonical_order = True
    options.name_IDs = ["*"]
    options.name_languages = ["*"]

    face = subset.load_font(io.BytesIO(font), options)
    subsetter = subset.Subsetter(options=options)
    subsetter.populate(unicodes=codepoints)
    subsetter.subset(face)

    OUTPUT_TTF.parent.mkdir(parents=True, exist_ok=True)
    subset.save_font(face, str(OUTPUT_TTF), options)


def main() -> None:
    names = requested_icon_names(ICONS_RS.read_text(encoding="utf-8"))
    font, info = release_files(download_release())

    missing = [name for name in names if name not in info]
    if missing:
        sys.exit(f"icons missing from Lucide {VERSION} info.json: {missing}")

    codepoints = [int(info[name]["unicode"][2:-1]) for name in names]
    subset_font(font, codepoints)
    print(
        f"wrote {OUTPUT_TTF.relative_to(ROOT)} "
        f"({OUTPUT_TTF.stat().st_size} bytes, {len(codepoints)} icons)"
    )


if __name__ == "__main__":
    main()
