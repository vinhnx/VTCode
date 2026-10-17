"""Exercise PGO stage boundaries with fake tools and real subprocesses."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


SOURCE = Path(__file__).resolve().parents[1] / "perf"


class PgoTests(unittest.TestCase):
    def fixture(self, directory, failure="", profiles=True):
        root = Path(directory)
        scripts = root / "scripts/perf"
        scripts.mkdir(parents=True)
        for name in ("pgo.py", "pgo_train_startup.py", "startup_env.py"):
            shutil.copy2(SOURCE / name, scripts / name)
        config = root / ".cargo/config.toml"
        config.parent.mkdir()
        config.write_text('[target.fixture-host]\nrustflags = ["-C", "link-arg=sentinel"]\n')
        commands = root / "commands"
        commands.mkdir()
        calls = root / "calls.jsonl"
        sysroot = root / "sysroot"
        profdata = sysroot / "lib/rustlib/fixture-host/bin/llvm-profdata"
        profdata.parent.mkdir(parents=True)

        def tool(path, body):
            path.write_text(f"#!{sys.executable}\n" + body)
            path.chmod(0o755)

        tool(commands / "rustc", f"import sys\nprint({str(sysroot)!r} if '--print' in sys.argv else 'host: fixture-host\\nLLVM version: fixture')\n")
        tool(commands / "git", "import sys\nprint('revision' if 'rev-parse' in sys.argv else '')\n")
        binary_source = (
            "import os, pathlib\n"
            "assert 'OPENAI_API_KEY' not in os.environ\n"
            "assert 'CARGO_ENCODED_RUSTFLAGS' not in os.environ\n"
            "assert pathlib.Path.cwd().name == 'workspace'\n"
            "assert pathlib.Path(os.environ['HOME']).parent == pathlib.Path.cwd().parent\n"
        )
        if profiles:
            content = "" if profiles == "empty" else "profile"
            binary_source += f"pathlib.Path(os.environ['LLVM_PROFILE_FILE'].replace('%m', 'id').replace('%p', str(os.getpid()))).write_text({content!r})\n"
        if failure == "train":
            binary_source += "raise SystemExit(19)\n"
        tool(commands / "cargo", (
            "import json, os, pathlib, sys\n"
            "arguments = sys.argv[1:]\n"
            "target = pathlib.Path(arguments[arguments.index('--target-dir') + 1])\n"
            "stage = target.name.removesuffix('-target')\n"
            f"with open({str(calls)!r}, 'a') as stream:\n"
            "    stream.write(json.dumps({'stage': stage, 'arguments': arguments, 'flags': os.environ['CARGO_ENCODED_RUSTFLAGS'], 'units': os.environ['CARGO_PROFILE_RELEASE_CODEGEN_UNITS']}) + '\\n')\n"
            f"if stage == {failure!r}: raise SystemExit(23)\n"
            "binary = target / 'fixture-host/release/vtcode'\n"
            "binary.parent.mkdir(parents=True)\n"
            f"binary.write_text({('#!' + sys.executable + chr(10) + binary_source)!r})\n"
            "binary.chmod(0o755)\n"
        ))
        tool(profdata, (
            "import pathlib, sys\n"
            f"with open({str(calls)!r}, 'a') as stream: stream.write('{{\"stage\": \"merge\"}}\\n')\n"
            f"if {failure!r} == 'merge': raise SystemExit(31)\n"
            "assert list(pathlib.Path(sys.argv[-1]).glob('*.profraw'))\n"
            "pathlib.Path(sys.argv[sys.argv.index('-o') + 1]).write_text('merged')\n"
        ))
        environment = os.environ.copy()
        environment.pop("CARGO_ENCODED_RUSTFLAGS", None)
        environment.pop("RUSTFLAGS", None)
        environment.update(PATH=f"{commands}:{environment.get('PATH', '')}", OPENAI_API_KEY="synthetic-secret")
        return scripts, calls, environment

    def run_fixture(self, failure="", profiles=True, flags=None, existing=False, missing_tools=False):
        with tempfile.TemporaryDirectory(prefix="vtcode pgo test ") as directory:
            scripts, calls, environment = self.fixture(directory, failure, profiles)
            if flags is not None:
                environment["CARGO_ENCODED_RUSTFLAGS"] = flags
                environment["RUSTFLAGS"] = "-C ignored-by-cargo"
            if missing_tools:
                (Path(directory) / "sysroot/lib/rustlib/fixture-host/bin/llvm-profdata").unlink()
            artifacts = Path(directory) / "artifacts with spaces"
            if existing:
                artifacts.mkdir()
                (artifacts / "keep.profraw").write_text("existing")
            result = subprocess.run(
                [sys.executable, str(scripts / "pgo.py"), "--output", str(artifacts), "--",
                 sys.executable, str(scripts / "pgo_train_startup.py")],
                env=environment, capture_output=True, text=True, timeout=20,
            )
            stages = [json.loads(line) for line in calls.read_text().splitlines()] if calls.exists() else []
            summary = artifacts / "summary.json"
            if existing:
                self.assertEqual((artifacts / "keep.profraw").read_text(), "existing")
            self.assertFalse((Path(directory) / "target").exists())
            return result, stages, json.loads(summary.read_text()) if summary.exists() else None

    def test_success_uses_matching_flags_and_distinct_artifacts(self):
        result, stages, summary = self.run_fixture()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([stage["stage"] for stage in stages], ["baseline", "generate", "merge", "use"])
        self.assertEqual(summary["base_rustflags"], ["-C", "link-arg=sentinel"])
        self.assertEqual(summary["profile_count"], 4)
        builds = [stage for stage in stages if stage["stage"] != "merge"]
        for stage in builds:
            self.assertEqual(stage["units"], "1")
            self.assertIn("--locked", stage["arguments"])
            self.assertIn("--target", stage["arguments"])
            self.assertTrue(stage["flags"].startswith("-C\x1flink-arg=sentinel"))
        self.assertNotIn("profile-", builds[0]["flags"])
        self.assertIn("-Cprofile-generate=", builds[1]["flags"])
        self.assertIn("-Cprofile-use=", builds[2]["flags"])
        self.assertIn("-pgo-warn-missing-function", builds[2]["flags"])
        self.assertEqual(len({entry["path"] for entry in summary["binaries"].values()}), 3)

    def test_failures_stop_before_later_stages_without_success_summary(self):
        for failure, expected, status in (
            ("baseline", ["baseline"], 23),
            ("generate", ["baseline", "generate"], 23),
            ("train", ["baseline", "generate"], 1),
            ("merge", ["baseline", "generate", "merge"], 31),
            ("use", ["baseline", "generate", "merge", "use"], 23),
        ):
            with self.subTest(failure=failure):
                result, stages, summary = self.run_fixture(failure=failure)
                self.assertEqual(result.returncode, status, result.stderr)
                self.assertEqual([stage["stage"] for stage in stages], expected)
                self.assertIsNone(summary)

    def test_no_profiles_rejects_training_before_merge(self):
        for profiles in (False, "empty"):
            with self.subTest(profiles=profiles):
                result, stages, summary = self.run_fixture(profiles=profiles)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("nonempty profiles", result.stderr)
                self.assertEqual([stage["stage"] for stage in stages], ["baseline", "generate"])
                self.assertIsNone(summary)

    def test_missing_matching_llvm_tools_rejects_before_build(self):
        result, stages, summary = self.run_fixture(missing_tools=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("rustup component add llvm-tools-preview", result.stderr)
        self.assertEqual(stages, [])
        self.assertIsNone(summary)

    def test_existing_directory_is_preserved_and_refused(self):
        result, stages, summary = self.run_fixture(existing=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(stages, [])
        self.assertIsNone(summary)

    def test_encoded_flags_override_rustflags_without_splitting_spaces(self):
        result, _, summary = self.run_fixture(flags="-C\x1flink-arg=path with spaces")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(summary["base_rustflags"], ["-C", "link-arg=path with spaces"])

    def test_conflicting_pgo_or_codegen_flags_reject_before_build(self):
        for flags in ("-Cprofile-use=stale.profdata", "-C\x1fprofile-generate=stale", "-Ccodegen-units=8"):
            with self.subTest(flags=flags):
                result, stages, summary = self.run_fixture(flags=flags)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("remove existing", result.stderr)
                self.assertEqual(stages, [])
                self.assertIsNone(summary)


if __name__ == "__main__":
    unittest.main()
