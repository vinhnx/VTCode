"""Sequential CI suites must preserve earlier nextest failure reports."""

import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[2]


class NextestReportTests(unittest.TestCase):
    def test_sequential_workflow_suites_preserve_failure_reports(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            (root / ".github").mkdir()
            shutil.copyfile(ROOT / ".github/nextest.toml", root / ".github/nextest.toml")
            (root / "Cargo.toml").write_text(
                '[package]\nname = "ci-report-harness-fixture"\nversion = "0.0.0"\n'
                'edition = "2024"\n[profile.ci]\ninherits = "dev"\n'
            )
            (root / "src/lib.rs").write_text(
                '#[test]\nfn earlier_failure() { panic!("earlier suite failure evidence"); }\n'
                '#[test]\nfn later_success() { assert_eq!(2 + 3, 5); }\n'
            )
            env = {**os.environ, "CARGO_TARGET_DIR": str(root / "target")}
            for job in ("test", "check-windows"):
                with self.subTest(job=job):
                    block = re.search(
                        rf"^  {re.escape(job)}:\n(.*?)(?=^  \S|\Z)",
                        workflow, re.MULTILINE | re.DOTALL,
                    )
                    self.assertIsNotNone(block, f"missing workflow job: {job}")
                    profiles = re.findall(r"--profile ([\w-]+)", block.group(1))
                    self.assertEqual(len(profiles), 2, f"expected two nextest suites in {job}")
                    self.assertEqual(
                        re.findall(r"--config-file ([\w./-]+)", block.group(1)),
                        [".github/nextest.toml"] * len(profiles),
                        f"every CI suite must use tracked configuration in {job}",
                    )
                    previous_reports = {}
                    for profile, test_name, expected_exit in zip(
                        profiles, ("earlier_failure", "later_success"), (100, 0),
                    ):
                        result = subprocess.run(
                            [
                                "cargo", "nextest", "run", "--offline",
                                "--config-file", ".github/nextest.toml", "--profile", profile,
                                "--cargo-profile", "ci", "--test-threads", "1",
                                "--status-level", "fail", "--final-status-level", "fail",
                                test_name,
                            ],
                            cwd=root, env=env, capture_output=True, text=True, timeout=120,
                        )
                        self.assertEqual(result.returncode, expected_exit, result.stdout + result.stderr)
                        for path, contents in previous_reports.items():
                            self.assertEqual(path.read_bytes(), contents, f"report overwritten: {path}")
                        reports = list((root / "target/nextest" / profile).rglob("*.xml"))
                        self.assertEqual(len(reports), 1, result.stdout + result.stderr)
                        report = ET.parse(reports[0]).getroot()
                        cases = report.findall(".//testcase")
                        self.assertEqual([case.attrib["name"] for case in cases], [test_name])
                        self.assertEqual(len(report.findall(".//failure")), int(expected_exit != 0))
                        if expected_exit:
                            self.assertIn("earlier suite failure evidence", reports[0].read_text())
                        previous_reports[reports[0]] = reports[0].read_bytes()


if __name__ == "__main__":
    unittest.main()
