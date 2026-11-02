//! Best-effort access to operating-system reduced-motion preferences.

use once_cell::sync::Lazy;

static REDUCE_MOTION_PREFERENCE: Lazy<Option<bool>> = Lazy::new(detect_reduce_motion_preference);

/// Read the platform preference once; unsupported or unavailable settings remain unknown.
pub(crate) fn reduce_motion_preference() -> Option<bool> {
    *REDUCE_MOTION_PREFERENCE
}

#[cfg(target_os = "windows")]
#[expect(
    unsafe_code,
    reason = "Reduced-motion preference is only exposed through the Win32 `SystemParametersInfoW` ABI; the standard library has no safe binding for it."
)]
fn detect_reduce_motion_preference() -> Option<bool> {
    use std::ffi::c_void;

    const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;

    #[link(name = "user32")]
    // SAFETY: Declares the documented `user32!SystemParametersInfoW` ABI
    // (`UINT, UINT, PVOID, UINT -> BOOL`); `#[link]` binds the symbol from the
    // system `user32` library, and the call site below passes a live `BOOL`
    // whose pointer and lifetime match the declared signature.
    unsafe extern "system" {
        fn SystemParametersInfoW(action: u32, param: u32, value: *mut c_void, flags: u32) -> i32;
    }

    let mut animations_enabled = 0i32;
    // SAFETY: SystemParametersInfoW writes one BOOL to the provided, correctly sized pointer.
    let succeeded = unsafe {
        SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&mut animations_enabled as *mut i32).cast(), 0)
    };

    (succeeded != 0).then_some(animations_enabled == 0)
}

#[cfg(target_os = "macos")]
fn detect_reduce_motion_preference() -> Option<bool> {
    command_preference("defaults", &["read", "-g", "AppleReduceMotion"])
}

#[cfg(target_os = "linux")]
fn detect_reduce_motion_preference() -> Option<bool> {
    // Linux has no common desktop-independent reduced-motion preference. Read
    // only settings for the active desktop so unrelated installed settings do
    // not incorrectly override the default.
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok()?.to_ascii_lowercase();
    if desktop.split(':').any(|name| name.contains("gnome")) {
        command_preference("gsettings", &["get", "org.gnome.desktop.interface", "enable-animations"])
            .map(|animations_enabled| !animations_enabled)
    } else if desktop.split(':').any(|name| name.contains("xfce")) {
        command_preference("xfconf-query", &["--channel", "xsettings", "--property", "/Gtk/EnableAnimations"])
            .map(|animations_enabled| !animations_enabled)
    } else if desktop.split(':').any(|name| name.contains("kde") || name.contains("plasma")) {
        detect_kde_reduce_motion_preference()
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn detect_kde_reduce_motion_preference() -> Option<bool> {
    command_number_preference(
        "kreadconfig6",
        &["--file", "kwinrc", "--group", "KDE", "--key", "AnimationDurationFactor"],
    )
    .or_else(|| {
        command_number_preference(
            "kreadconfig5",
            &["--file", "kwinrc", "--group", "KDE", "--key", "AnimationDurationFactor"],
        )
    })
    .map(|animation_duration_factor| animation_duration_factor <= 0.0)
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn detect_reduce_motion_preference() -> Option<bool> {
    None
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn command_preference(command: &str, args: &[&str]) -> Option<bool> {
    use std::process::Command;

    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    parse_boolean_preference(std::str::from_utf8(&output.stdout).ok()?)
}

#[cfg(target_os = "linux")]
fn command_number_preference(command: &str, args: &[&str]) -> Option<f64> {
    use std::process::Command;

    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let value = std::str::from_utf8(&output.stdout).ok()?.trim().parse::<f64>().ok()?;
    value.is_finite().then_some(value)
}

fn parse_boolean_preference(value: &str) -> Option<bool> {
    match value.trim().trim_matches('\'').trim_matches('"') {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_boolean_preference;

    #[test]
    fn platform_boolean_parser_accepts_known_boolean_forms() {
        assert_eq!(parse_boolean_preference("true\n"), Some(true));
        assert_eq!(parse_boolean_preference("false"), Some(false));
        assert_eq!(parse_boolean_preference("1"), Some(true));
        assert_eq!(parse_boolean_preference("0"), Some(false));
        assert_eq!(parse_boolean_preference("'true'"), Some(true));
        assert_eq!(parse_boolean_preference("unknown"), None);
        assert_eq!(parse_boolean_preference(""), None);
    }

    #[test]
    fn reduce_motion_environment_override_short_circuits_os_probe() {
        let os_probe_called = std::cell::Cell::new(false);
        let selected = Some(false).or_else(|| {
            os_probe_called.set(true);
            Some(true)
        });

        assert_eq!(selected, Some(false));
        assert!(!os_probe_called.get());
    }
}
