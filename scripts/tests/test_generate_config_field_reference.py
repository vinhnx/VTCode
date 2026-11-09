"""Config reference output-path regression without invoking Cargo."""

import argparse
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


spec = importlib.util.spec_from_file_location(
    "config_reference", Path(__file__).parents[1] / "generate_config_field_reference.py"
)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


class ConfigReferenceTests(unittest.TestCase):
    def test_output_outside_repository_reports_success(self):
        schema = {"type": "object", "properties": {"example": {"type": "boolean", "default": True}}}
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "nested" / "reference.md"
            with mock.patch.object(module, "parse_args", return_value=argparse.Namespace(output=output)), \
                    mock.patch.object(module, "load_schema_from_cargo", return_value=schema), \
                    mock.patch("builtins.print") as report:
                self.assertEqual(module.main(), 0)
            self.assertIn("| `example` |", output.read_text())
            self.assertIn(str(output), report.call_args.args[0])

    def test_home_paths_render_portably_on_both_sides_of_the_prefix(self):
        home = "/home/alice"
        self.assertEqual(module.portable_default("/home/alice/.cache/vtcode", home), "~/.cache/vtcode")
        self.assertEqual(module.portable_default("/home/alice", home), "~")
        # A sibling directory sharing the prefix text is not under home.
        self.assertEqual(module.portable_default("/home/alice2/x", home), "/home/alice2/x")
        self.assertEqual(module.portable_default(".vtcode/bin", home), ".vtcode/bin")
        self.assertEqual(
            module.portable_default({"dirs": ["/home/alice/bin", "/opt/bin"], "n": 3}, home),
            {"dirs": ["~/bin", "/opt/bin"], "n": 3},
        )

    def test_format_default_never_emits_the_generating_home(self):
        rendered = module.format_default(str(Path.home() / ".local" / "state"))
        self.assertNotIn(str(Path.home()), rendered)
        self.assertIn("~/.local/state", rendered)


if __name__ == "__main__":
    unittest.main()
