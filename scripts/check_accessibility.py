#!/usr/bin/env python3
"""Fail when the documented accessibility contract drifts across surfaces.

Accessibility switches are a two-way contract: a config key must exist in the
generated field reference, be described in the accessibility statement, and
(where user-facing) be translated in every desktop locale. These are cheap to
check deterministically and expensive to discover in review.

Scope is deliberately narrow. This is not an accessibility auditor; it guards
the specific invariants this project has already committed to:

- every `app.settings.accessibility.*` key has all 20 locale values
- the shared `[ui]` accessibility switches stay documented in the generated
  config field reference and in `.github/ACCESSIBILITY.md`
- the new high-contrast switch is exposed in both the config example and the
  generated reference

Usage: python3 scripts/check_accessibility.py [--root PATH]
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent

# Mirrors `UiLanguage::ALL` in vtcode-desktop's localization module. Keep in
# sync manually; a locale added there must be added here too.
LOCALES = (
    "en",
    "vi",
    "es",
    "fr",
    "ja",
    "de",
    "it",
    "zh-CN",
    "zh-TW",
    "zh-HK",
    "pt-BR",
    "ko",
    "nl",
    "sv",
    "uk",
    "hi",
    "pl",
    "tr",
    "ru",
    "id",
)

SWITCHES = (
    "ui.high_contrast_mode",
    "ui.minimum_contrast",
    "ui.screen_reader_mode",
    "ui.reduce_motion_mode",
    "ui.safe_colors_only",
    "ui.bold_is_bright",
)

REMEDIATION = (
    "Remediation:\n"
    "1. Add the missing locale value to crates/codegen/vtcode-desktop/locales/app.settings.yml.\n"
    "2. Regenerate docs/config/config_field_reference.md with\n"
    "   python3 scripts/generate_config_field_reference.py.\n"
    "3. Update .github/ACCESSIBILITY.md when user-facing behavior changes."
)


def locale_keys(root: Path) -> list[str]:
    """Return lines holding accessibility keys from the settings locale file."""
    path = root / "crates/codegen/vtcode-desktop/locales/app.settings.yml"
    lines = path.read_text(encoding="utf-8").splitlines()
    start = next(i for i, line in enumerate(lines) if line == "    accessibility:")
    keys: list[str] = []
    for line in lines[start + 1 :]:
        if line.startswith("    ") and not line.startswith("      "):
            break
        if line.startswith("      ") and ":" in line:
            keys.append(line.strip())
    return keys


def check_locale_parity(root: Path) -> list[str]:
    errors: list[str] = []
    for line in locale_keys(root):
        key = line.split(":", 1)[0]
        present = set(re.findall(r"\b([a-z]{2}(?:-[A-Za-z]{2})?)\s*:", line))
        missing = [locale for locale in LOCALES if locale not in present]
        if missing:
            errors.append(
                f"app.settings.accessibility.{key} missing locales: {', '.join(missing)}"
            )
    return errors


def check_documented_switches(root: Path) -> list[str]:
    errors: list[str] = []
    reference = (root / "docs/config/config_field_reference.md").read_text(
        encoding="utf-8"
    )
    statement = (root / ".github/ACCESSIBILITY.md").read_text(encoding="utf-8")
    example = (root / "vtcode.toml.example").read_text(encoding="utf-8")
    for switch in SWITCHES:
        if f"`{switch}`" not in reference:
            errors.append(f"config field reference is missing {switch}")
        if switch not in statement:
            errors.append(f"accessibility statement is missing {switch}")
    if "high_contrast_mode" not in example:
        errors.append("vtcode.toml.example does not document high_contrast_mode")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=REPO_ROOT, help="repository root")
    args = parser.parse_args()

    errors = check_locale_parity(args.root) + check_documented_switches(args.root)
    if errors:
        print("Accessibility contract drift detected:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        print(REMEDIATION, file=sys.stderr)
        return 1
    print("Accessibility contract is consistent.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
