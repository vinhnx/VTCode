#!/usr/bin/env python3
"""
Synchronize workspace prompt/doc assets with the vtcode-core embedded assets mirror.
"""

import argparse
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CORE_EMBEDDED = ROOT / "vtcode-core" / "embedded_assets_source"

ASSET_MAPPINGS = {
    ROOT / "docs" / "modules/vtcode_docs_map.md": CORE_EMBEDDED / "docs" / "modules/vtcode_docs_map.md",
}


def sync_assets(dry_run: bool = False) -> None:
    for source, dest in ASSET_MAPPINGS.items():
        if not source.is_file():
            raise FileNotFoundError(f"Source asset missing: {source}");

        dest.parent.mkdir(parents=True, exist_ok=True)

        if source.read_bytes() == dest.read_bytes() if dest.exists() else False:
            continue

        if dry_run:
            print(
                f"Would update {dest.relative_to(ROOT)} from {source.relative_to(ROOT)}"
            )
        else:
            shutil.copy2(source, dest)
            print(f"Updated {dest.relative_to(ROOT)}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--dry-run", action="store_true", help="Show actions without copying"
    )
    args = parser.parse_args()

    sync_assets(dry_run=args.dry_run)


if __name__ == "__main__":
    main()
