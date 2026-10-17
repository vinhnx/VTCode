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


if __name__ == "__main__":
    unittest.main()
