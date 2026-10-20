#!/usr/bin/env python3
"""Release artifact contracts, using only the standard library."""

import os
import re
import subprocess
import tarfile
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RELEASE = (ROOT / ".github/workflows/build-linux-windows.yml").read_text()


def job(text, name):
    match = re.search(
        rf"^  {re.escape(name)}:\n(.*?)(?=^  [\w-]+:|\Z)", text, re.M | re.S
    )
    if match is None:
        raise AssertionError(f"missing job: {name}")
    return match.group(1)


def step_script(block, name):
    step = re.search(
        rf"^      - name: {re.escape(name)}\n(.*?)(?=^      - |\Z)", block, re.M | re.S
    )
    if step is None:
        raise AssertionError(f"missing step: {name}")
    return textwrap.dedent(step.group(1).split("        run: |\n", 1)[1])


class CiLoadTests(unittest.TestCase):
    def test_release_keeps_every_target_and_man_dependency(self):
        native = job(RELEASE, "build-linux-gnu")
        cross = job(RELEASE, "build-linux")
        windows = job(RELEASE, "build-windows")
        targets = re.findall(r"^          - ([\w-]+)\s*$", native + cross, re.M)
        self.assertCountEqual(
            targets,
            [
                "x86_64-unknown-linux-gnu",
                "x86_64-unknown-linux-musl",
                "aarch64-unknown-linux-gnu",
            ],
        )
        self.assertNotRegex(
            cross,
            r"^\s+needs:",
            "cross builds must start immediately, not idle on build-linux-gnu",
        )
        self.assertIn("name: man-pages", native)
        self.assertIn("Download man pages", cross)
        # Packaging depends on the shared artifact; the fetch must retry and
        # fail closed instead of silently producing an archive without man pages.
        man_step = step_script(cross, "Download man pages")
        # Poll the API for existence instead of scraping `gh` stderr: a missing
        # artifact and a real 404 both exit non-zero, so message matching cannot
        # tell "wait" from "give up".
        self.assertIn("/actions/runs/${GITHUB_RUN_ID}/artifacts", man_step)
        self.assertIn("grep -qx man-pages", man_step)
        self.assertNotIn("2>&1) &&", man_step, "do not classify errors from stderr")
        self.assertIn("/actions/runs/${GITHUB_RUN_ID}/jobs", man_step)
        self.assertIn("Build (x86_64-unknown-linux-gnu)", man_step)
        self.assertIn('.status == "completed" and .conclusion != "success"', man_step)
        # Bounded wait: 40 attempts, no sleep after the final one.
        self.assertIn("seq 1 40", man_step)
        self.assertIn("sleep 15", man_step)
        self.assertIn('if [[ "$i" -lt 40 ]]', man_step)
        # Never package an empty artifact, and never merge stale extractions.
        self.assertIn("rm -rf man", man_step)
        self.assertIn("man-pages artifact contained no pages", man_step)
        self.assertIn("exit 1", man_step)
        # GET .../actions/artifacts requires the `actions` scope.
        self.assertIn("actions: read", cross)
        self.assertIn("--target x86_64-pc-windows-msvc", windows)
        self.assertNotIn(
            "--frozen", windows.split("        run: cargo build", 1)[1].splitlines()[0]
        )
        self.assertNotIn("mlugg/setup-zig", RELEASE)
        self.assertNotIn("  gen-man:", RELEASE)

    def run_man_page_download(
        self,
        *,
        artifact_ready_at,
        producer_state,
        has_man_page=True,
        fail_endpoint="",
    ):
        script = step_script(job(RELEASE, "build-linux"), "Download man pages")
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            tools = root / "bin"
            tools.mkdir()
            gh = tools / "gh"
            gh.write_text(
                r"""#!/usr/bin/env bash
set -eu
if [[ "$1" == api ]]; then
  case "$3" in
    */artifacts)
      count_file="$GH_STUB_DIR/artifact-count"
      count=$(cat "$count_file" 2>/dev/null || printf 0)
      count=$((count + 1))
      printf "%s\n" "$count" > "$count_file"
      [[ "$GH_API_FAIL_ENDPOINT" != artifacts ]] || exit 7
      if [[ "$GH_ARTIFACT_READY_AT" -gt 0 && "$count" -ge "$GH_ARTIFACT_READY_AT" ]]; then
        printf "man-pages\n"
      fi
      ;;
    */jobs)
      [[ "$GH_API_FAIL_ENDPOINT" != jobs ]] || exit 8
      if [[ "$GH_PRODUCER_STATE" == completed$'\t'* ]]; then
        conclusion="${GH_PRODUCER_STATE#*$'\t'}"
        [[ "$conclusion" == success ]] || printf "%s\n" "$conclusion"
      fi
      ;;
    *) exit 90 ;;
  esac
elif [[ "$1" == run && "$2" == download ]]; then
  shift 2
  while [[ $# -gt 0 ]]; do
    if [[ "$1" == --dir ]]; then dir="$2"; shift 2; else shift; fi
  done
  mkdir -p "$dir/man1"
  if [[ "$GH_HAS_MAN_PAGE" == true ]]; then printf "test\n" > "$dir/man1/vtcode.1"; fi
else
  exit 91
fi
"""
            )
            gh.chmod(0o755)
            sleep = tools / "sleep"
            sleep.write_text(
                r"""#!/usr/bin/env bash
printf "%s\n" "$*" >> "$GH_STUB_DIR/sleep-count"
"""
            )
            sleep.chmod(0o755)
            stub_dir = root / "stub-state"
            stub_dir.mkdir()
            environment = os.environ.copy()
            environment.update(
                {
                    "PATH": f"{tools}{os.pathsep}{environment['PATH']}",
                    "GH_STUB_DIR": str(stub_dir),
                    "GH_ARTIFACT_READY_AT": str(artifact_ready_at),
                    "GH_PRODUCER_STATE": producer_state,
                    "GH_HAS_MAN_PAGE": "true" if has_man_page else "false",
                    "GH_API_FAIL_ENDPOINT": fail_endpoint,
                    "GITHUB_REPOSITORY": "example/project",
                    "GITHUB_RUN_ID": "12345",
                }
            )
            result = subprocess.run(
                ["bash", "-eu", "-c", script],
                cwd=root,
                check=False,
                env=environment,
                capture_output=True,
                text=True,
            )
            artifact_count = stub_dir / "artifact-count"
            sleep_count = stub_dir / "sleep-count"
            return {
                "process": result,
                "artifact_attempts": (
                    int(artifact_count.read_text()) if artifact_count.exists() else 0
                ),
                "sleep_calls": (
                    len(sleep_count.read_text().splitlines())
                    if sleep_count.exists()
                    else 0
                ),
                "page_exists": (root / "man/man1/vtcode.1").is_file(),
            }

    def test_cross_download_retries_and_fails_fast_when_gnu_producer_fails(self):
        result = self.run_man_page_download(
            artifact_ready_at=2,
            producer_state="in_progress\t",
        )
        self.assertEqual(result["process"].returncode, 0, result["process"].stderr)
        self.assertEqual(result["artifact_attempts"], 2)
        self.assertEqual(result["sleep_calls"], 1)
        self.assertTrue(result["page_exists"])

        result = self.run_man_page_download(
            artifact_ready_at=0,
            producer_state="completed\tfailure",
        )
        self.assertEqual(result["process"].returncode, 1)
        self.assertEqual(result["artifact_attempts"], 1)
        self.assertEqual(result["sleep_calls"], 0)
        self.assertFalse(result["page_exists"])
        self.assertIn("finished with conclusion 'failure'", result["process"].stderr)

    def test_cross_download_fails_closed_for_api_errors_and_empty_artifacts(self):
        result = self.run_man_page_download(
            artifact_ready_at=0,
            producer_state="in_progress\t",
            fail_endpoint="artifacts",
        )
        self.assertEqual(result["process"].returncode, 7)
        self.assertEqual(result["artifact_attempts"], 1)
        self.assertEqual(result["sleep_calls"], 0)

        result = self.run_man_page_download(
            artifact_ready_at=1,
            producer_state="in_progress\t",
            has_man_page=False,
        )
        self.assertEqual(result["process"].returncode, 1)
        self.assertEqual(result["artifact_attempts"], 1)
        self.assertEqual(result["sleep_calls"], 0)
        self.assertFalse(result["page_exists"])
        self.assertIn("man-pages artifact contained no pages", result["process"].stderr)

    def test_compiler_cache_is_explicit_and_excludes_cross_containers(self):
        defaults = re.findall(
            r"      compiler_cache_pilot:\n(.*?)(?=^      \w|^\S)", RELEASE, re.M | re.S
        )
        self.assertEqual(len(defaults), 2)
        self.assertTrue(all("default: false" in block for block in defaults))
        for name in ("build-linux-gnu", "build-windows"):
            block = job(RELEASE, name)
            self.assertIn("inputs.compiler_cache_pilot && 'sccache' || ''", block)
            self.assertIn("sccache --show-stats", block)
        self.assertNotIn("sccache", job(RELEASE, "build-linux"))

    def test_new_and_old_release_tags_generate_pages_and_propagate_failures(self):
        script = step_script(
            job(RELEASE, "build-linux-gnu"),
            "Generate man pages from the release binary",
        )
        for mode in ("new", "old", "help-error", "generate-error"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                binary = root / "target/x86_64-unknown-linux-gnu/release-fast/vtcode"
                binary.parent.mkdir(parents=True)
                binary.write_text(
                    "#!/bin/bash\nset -eu\n"
                    + {
                        "new": 'if [[ "$*" == "man --help" ]]; then echo --all-pages; exit; fi\nprintf "%s\\n" "$*" > calls\nmkdir -p man/man1\necho native > man/man1/vtcode.1\n',
                        "old": 'echo "  --allowed-tools <TOOLS>  Global tool allow-list"\necho "  --all  Show all sessions"\n',
                        "help-error": "exit 7\n",
                        "generate-error": 'if [[ "$*" == "man --help" ]]; then echo --all-pages; exit; fi\nexit 9\n',
                    }[mode]
                )
                binary.chmod(0o755)
                prelude = 'cargo() { printf "%s\\n" "$*" > calls; mkdir -p man/man1; echo legacy > man/man1/vtcode.1; }\n'
                result = subprocess.run(
                    ["bash", "-eu", "-c", prelude + script],
                    cwd=root,
                    capture_output=True,
                    text=True,
                )
                self.assertEqual(
                    result.returncode,
                    {"new": 0, "old": 0, "help-error": 7, "generate-error": 9}[mode],
                    result.stderr,
                )
                if mode in ("new", "old"):
                    self.assertEqual(
                        (root / "man/man1/vtcode.1").read_text().strip(),
                        "native" if mode == "new" else "legacy",
                    )
                    self.assertEqual(
                        (root / "calls").read_text().strip(),
                        "man --all-pages --output man/man1"
                        if mode == "new"
                        else "run --locked -p xtask -- gen-man --out-dir man/man1",
                    )
                else:
                    self.assertFalse(
                        (root / "calls").exists(),
                        "generation errors must not use legacy fallback",
                    )

    def test_linux_archives_include_binary_and_shared_pages(self):
        for name, target in [
            ("build-linux-gnu", "x86_64-unknown-linux-gnu"),
            ("build-linux", "aarch64-unknown-linux-gnu"),
        ]:
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                binary = root / f"target/{target}/release-fast/vtcode"
                binary.parent.mkdir(parents=True)
                binary.write_bytes(b"expected-release-binary")
                man = root / "man/man1/vtcode.1"
                man.parent.mkdir(parents=True)
                man.write_text("expected-release-man-page")
                script = (
                    step_script(job(RELEASE, name), "Package")
                    .replace("${{ matrix.target }}", target)
                    .replace("${{ inputs.tag }}", "1.2.3")
                )
                subprocess.run(
                    ["bash", "-eu", "-c", script],
                    cwd=root,
                    check=True,
                    capture_output=True,
                )
                archive = root / f"vtcode-1.2.3-{target}.tar.gz"
                with tarfile.open(archive) as contents:
                    self.assertEqual(
                        contents.extractfile("./vtcode").read(),
                        b"expected-release-binary",
                    )
                    self.assertEqual(
                        contents.extractfile("./man/man1/vtcode.1").read(),
                        b"expected-release-man-page",
                    )
                self.assertTrue((root / f"vtcode-1.2.3-{target}.sha256").is_file())

    def test_release_waits_for_tag_without_hardcoded_repo(self):
        """Every build job polls the tag by name and derives the repo from the runner."""
        self.assertEqual(RELEASE.count("name: Wait for tag to exist"), 3)
        self.assertEqual(
            RELEASE.count("https://github.com/${GITHUB_REPOSITORY}.git"), 3
        )
        self.assertNotIn("vinhnx/VTCode.git", RELEASE)


if __name__ == "__main__":
    unittest.main()
