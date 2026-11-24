"""Regression coverage for the accessibility contract gate.

The gate is only useful if it actually fails on drift. These tests pin both
sides: the shipped tree must pass, and a locale missing a translation or a
config switch missing from the generated reference must be reported.
"""

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    "check_accessibility", SCRIPTS / "check_accessibility.py"
)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)

LOCALE_LINE = (
    '      contrast: {en: "Contrast", vi: "Độ tương phản", es: "Contraste", '
    'fr: "Contraste", ja: "コントラスト", de: "Kontrast", it: "Contrasto", '
    'zh-CN: "对比度", zh-TW: "對比度", zh-HK: "對比度", pt-BR: "Contraste", '
    'ko: "대비", nl: "Contrast", sv: "Kontrast", uk: "Контрастність", '
    'hi: "कंट्रास्ट", pl: "Kontrast", tr: "Kontrast", ru: "Контрастность", '
    'id: "Kontras"}\n'
)


class ContractTests(unittest.TestCase):
    def test_shipped_tree_has_no_drift(self):
        self.assertEqual(module.check_locale_parity(module.REPO_ROOT), [])
        self.assertEqual(module.check_documented_switches(module.REPO_ROOT), [])

    def test_missing_locale_value_is_reported(self):
        with tempfile.TemporaryDirectory(prefix="vtcode-a11y-") as directory:
            root = Path(directory)
            locale = root / "crates/codegen/vtcode-desktop/locales"
            locale.mkdir(parents=True)
            (locale / "app.settings.yml").write_text(
                "    accessibility:\n"
                + LOCALE_LINE.replace('vi: "Độ tương phản", ', "")
            )
            errors = module.check_locale_parity(root)
            self.assertEqual(len(errors), 1)
            self.assertIn("contrast", errors[0])
            self.assertIn("vi", errors[0])

    def test_reporting_contract_passes_on_shipped_tree(self):
        self.assertEqual(module.check_reporting_contract(module.REPO_ROOT), [])

    def test_missing_issue_template_is_reported(self):
        with tempfile.TemporaryDirectory(prefix="vtcode-a11y-") as directory:
            root = Path(directory)
            (root / ".github").mkdir()
            (root / ".github/ACCESSIBILITY.md").write_text(
                "template=accessibility_report.md\n"
                + "\n".join(module.REPORTING_SECTIONS),
                encoding="utf-8",
            )
            errors = module.check_reporting_contract(root)
            self.assertEqual(len(errors), 1)
            self.assertIn(module.ACCESSIBILITY_TEMPLATE, errors[0])

    def test_wrong_label_is_reported(self):
        with tempfile.TemporaryDirectory(prefix="vtcode-a11y-") as directory:
            root = self._reporting_root(directory, label="bug", media_guidance=True)
            errors = module.check_reporting_contract(root)
            self.assertEqual(len(errors), 1)
            self.assertIn("does not request the accessibility label", errors[0])

    def test_missing_media_guidance_and_section_are_reported(self):
        with tempfile.TemporaryDirectory(prefix="vtcode-a11y-") as directory:
            root = self._reporting_root(
                directory,
                label="accessibility",
                media_guidance=False,
                omit_section="## How we respond",
            )
            errors = module.check_reporting_contract(root)
            self.assertEqual(len(errors), 2)
            self.assertTrue(any("make media optional" in error for error in errors))
            self.assertTrue(any("## How we respond" in error for error in errors))

    @staticmethod
    def _reporting_root(directory, *, label, media_guidance, omit_section=None):
        root = Path(directory)
        template_dir = root / ".github/ISSUE_TEMPLATE"
        template_dir.mkdir(parents=True)
        (root / ".github/ACCESSIBILITY.md").write_text(
            "template=accessibility_report.md\n"
            + "\n".join(s for s in module.REPORTING_SECTIONS if s != omit_section),
            encoding="utf-8",
        )
        guidance = (
            "Media is optional; no disability disclosure needed.\n"
            if media_guidance
            else ""
        )
        (template_dir / "accessibility_report.md").write_text(
            f"labels: {label}\n{guidance}", encoding="utf-8"
        )
        return root

    def test_undocumented_switch_is_reported(self):
        with tempfile.TemporaryDirectory(prefix="vtcode-a11y-") as directory:
            root = Path(directory)
            (root / "docs/config").mkdir(parents=True)
            (root / ".github").mkdir()
            documented = [
                "`ui.minimum_contrast`",
                "`ui.screen_reader_mode`",
                "`ui.reduce_motion_mode`",
                "`ui.safe_colors_only`",
                "`ui.bold_is_bright`",
            ]
            (root / "docs/config/config_field_reference.md").write_text(
                "\n".join(documented)
            )
            (root / ".github/ACCESSIBILITY.md").write_text(" ".join(documented))
            (root / "vtcode.toml.example").write_text("[ui]\n")
            errors = module.check_documented_switches(root)
            self.assertIn(
                "config field reference is missing ui.high_contrast_mode", errors
            )
            self.assertIn(
                "accessibility statement is missing ui.high_contrast_mode", errors
            )
            self.assertIn(
                "vtcode.toml.example does not document high_contrast_mode", errors
            )


if __name__ == "__main__":
    unittest.main()
