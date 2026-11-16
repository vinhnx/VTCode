//! Updater tests: version parsing, install-source detection, startup
//! checks, and the release-notes announcement policy.

use super::release_notes::record_version_notes_seen;
use super::*;
use std::path::Path;

#[test]
fn test_version_parsing() {
    let updater = Updater::new("0.58.4").expect("updater");
    assert_eq!(updater.current_version().major, 0);
    assert_eq!(updater.current_version().minor, 58);
    assert_eq!(updater.current_version().patch, 4);
}

#[test]
fn test_install_source_detection() {
    assert_eq!(
        install_source::detect_install_source_from_path(Path::new("/opt/homebrew/Cellar/vtcode/0.1/bin/vtcode")),
        InstallSource::Homebrew
    );
    assert_eq!(
        install_source::detect_install_source_from_path(Path::new("/Users/dev/.cargo/bin/vtcode")),
        InstallSource::Cargo
    );
    assert_eq!(
        install_source::detect_install_source_from_path(Path::new("/usr/local/bin/vtcode")),
        InstallSource::Standalone
    );
}

#[test]
fn startup_update_check_respects_disabled_interval() {
    let updater = Updater {
        current_version: Version::parse("0.111.0").expect("version"),
        config: UpdateConfig { check_interval_hours: 0, ..UpdateConfig::default() },
    };

    let check = updater.startup_update_check().expect("startup check");
    assert!(check.cached_notice.is_none());
    assert!(!check.should_refresh);
}

#[test]
fn startup_update_check_respects_pinned_version() {
    let mut config = UpdateConfig::default();
    config.set_pin(Version::parse("0.111.0").expect("version"), None, false);
    let updater = Updater {
        current_version: Version::parse("0.111.0").expect("version"),
        config,
    };

    let check = updater.startup_update_check().expect("startup check");
    assert!(check.cached_notice.is_none());
    assert!(!check.should_refresh);
}

#[test]
fn startup_update_check_suppresses_dismissed_version() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("temp dir");
    let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

    let dismissed = Version::parse("0.113.0").expect("dismissed");
    cache::record_successful_check(Some(&dismissed), true).expect("write cache");
    cache::record_dismissed_version(&dismissed).expect("record dismissal");

    let updater = Updater {
        current_version: Version::parse("0.111.0").expect("current"),
        config: UpdateConfig::default(),
    };

    let check = updater.startup_update_check().expect("startup check");
    assert!(check.cached_notice.is_none(), "Dismissed version should not produce a cached notice");

    cache::set_cache_dir_override_for_tests(previous);
}

#[test]
fn startup_update_check_shows_undismissed_newer_version() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("temp dir");
    let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

    let latest = Version::parse("0.114.0").expect("latest");
    cache::record_successful_check(Some(&latest), true).expect("write cache");
    // Only dismissed 0.113.0, not 0.114.0 — notice should appear
    cache::record_dismissed_version(&Version::parse("0.113.0").expect("older")).expect("record dismissal");

    let updater = Updater {
        current_version: Version::parse("0.111.0").expect("current"),
        config: UpdateConfig::default(),
    };

    let check = updater.startup_update_check().expect("startup check");
    assert!(check.cached_notice.is_some(), "Newer, non-dismissed version should produce a cached notice");

    cache::set_cache_dir_override_for_tests(previous);
}

#[test]
fn explicit_check_records_the_fetched_release_into_the_cache() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("temp dir");
    let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

    // An explicit check (`vtcode update`, `/update`) fetched a newer
    // release: the cache must end up holding its version and notes, the
    // same state the background refresh leaves behind. The inline managed
    // update relaunches off this cache, so a stale one defers the notes
    // to a second start (the N vs N+1 regression).
    let current = Version::parse(env!("CARGO_PKG_VERSION")).expect("current version");
    let fetched = Version::parse("99.0.0").expect("fetched version");
    record_fetched_release(
        &current,
        &UpdateInfo {
            version: fetched.clone(),
            release_notes: "### Highlights\n- Fetched feature (abc1234)\n".into(),
        },
    );

    let snapshot = cache::read_snapshot().expect("snapshot");
    assert_eq!(snapshot.latest_version, Some(fetched.clone()));
    assert!(snapshot.latest_was_newer, "the fetched release is newer than the running one");
    let served = cache::latest_release_info().expect("the fetched release is cached");
    assert_eq!(served.0, fetched);
    assert!(served.1.contains("Fetched feature"), "the fetched notes are cached");

    // Re-checking while already current records `latest_was_newer=false`
    // — the exact state the first launch after an update must find.
    record_fetched_release(
        &current,
        &UpdateInfo {
            version: current.clone(),
            release_notes: "### Highlights\n- Current feature (def5678)\n".into(),
        },
    );
    let snapshot = cache::read_snapshot().expect("snapshot");
    assert_eq!(snapshot.latest_version, Some(current));
    assert!(!snapshot.latest_was_newer);

    cache::set_cache_dir_override_for_tests(previous);
}

#[test]
fn preflight_fetch_backfills_the_first_launch_banner() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("temp dir");
    let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));
    let previous_preflight =
        preflight::set_preflight_latest_release_for_tests(None).map(|(version, _)| (version, String::new()));

    // A manual `brew upgrade` leaves the cache stale: the first launch
    // after the upgrade reads no notes for the running version. The
    // spawned preflight fetch (which lands a moment later) must backfill
    // the banner instead of deferring the notes to a second launch.
    let stale = Version::parse("0.1.0").expect("stale version");
    cache::record_successful_check(Some(&stale), true).expect("write stale cache");

    let current = Version::parse(env!("CARGO_PKG_VERSION")).expect("current version");
    preflight::set_preflight_latest_release_for_tests(Some((
        current.clone(),
        "### Highlights\n- Fresh feature (abc1234)\n".to_string(),
    )));

    let highlights = cached_current_release_highlights().expect("preflight fetch serves the current banner");
    assert_eq!(highlights.0, current);
    assert_eq!(highlights.1, vec!["Fresh feature".to_string()]);

    preflight::set_preflight_latest_release_for_tests(previous_preflight);
    cache::set_cache_dir_override_for_tests(previous);
}

#[test]
fn a_newer_preflight_release_never_leaks_into_the_current_banner() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("temp dir");
    let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));
    let previous_preflight =
        preflight::set_preflight_latest_release_for_tests(None).map(|(version, _)| (version, String::new()));

    // The preflight fetched an even newer release than the running one:
    // its notes belong to the update notice, never to the current-version
    // banner (mirrors the cache-side leak guard).
    let stale = Version::parse("0.1.0").expect("stale version");
    cache::record_successful_check(Some(&stale), true).expect("write stale cache");
    preflight::set_preflight_latest_release_for_tests(Some((
        Version::parse("99.0.0").expect("newer version"),
        "### Highlights\n- Later feature (def5678)\n".to_string(),
    )));

    assert!(
        cached_current_release_highlights().is_none(),
        "newer preflight notes must not leak into the current-version banner"
    );

    preflight::set_preflight_latest_release_for_tests(previous_preflight);
    cache::set_cache_dir_override_for_tests(previous);
}

#[test]
fn backfilled_current_notes_survive_a_newer_check_slot_release() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("temp dir");
    let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

    // The desktop backfill writes the running version's notes to the install
    // slot while `update/check` keeps naming a newer release in the check
    // slot. The current banner must read only its own version's notes.
    let current = Version::parse(env!("CARGO_PKG_VERSION")).expect("current version");
    let newer = Version::parse("99.0.0").expect("newer version");
    cache::record_successful_check_with_notes(Some(&newer), true, Some("### Highlights\n- Later feature (def5678)\n"))
        .expect("write newer release to check slot");
    cache::record_installed_release(&current, "### Highlights\n- Current feature (abc1234)\n")
        .expect("write current release to install slot");

    let highlights = cached_current_release_highlights().expect("install slot serves the current banner");
    assert_eq!(highlights.0, current);
    assert_eq!(highlights.1, vec!["Current feature".to_string()]);

    cache::set_cache_dir_override_for_tests(previous);
}

#[test]
fn seeded_release_notes_show_on_first_launch_after_install() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("temp dir");
    let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

    // Simulate `vtcode update` installing the running version: seed the
    // cache with highlights, then the first startup must find them without
    // waiting for a background refresh (the N vs N+1 regression).
    let current = Version::parse(env!("CARGO_PKG_VERSION")).expect("current version");
    let body = "### Highlights\n- Fresh feature (abc1234)\n";
    seed_release_notes_for_installed_version(&current, body);

    assert!(should_show_release_notes_for_current_version(), "freshly installed version must not be marked seen");
    let highlights = cached_current_release_highlights().expect("seeded highlights must be readable");
    assert_eq!(highlights.0, current);
    assert_eq!(highlights.1, vec!["Fresh feature".to_string()]);

    record_current_version_seen();
    assert!(!should_show_release_notes_for_current_version(), "notes must show once, then be marked seen");

    cache::set_cache_dir_override_for_tests(previous);
}

#[test]
fn display_prefers_the_hydrated_snapshot() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("temp dir");
    let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

    // Hydration already decided: the snapshot wins without consulting the
    // live cache, even on a fresh cache that knows nothing.
    let current = Version::parse(env!("CARGO_PKG_VERSION")).expect("current version");
    let bootstrap = Some((current.clone(), vec!["Hydrated note".to_string()]));
    assert_eq!(resolve_release_notes_for_display(bootstrap), Some((current, vec!["Hydrated note".to_string()])));

    cache::set_cache_dir_override_for_tests(previous);
}

#[test]
fn display_backfills_when_hydration_found_no_notes() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("temp dir");
    let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

    // Hydration ran before the preflight fetch landed (empty snapshot);
    // the display-time re-read must serve the notes that arrived since.
    let current = Version::parse(env!("CARGO_PKG_VERSION")).expect("current version");
    assert!(resolve_release_notes_for_display(None).is_none(), "nothing cached yet");
    seed_release_notes_for_installed_version(&current, "### Highlights\n- Late feature (abc1234)\n");

    let served = resolve_release_notes_for_display(None).expect("late notes must be served");
    assert_eq!(served.0, current);
    assert_eq!(served.1, vec!["Late feature".to_string()]);

    cache::set_cache_dir_override_for_tests(previous);
}

#[test]
fn display_stays_quiet_without_notes_or_once_seen() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("temp dir");
    let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

    assert!(resolve_release_notes_for_display(None).is_none(), "a fresh cache announces nothing");

    let current = Version::parse(env!("CARGO_PKG_VERSION")).expect("current version");
    seed_release_notes_for_installed_version(&current, "### Highlights\n- Fresh feature (abc1234)\n");
    record_current_version_seen();
    assert!(resolve_release_notes_for_display(None).is_none(), "a seen release never re-announces");

    cache::set_cache_dir_override_for_tests(previous);
}

#[test]
fn seeded_release_notes_survive_a_later_newer_check() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("temp dir");
    let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

    // The updater installs the running version and seeds its notes, then a
    // preflight check reports an even newer release before first launch
    // (the common N+1 race). The installed version's notes must still be
    // readable instead of being handed the newer release's changelog.
    let current = Version::parse(env!("CARGO_PKG_VERSION")).expect("current version");
    seed_release_notes_for_installed_version(&current, "### Highlights\n- Fresh feature (abc1234)\n");

    let newer = Version::parse("99.0.0").expect("newer version");
    cache::record_successful_check_with_notes(Some(&newer), true, Some("### Highlights\n- Later feature (def5678)\n"))
        .expect("write newer check");

    let served = cached_current_release_highlights().expect("install notes survive the newer check");
    assert_eq!(served.0, current);
    assert_eq!(served.1, vec!["Fresh feature".to_string()]);
    assert_eq!(cached_latest_release().map(|(version, _)| version), Some(newer));

    cache::set_cache_dir_override_for_tests(previous);
}

#[test]
fn seen_marker_is_per_version_so_update_can_acknowledge_the_installed_release() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("temp dir");
    let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

    // `vtcode update` prints the new release's notes before the replaced
    // binary exits, so it acknowledges that version explicitly while
    // `CARGO_PKG_VERSION` is still the old one.
    let installed = Version::parse("99.1.0").expect("installed version");
    record_version_notes_seen(&installed);

    assert!(cache::current_release_info(&Version::parse(env!("CARGO_PKG_VERSION")).expect("current")).is_none());
    assert!(!current_version_notes_seen(), "acknowledging the new release does not mark the running one");

    cache::set_cache_dir_override_for_tests(previous);
}

#[test]
fn current_version_notes_seen_tracks_the_shared_marker() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("temp dir");
    let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

    assert!(!current_version_notes_seen(), "a fresh cache reports unseen notes");
    record_current_version_seen();
    assert!(current_version_notes_seen(), "the shared marker records the running version");

    cache::set_cache_dir_override_for_tests(previous);
}

#[test]
fn seeing_an_installed_release_drops_its_cached_body() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("temp dir");
    let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

    // The install record bridges install time to the first announcement;
    // once seen it must not linger as a second copy of the changelog.
    let current = Version::parse(env!("CARGO_PKG_VERSION")).expect("current version");
    seed_release_notes_for_installed_version(&current, "### Highlights\n- Fresh feature (abc1234)\n");
    let snapshot = cache::read_snapshot().expect("snapshot");
    assert!(snapshot.installed_release.is_some(), "install record exists before it is seen");

    record_current_version_seen();
    let snapshot = cache::read_snapshot().expect("snapshot");
    assert!(snapshot.installed_release.is_none(), "the seen install record is dropped");

    cache::set_cache_dir_override_for_tests(previous);
}

#[test]
fn an_unreadable_cache_stays_quiet_about_release_notes() {
    // The gate and its "seen" mirror must agree on a broken cache: an
    // announcement is a courtesy, so neither may surface one.
    let previous = cache::set_cache_dir_override_for_tests(Some(std::path::PathBuf::from("/nonexistent/vtcode-cache")));
    assert!(current_version_notes_seen(), "an unreadable cache reports seen");
    assert!(!should_show_release_notes_for_current_version(), "and never announces");
    cache::set_cache_dir_override_for_tests(previous);
}

#[test]
fn latest_release_serves_a_newer_cached_release() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("temp dir");
    let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

    // A background check found a newer release: its notes belong to the
    // desktop status snapshot, not to the current-version startup banner.
    let latest = Version::parse("99.0.0").expect("latest version");
    assert_ne!(latest.to_string(), env!("CARGO_PKG_VERSION"));
    let body = "### Highlights\n- Shiny feature (abc1234)\n";
    cache::record_successful_check_with_notes(Some(&latest), true, Some(body)).expect("write cache");

    let served = cached_latest_release().expect("latest release must be readable");
    assert_eq!(served.0, latest);
    assert_eq!(served.1, vec!["Shiny feature".to_string()]);
    assert!(
        cached_current_release_highlights().is_none(),
        "newer notes must not leak into the current-version banner"
    );

    cache::set_cache_dir_override_for_tests(previous);
}

#[test]
fn latest_release_without_a_highlights_section_serves_no_items() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("temp dir");
    let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

    let latest = Version::parse("99.0.0").expect("latest version");
    cache::record_successful_check_with_notes(Some(&latest), false, None).expect("write cache");
    assert!(cached_latest_release().is_none(), "no cached notes means no release to read");

    // Notes without a Highlights section still describe a readable release:
    // the viewer renders its link-only state instead of hiding the entry.
    cache::record_successful_check_with_notes(Some(&latest), false, Some("## 99.0.0\nNo highlights here\n"))
        .expect("write cache");
    let served = cached_latest_release().expect("notes describe a readable release");
    assert_eq!(served.0, latest);
    assert!(served.1.is_empty(), "no Highlights section serves no items");

    cache::set_cache_dir_override_for_tests(previous);
}
