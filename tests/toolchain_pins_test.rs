#![allow(
    missing_docs,
    clippy::expect_used,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
//! Guards that stable/MSRV installations agree with the repository Rust pin.
//!
//! `rust-toolchain.toml` is the single source of truth. Any other manifest or
//! build file that pins a different version silently reintroduces a second
//! toolchain: `cargo` would activate one compiler while CI, Docker, Clippy, or
//! `cargo-msrv` reason about another. This test fails loudly when a pin drifts,
//! complementing `scripts/check_compatibility.sh --msrv` (the human-facing
//! report) with an always-on `cargo nextest run` guard.

use std::fs;
use std::path::{Path, PathBuf};

/// Directories whose Cargo manifests are intentionally not pinned to the
/// workspace channel (vendored forks, fuzz harnesses, editor extensions) or
/// that hold local runtime state (archived workspace snapshots, logs, memory).
const EXCLUDED_DIRS: &[&str] = &[
    "target",
    ".worktrees",
    ".vtcode",
    "patches",
    "fuzz",
    "extensions",
    ".git",
    "node_modules",
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Extract the value of a `key = "value"` TOML line, ignoring whitespace.
fn toml_string_value(contents: &str, key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let trimmed = line.trim_start();
        let rest = trimmed.strip_prefix(key)?.trim_start();
        let rest = rest.strip_prefix('=')?.trim_start();
        Some(rest.trim_matches(|c| c == '"' || c == '\'').to_string())
    })
}

/// The pinned channel from `rust-toolchain.toml`.
fn pinned_toolchain(root: &Path) -> String {
    let manifest = fs::read_to_string(root.join("rust-toolchain.toml")).expect("read rust-toolchain.toml");
    let channel = toml_string_value(&manifest, "channel").expect("rust-toolchain.toml must declare `channel`");
    assert!(
        channel.split('.').all(|part| part.chars().all(|c| c.is_ascii_digit())),
        "rust-toolchain.toml `channel` must be a concrete x.y.z version, got {channel:?}"
    );
    channel
}

fn collect_cargo_manifests(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if EXCLUDED_DIRS.contains(&name.as_ref()) {
                continue;
            }
            collect_cargo_manifests(&path, found);
        } else if file_type.is_file() && entry.file_name() == "Cargo.toml" {
            found.push(path);
        }
    }
}

/// Every workspace manifest that pins `rust-version` directly.
fn rust_version_pins(root: &Path) -> Vec<(PathBuf, String)> {
    let mut manifests = Vec::new();
    collect_cargo_manifests(root, &mut manifests);

    let mut pins = Vec::new();
    for manifest in manifests {
        let contents = fs::read_to_string(&manifest).expect("read Cargo.toml");
        // `rust-version.workspace = true` inherits the workspace pin and is
        // checked through the workspace manifest itself.
        if contents.contains("rust-version.workspace = true") {
            continue;
        }
        if let Some(version) = toml_string_value(&contents, "rust-version") {
            pins.push((manifest, version));
        }
    }
    assert!(pins.len() > 10, "expected the workspace to pin rust-version in many manifests");
    pins
}

#[derive(Debug)]
struct ToolchainPin {
    location: String,
    version: String,
}

/// Inspect declared workflow pins without assuming a job or workflow filename.
/// Nightly checks intentionally exercise a separate channel, not the MSRV pin.
fn workflow_toolchain_pins(path: &Path, contents: &str) -> Result<Vec<ToolchainPin>, String> {
    let lines = contents.lines().collect::<Vec<_>>();
    let mut pins = Vec::new();
    for (line_index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let uses = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        if !uses.starts_with("uses: dtolnay/rust-toolchain@") {
            continue;
        }
        let indentation = line.len() - trimmed.len();
        let version = lines
            .iter()
            .skip(line_index + 1)
            .take_while(|line| {
                let next = line.trim_start();
                next.is_empty()
                    || next.starts_with('#')
                    || (line.len() - next.len() >= indentation
                        && !(next.starts_with("- ") && line.len() - next.len() == indentation))
            })
            .find_map(|line| line.trim().strip_prefix("toolchain:"));
        let location = format!("{}:{} toolchain", path.display(), line_index + 1);
        let version = version
            .unwrap_or_default()
            .split('#')
            .next()
            .unwrap_or_default()
            .trim()
            .trim_matches(['\'', '"']);
        if version.is_empty() {
            return Err(format!("{location} must explicitly select the workspace pin"));
        }
        if version != "nightly" && !version.starts_with("nightly-") {
            pins.push(ToolchainPin { location, version: version.to_owned() });
        }
    }
    Ok(pins)
}

/// Toolchain versions embedded outside Cargo manifests.
fn non_manifest_pins(root: &Path) -> Vec<ToolchainPin> {
    let mut pins = Vec::new();

    let clippy = fs::read_to_string(root.join("clippy.toml")).expect("read clippy.toml");
    pins.push(ToolchainPin {
        location: "clippy.toml msrv".into(),
        version: toml_string_value(&clippy, "msrv").expect("clippy.toml must set `msrv`"),
    });

    let dockerfile = fs::read_to_string(root.join("Dockerfile.build")).expect("read Dockerfile.build");
    let rust_base = dockerfile
        .lines()
        .find_map(|line| line.strip_prefix("FROM rust:"))
        .expect("Dockerfile.build must start from a rust image");
    let docker_version = rust_base
        .split_once('-')
        .map_or(rust_base, |(version, _)| version)
        .trim()
        .to_string();
    pins.push(ToolchainPin {
        location: "Dockerfile.build FROM rust".into(),
        version: docker_version,
    });

    for entry in fs::read_dir(root.join(".github/workflows")).expect("read active workflows") {
        let path = entry.expect("read workflow entry").path();
        if !path.is_file() || !matches!(path.extension().and_then(|extension| extension.to_str()), Some("yml" | "yaml"))
        {
            continue;
        }
        let contents = fs::read_to_string(&path).expect("read workflow");
        let relative = path.strip_prefix(root).expect("workflow inside workspace");
        pins.extend(workflow_toolchain_pins(relative, &contents).expect("explicit workflow toolchain pins"));
    }

    pins
}

#[test]
fn every_rust_toolchain_pin_agrees() {
    let root = workspace_root();
    let pinned = pinned_toolchain(&root);

    let mut mismatches = rust_version_pins(&root)
        .into_iter()
        .filter(|(_, version)| version != &pinned)
        .map(|(path, version)| {
            let relative = path.strip_prefix(&root).unwrap_or(&path);
            format!("{} pins {version}", relative.display())
        })
        .collect::<Vec<_>>();

    mismatches.extend(
        non_manifest_pins(&root)
            .into_iter()
            .filter(|pin| pin.version != pinned)
            .map(|pin| format!("{} = {}", pin.location, pin.version)),
    );

    assert!(
        mismatches.is_empty(),
        "rust-toolchain.toml pins {pinned}, but these disagree:\n  {}",
        mismatches.join("\n  ")
    );
}

#[test]
fn workspace_package_inherits_the_pinned_toolchain() {
    let root = workspace_root();
    let pinned = pinned_toolchain(&root);
    let workspace_manifest = fs::read_to_string(root.join("Cargo.toml")).expect("read workspace Cargo.toml");

    let rust_version =
        toml_string_value(&workspace_manifest, "rust-version").expect("workspace.package must declare rust-version");
    assert_eq!(rust_version, pinned, "workspace.package rust-version must match the pinned toolchain");

    let edition = toml_string_value(&workspace_manifest, "edition").expect("workspace.package must declare edition");
    assert_eq!(edition, "2024", "the workspace targets Rust edition 2024");
}

#[test]
fn workflow_pin_guard_follows_active_names_and_detects_drift() {
    let path = Path::new(".github/workflows/renamed-build.yaml");
    let pins = workflow_toolchain_pins(
        path,
        r#"
steps:
  - uses: dtolnay/rust-toolchain@pinned-action
    with:
      toolchain: '1.99.1' # workspace pin
  - name: another installer
    uses: dtolnay/rust-toolchain@pinned-action
    with:
      toolchain: 1.97.0
  - uses: dtolnay/rust-toolchain@pinned-action
    with:
      toolchain: nightly-2026-01-01
"#,
    )
    .unwrap();
    assert_eq!(pins.len(), 2);
    assert_eq!(pins[0].version, "1.99.1");
    let mismatches = pins.iter().filter(|pin| pin.version != "1.99.1").collect::<Vec<_>>();
    assert_eq!(mismatches.len(), 1);
    assert_eq!(mismatches[0].version, "1.97.0");
    assert!(mismatches[0].location.contains("renamed-build.yaml:7"));
}

#[test]
fn workflow_installers_cannot_borrow_a_later_steps_pin_or_default_to_stable() {
    let path = Path::new("renamed.yml");
    let missing = workflow_toolchain_pins(
        path,
        "  - uses: dtolnay/rust-toolchain@action\n  - name: another step\n    with:\n      toolchain: 1.99.1\n",
    );
    assert!(missing.unwrap_err().contains("must explicitly select"));
    let floating =
        workflow_toolchain_pins(path, "  - uses: dtolnay/rust-toolchain@action\n    with:\n      toolchain: stable\n")
            .unwrap();
    assert_eq!(floating[0].version, "stable");
    assert_ne!(floating[0].version, "1.99.1");
}
