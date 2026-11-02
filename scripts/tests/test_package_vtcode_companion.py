"""Private companion package boundaries; network fixtures are not live-connection proof."""

import importlib.util
import json
import os
import shutil
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch
from urllib.error import HTTPError

SPEC = importlib.util.spec_from_file_location(
    "companion_package",
    Path(__file__).resolve().parents[1] / "package-vtcode-companion.py",
)
PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)

LAUNCHER_SPEC = importlib.util.spec_from_file_location(
    "companion_launcher", PACKAGE.SOURCE / "server" / "launch.py"
)
LAUNCHER = importlib.util.module_from_spec(LAUNCHER_SPEC)
LAUNCHER_SPEC.loader.exec_module(LAUNCHER)


class CompanionPackageTests(unittest.TestCase):
    def test_github_marketplace_manifest_has_no_machine_specific_launch_paths(self):
        catalog = json.loads(
            (PACKAGE.ROOT / ".agents/plugins/marketplace.json").read_text()
        )
        self.assertEqual(catalog["plugins"][0]["name"], "vtcode-companion")
        self.assertEqual(
            catalog["plugins"][0]["source"]["path"], "./plugins/vtcode-companion"
        )
        server = json.loads((PACKAGE.SOURCE / "mcp.json").read_text())["mcpServers"][
            "vtcode"
        ]
        self.assertEqual(server["type"], "stdio")
        self.assertEqual(server["command"], "python3")
        self.assertEqual(server["args"], ["${PLUGIN_ROOT}/server/launch.py"])
        self.assertNotIn("cwd", server)

    def test_github_launcher_uses_saved_user_binary_and_workspace(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "vtcode"
            binary.write_text("#!/bin/sh\nexit 0\n")
            binary.chmod(0o700)
            workspace = root / "workspace"
            workspace.mkdir()
            unrelated_cwd = root / "unrelated"
            unrelated_cwd.mkdir()
            config_dir = root / "config"
            config_dir.mkdir()
            (config_dir / "companion.json").write_text(
                json.dumps({"binary": str(binary), "workspace": str(workspace)})
            )
            original_directory = Path.cwd()
            try:
                os.chdir(unrelated_cwd)
                with (
                    patch.dict(os.environ, {"VTCODE_CONFIG": str(config_dir)}),
                    patch.object(LAUNCHER.sys, "argv", ["launch.py"]),
                ):
                    resolved_binary, resolved_workspace = LAUNCHER.configured_target()
            finally:
                os.chdir(original_directory)
            self.assertEqual(resolved_binary, binary.resolve())
            self.assertEqual(resolved_workspace, workspace.resolve())

    def test_github_launcher_requires_user_setup(self):
        with tempfile.TemporaryDirectory() as directory:
            config_dir = Path(directory)
            with (
                patch.dict(os.environ, {"VTCODE_CONFIG": str(config_dir)}),
                patch.object(LAUNCHER.sys, "argv", ["launch.py"]),
                self.assertRaisesRegex(ValueError, "Companion is not configured"),
            ):
                LAUNCHER.configured_target()

    def test_github_launcher_rejects_relative_saved_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            config_dir = Path(directory)
            (config_dir / "companion.json").write_text(
                json.dumps({"binary": "vtcode", "workspace": "."})
            )
            with (
                patch.dict(os.environ, {"VTCODE_CONFIG": str(config_dir)}),
                patch.object(LAUNCHER.sys, "argv", ["launch.py"]),
                self.assertRaisesRegex(ValueError, "absolute paths"),
            ):
                LAUNCHER.configured_target()

    def test_github_launcher_rejects_symlinked_user_configuration(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            real = root / "real.json"
            real.write_text(json.dumps({"binary": "/bin/vtcode", "workspace": "/tmp"}))
            link = root / "companion.json"
            link.symlink_to(real)
            with (
                patch.dict(os.environ, {"VTCODE_CONFIG": str(root)}),
                patch.object(LAUNCHER.sys, "argv", ["launch.py"]),
                self.assertRaisesRegex(ValueError, "symlink"),
            ):
                LAUNCHER.configured_target()

    def test_machine_specific_launcher_arguments_remain_supported(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "vtcode"
            binary.write_text("#!/bin/sh\nexit 0\n")
            binary.chmod(0o700)
            workspace = root / "workspace"
            workspace.mkdir()
            with patch.object(
                LAUNCHER.sys,
                "argv",
                ["launch.py", str(binary), str(workspace)],
            ):
                self.assertEqual(
                    LAUNCHER.configured_target(),
                    (binary.resolve(), workspace.resolve()),
                )

    def test_marketplace_resolves_plugin_and_preserves_fixed_launch_arguments(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "binary with spaces"
            binary.write_text("#!/bin/sh\nexit 0\n")
            binary.chmod(0o700)
            workspace = root / "workspace"
            workspace.mkdir()
            destination = root / "local catalog"
            PACKAGE.prepare_marketplace(binary, workspace, destination)
            catalog = json.loads(
                (destination / ".agents/plugins/marketplace.json").read_text()
            )
            self.assertEqual(catalog["name"], "vtcode-local")
            self.assertEqual(len(catalog["plugins"]), 1)
            entry = catalog["plugins"][0]
            plugin = destination / entry["source"]["path"]
            self.assertEqual(
                json.loads((plugin / "plugin.json").read_text())["name"], entry["name"]
            )
            server = json.loads((plugin / "mcp.json").read_text())["mcpServers"][
                "vtcode"
            ]
            self.assertEqual(
                server["args"],
                [
                    "${PLUGIN_ROOT}/server/launch.py",
                    str(binary.resolve()),
                    str(workspace.resolve()),
                ],
            )
            self.assertTrue((plugin / "skills/vtcode-companion/SKILL.md").is_file())
            self.assertTrue((plugin / "server/launch.py").is_file())
            self.assertEqual(
                (plugin / "assets/icon.png").read_bytes(),
                (PACKAGE.SOURCE / "assets/icon.png").read_bytes(),
            )
            before = {
                p.relative_to(destination): p.read_bytes()
                for p in destination.rglob("*")
                if p.is_file()
            }
            with self.assertRaisesRegex(ValueError, "already exists"):
                PACKAGE.prepare_marketplace(binary, workspace, destination)
            self.assertEqual(
                before,
                {
                    p.relative_to(destination): p.read_bytes()
                    for p in destination.rglob("*")
                    if p.is_file()
                },
            )

    def test_invalid_local_inputs_do_not_create_marketplace(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            destination = root / "catalog"
            with self.assertRaises(OSError):
                PACKAGE.prepare_marketplace(root / "missing", root, destination)
            self.assertFalse(destination.exists())
            link = root / "linked"
            link.symlink_to(root / "absent", target_is_directory=True)
            with self.assertRaisesRegex(ValueError, "already exists"):
                PACKAGE.prepare_marketplace(root / "missing", root, link)
            self.assertTrue(link.is_symlink())

    def test_local_archive_binds_exact_binary_and_workspace_without_credentials(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "VT Code executable"
            binary.write_text("#!/bin/sh\nexit 0\n")
            binary.chmod(0o700)
            workspace = root / "workspace with spaces"
            workspace.mkdir()
            output = root / "local.zip"
            with patch.dict(
                os.environ, {"VTCODE_COMPANION_ACCESS_TOKEN": "secret-not-for-zip"}
            ):
                PACKAGE.package_local(binary, workspace, output)
            with zipfile.ZipFile(output) as archive:
                server = json.loads(archive.read("mcp.json"))["mcpServers"]["vtcode"]
                self.assertEqual(server["type"], "stdio")
                self.assertEqual(server["command"], "python3")
                self.assertEqual(
                    server["args"],
                    [
                        "${PLUGIN_ROOT}/server/launch.py",
                        str(binary.resolve()),
                        str(workspace.resolve()),
                    ],
                )
                self.assertEqual(server["cwd"], "${PLUGIN_ROOT}")
                self.assertEqual(
                    set(archive.namelist()),
                    {
                        "plugin.json",
                        "mcp.json",
                        "skills/vtcode-companion/SKILL.md",
                        "server/launch.py",
                        "assets/icon.png",
                        "assets/banner.png",
                    },
                )
                self.assertNotIn("env", server)
                self.assertNotIn("headers", server)
                self.assertNotIn("url", server)
                self.assertNotIn(
                    b"secret-not-for-zip",
                    b"".join(archive.read(name) for name in archive.namelist()),
                )
                manifest = json.loads(archive.read("plugin.json"))
                interface = manifest["extensions"]["com.openai"]["interface"]
                self.assertEqual(interface["displayName"], "VT Code Companion")
                self.assertEqual(manifest["version"], "0.1.1")
                self.assertEqual(interface["logo"], "./assets/icon.png")
                self.assertEqual(interface["composerIcon"], "./assets/icon.png")
                self.assertEqual(interface["screenshots"], ["./assets/banner.png"])
                resources = PACKAGE.ROOT / "resources"
                self.assertEqual(
                    archive.read("assets/icon.png"),
                    (resources / "icons/vtcode-app-dynamic-512.png").read_bytes(),
                )
                self.assertEqual(
                    archive.read("assets/banner.png"),
                    (resources / "logo/vtcode_5.png").read_bytes(),
                )

    def test_referenced_images_reject_missing_escaping_symlink_and_wrong_shape(self):
        for reference in [
            "./assets/missing.png",
            "./assets/../../outside.png",
            "./assets/linked.png",
            "./assets/banner.png",
        ]:
            with (
                self.subTest(reference=reference),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory)
                source = root / "plugin"
                shutil.copytree(PACKAGE.SOURCE, source)
                (source / "assets/linked.png").symlink_to(source / "assets/icon.png")
                manifest_path = source / "plugin.json"
                manifest = json.loads(manifest_path.read_text())
                manifest["extensions"]["com.openai"]["interface"]["logo"] = reference
                manifest_path.write_text(json.dumps(manifest))
                with (
                    patch.object(PACKAGE, "SOURCE", source),
                    self.assertRaises((OSError, ValueError)),
                ):
                    PACKAGE.package(
                        "https://mcp.example.test/mcp",
                        "https://mcp.example.test/sse/",
                        root / "out.zip",
                    )
                self.assertFalse((root / "out.zip").exists())

    def test_local_archive_rejects_missing_binary_and_nondirectory_workspace(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(FileNotFoundError):
                PACKAGE.package_local(root / "missing", root, root / "local.zip")
            binary = root / "binary"
            binary.write_text("not executable")
            with self.assertRaisesRegex(ValueError, "executable"):
                PACKAGE.package_local(binary, root, root / "local.zip")
            binary.chmod(0o700)
            with self.assertRaisesRegex(ValueError, "directory"):
                PACKAGE.package_local(binary, binary, root / "local.zip")

    def responses(self):
        tools = [
            {
                "name": name,
                "annotations": {"readOnlyHint": False, "destructiveHint": True},
            }
            for name in sorted(PACKAGE.EXPECTED_TOOLS)
        ]
        return [
            {
                "resource": "https://mcp.example.test/sse/",
                "authorization_servers": ["https://auth.example.test"],
            },
            HTTPError("https://mcp.example.test/mcp", 401, "Unauthorized", {}, None),
            {"result": {"capabilities": {"tools": {}}}},
            {"result": {"tools": tools}},
        ]

    def test_requires_authenticated_discovery_and_all_five_tools(self):
        with patch.object(
            PACKAGE, "request_json", side_effect=self.responses()
        ) as requests:
            resource = PACKAGE.verify_endpoint(
                "https://mcp.example.test/mcp", "fixture-access-token"
            )
        self.assertEqual(resource, "https://mcp.example.test/sse/")
        self.assertIsNone(requests.call_args_list[1].kwargs.get("token"))
        self.assertEqual(requests.call_args_list[2].args[2], "fixture-access-token")

    def test_rejects_public_execution_endpoint(self):
        responses = self.responses()
        responses[1] = {"result": {}}
        with (
            patch.object(PACKAGE, "request_json", side_effect=responses),
            self.assertRaisesRegex(ValueError, "unauthenticated coding service"),
        ):
            PACKAGE.verify_endpoint("https://mcp.example.test/mcp", "fixture-token")

    def test_rejects_incomplete_tools_or_unsafe_action_hints(self):
        for missing_tools in [True, False]:
            responses = self.responses()
            if missing_tools:
                responses[-1]["result"]["tools"].pop()
            else:
                for tool in responses[-1]["result"]["tools"]:
                    tool["annotations"]["readOnlyHint"] = True
            with (
                patch.object(PACKAGE, "request_json", side_effect=responses),
                self.assertRaises(ValueError),
            ):
                PACKAGE.verify_endpoint("https://mcp.example.test/mcp", "fixture-token")

    def test_rejects_foreign_oauth_resource_and_bad_urls(self):
        responses = self.responses()
        responses[0]["resource"] = "https://foreign.example.test/mcp"
        with (
            patch.object(PACKAGE, "request_json", side_effect=responses),
            self.assertRaisesRegex(ValueError, "origin"),
        ):
            PACKAGE.verify_endpoint("https://mcp.example.test/mcp", "fixture-token")
        for url in [
            "http://localhost/mcp",
            "https://user:secret@example.test/mcp",
            "https://example.test/mcp?token=x",
        ]:
            with self.assertRaises(ValueError):
                PACKAGE.https_url(url)

    def test_rejects_malformed_tool_records_and_annotations_without_attribute_errors(
        self,
    ):
        for annotations in [None, [], "read-only"]:
            responses = self.responses()
            tool = next(
                tool
                for tool in responses[-1]["result"]["tools"]
                if tool["name"] == "start_task"
            )
            tool["annotations"] = annotations
            with (
                patch.object(PACKAGE, "request_json", side_effect=responses),
                self.assertRaisesRegex(TypeError, "annotations must be an object"),
            ):
                PACKAGE.verify_endpoint("https://mcp.example.test/mcp", "fixture-token")
        for extra in [None, "unexpected tool", {"name": []}, {"name": "search"}]:
            responses = self.responses()
            responses[-1]["result"]["tools"].append(extra)
            with (
                patch.object(PACKAGE, "request_json", side_effect=responses),
                self.assertRaises((TypeError, ValueError)),
            ):
                PACKAGE.verify_endpoint("https://mcp.example.test/mcp", "fixture-token")

    def test_archive_contains_only_declared_components_and_no_credentials(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "companion.zip"
            PACKAGE.package(
                "https://mcp.example.test/mcp", "https://mcp.example.test/sse/", output
            )
            with zipfile.ZipFile(output) as archive:
                self.assertEqual(
                    set(archive.namelist()),
                    {
                        "plugin.json",
                        "mcp.json",
                        "skills/vtcode-companion/SKILL.md",
                        "assets/icon.png",
                        "assets/banner.png",
                    },
                )
                mcp = json.loads(archive.read("mcp.json"))
                server = mcp["mcpServers"]["vtcode"]
                self.assertEqual(server["url"], "https://mcp.example.test/mcp")
                self.assertEqual(
                    server["extensions"]["com.openai"]["auth"]["type"], "oauth"
                )
                self.assertNotIn("headers", server)
                self.assertNotIn("apps", json.loads(archive.read("plugin.json")))
            with self.assertRaisesRegex(ValueError, "already exists"):
                PACKAGE.package(
                    "https://mcp.example.test/mcp",
                    "https://mcp.example.test/sse/",
                    output,
                )

    def test_archive_cannot_be_written_inside_plugin(self):
        with self.assertRaisesRegex(ValueError, "outside"):
            PACKAGE.package(
                "https://mcp.example.test/mcp",
                "https://mcp.example.test/sse/",
                PACKAGE.SOURCE / "out.zip",
            )

    def test_redirects_never_forward_bearer_credentials(self):
        self.assertIsNone(
            PACKAGE.NoRedirect().redirect_request(
                None, None, 302, "Found", {}, "https://foreign.test"
            )
        )


if __name__ == "__main__":
    unittest.main()
