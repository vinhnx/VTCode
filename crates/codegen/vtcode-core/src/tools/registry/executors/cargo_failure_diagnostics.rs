use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{Value, json};

static CARGO_SELECTOR_ERROR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^error: no test target named `([^`]+)` in `([^`]+)` package$")
        .expect("cargo selector error regex is valid")
});

static CARGO_FAIL_LINE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^\s*FAIL \[[^\]]+\](?: \(\s*\d+/\d+\))? ([^\s]+) ([^\s]+)\s*$")
        .expect("cargo fail-line regex is valid")
});

static CARGO_THREAD_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"thread '([^']+)'").expect("cargo thread regex is valid"));

static CARGO_PANIC_LOCATION_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(.+):(\d+):\d+:$").expect("cargo panic location regex is valid"));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CargoTestCommandKind {
    Test,
    Nextest,
}

fn first_command_token(command: &str) -> Option<String> {
    shell_words::split(command)
        .ok()
        .and_then(|parts| parts.into_iter().next())
        .filter(|part| !part.trim().is_empty())
}

fn cargo_test_command_kind(command: &str) -> Option<CargoTestCommandKind> {
    let parts = shell_words::split(command).ok()?;
    match parts.as_slice() {
        [cargo, test, ..] if cargo == "cargo" && test == "test" => Some(CargoTestCommandKind::Test),
        [cargo, nextest, run, ..] if cargo == "cargo" && nextest == "nextest" && run == "run" => {
            Some(CargoTestCommandKind::Nextest)
        }
        _ => None,
    }
}

fn cargo_package_from_command(command: &str) -> Option<String> {
    let parts = shell_words::split(command).ok()?;
    let mut iter = parts.iter();
    while let Some(part) = iter.next() {
        match part.as_str() {
            "-p" | "--package" => {
                let value = iter.next()?.trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn infer_cargo_test_binary_kind(
    source_file: Option<&str>,
    test_fqname: Option<&str>,
) -> &'static str {
    let normalized_path = source_file.map(|path| path.replace('\\', "/"));
    if let Some(path) = normalized_path.as_deref() {
        if path.starts_with("tests/") || path.contains("/tests/") {
            return "integration";
        }
        if path.starts_with("src/") || path.contains("/src/") {
            return "unit";
        }
    }

    if test_fqname.is_some_and(|name| name.contains("::")) {
        "unit"
    } else {
        "unknown"
    }
}

pub(super) fn cargo_test_rerun_hint(
    command_kind: CargoTestCommandKind,
    package: &str,
    binary_kind: &str,
    test_fqname: &str,
) -> String {
    match command_kind {
        CargoTestCommandKind::Nextest => format!("cargo nextest run -p {package} {test_fqname}"),
        CargoTestCommandKind::Test if binary_kind == "unit" => {
            format!("cargo test -p {package} --lib {test_fqname} -- --nocapture")
        }
        CargoTestCommandKind::Test => {
            format!("cargo test -p {package} {test_fqname} -- --nocapture")
        }
    }
}

pub(super) fn cargo_selector_error_diagnostics(
    command_kind: CargoTestCommandKind,
    command: &str,
    output: &str,
) -> Option<Value> {
    let captures = CARGO_SELECTOR_ERROR_RE.captures(output)?;
    let requested_target = captures.get(1)?.as_str().trim();
    let package = captures.get(2)?.as_str().trim();
    if requested_target.is_empty() || package.is_empty() {
        return None;
    }

    let validation_hint =
        format!("cargo test -p {package} --lib -- --list | rg '{requested_target}'");
    let rerun_hint = match command_kind {
        CargoTestCommandKind::Nextest => {
            format!("cargo nextest run -p {package} {requested_target}")
        }
        CargoTestCommandKind::Test => {
            format!("cargo test -p {package} --lib {requested_target} -- --nocapture")
        }
    };

    Some(json!({
        "kind": "cargo_test_selector_error",
        "package": package,
        "binary_kind": "test_target_selector",
        "requested_test_target": requested_target,
        "selector_error": true,
        "validation_hint": validation_hint,
        "rerun_hint": rerun_hint,
        "critical_note": format!(
            "Cargo rejected `{requested_target}` as a test target. This usually means a unit test name was passed to `--test`."
        ),
        "next_action": format!(
            "Validate whether `{requested_target}` is a unit test with: {validation_hint}"
        ),
        "command": command,
    }))
}

fn cargo_failed_test_and_package(output: &str) -> (Option<String>, Option<String>) {
    for line in output.lines().rev() {
        let trimmed = line.trim();
        if !trimmed.starts_with("FAIL [") {
            continue;
        }
        if let Some(captures) = CARGO_FAIL_LINE_RE.captures(trimmed) {
            let package = captures.get(1).map(|value| value.as_str().trim());
            let test_fqname = captures.get(2).map(|value| value.as_str().trim());
            if let (Some(package), Some(test_fqname)) = (package, test_fqname)
                && !package.is_empty()
                && !test_fqname.is_empty()
            {
                return (Some(package.to_string()), Some(test_fqname.to_string()));
            }
        }
    }

    let test_fqname = CARGO_THREAD_RE.captures_iter(output).find_map(|captures| {
        let candidate = captures.get(1)?.as_str().trim();
        (!candidate.is_empty()).then(|| candidate.to_string())
    });
    (None, test_fqname)
}

fn cargo_panic_location_and_message(output: &str) -> (Option<String>, Option<u64>, Option<String>) {
    let lines: Vec<&str> = output.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let Some((_, location)) = trimmed.split_once(" panicked at ") else {
            continue;
        };
        let Some(captures) = CARGO_PANIC_LOCATION_RE.captures(location) else {
            continue;
        };

        let source_file = captures
            .get(1)
            .map(|value| value.as_str().trim().to_string());
        let source_line = captures
            .get(2)
            .and_then(|value| value.as_str().parse::<u64>().ok());
        let panic_message = lines.iter().skip(index + 1).find_map(|candidate| {
            let trimmed = candidate.trim();
            if trimmed.is_empty() {
                return None;
            }
            if trimmed.starts_with("note:") || trimmed.starts_with("stack backtrace:") {
                return None;
            }
            Some(trimmed.to_string())
        });
        return (source_file, source_line, panic_message);
    }

    (None, None, None)
}

pub(super) fn cargo_test_failure_diagnostics(
    command: &str,
    output: &str,
    exit_code: Option<i32>,
) -> Option<Value> {
    if exit_code == Some(0) {
        return None;
    }

    let command_kind = cargo_test_command_kind(command)?;
    if let Some(diagnostics) = cargo_selector_error_diagnostics(command_kind, command, output) {
        return Some(diagnostics);
    }

    let (package_from_output, test_fqname) = cargo_failed_test_and_package(output);
    let (source_file, source_line, panic_message) = cargo_panic_location_and_message(output);
    let package = package_from_output.or_else(|| cargo_package_from_command(command))?;
    let test_fqname = test_fqname?;
    let binary_kind =
        infer_cargo_test_binary_kind(source_file.as_deref(), Some(test_fqname.as_str()));
    let rerun_hint = cargo_test_rerun_hint(command_kind, &package, binary_kind, &test_fqname);

    Some(json!({
        "kind": "cargo_test_failure",
        "package": package,
        "binary_kind": binary_kind,
        "test_fqname": test_fqname,
        "panic": panic_message,
        "source_file": source_file,
        "source_line": source_line,
        "rerun_hint": rerun_hint,
        "critical_note": "Cargo reported a concrete failing test with a panic location.",
        "next_action": format!("Rerun the failing test directly with: {rerun_hint}"),
        "command": command,
    }))
}

pub(super) fn attach_failure_diagnostics_metadata(response: &mut Value, diagnostics: &Value) {
    if let Some(obj) = response.as_object_mut() {
        for key in [
            "package",
            "binary_kind",
            "test_fqname",
            "panic",
            "source_file",
            "source_line",
            "selector_error",
            "validation_hint",
            "rerun_hint",
            "critical_note",
            "next_action",
        ] {
            if let Some(value) = diagnostics.get(key) {
                obj.insert(key.to_string(), value.clone());
            }
        }
        obj.insert("failure_diagnostics".to_string(), diagnostics.clone());
    }
}

pub(super) fn attach_exec_recovery_guidance(
    response: &mut Value,
    command: &str,
    exit_code: Option<i32>,
) {
    if exit_code != Some(127) {
        return;
    }

    let command_name = first_command_token(command).unwrap_or_else(|| "command".to_string());
    response["critical_note"] = json!(format!("Command `{command_name}` was not found in PATH."));
    response["next_action"] =
        json!("Check the command name or install the missing binary, then rerun the command.");
}
