#!/usr/bin/env bash
# Release build helpers: archive packaging, Linux/Windows CI builds, and the
# docs.rs rebuild trigger.
#
# This file is sourced by `scripts/release.sh`; it intentionally does not set
# shell options so the caller controls `set -euo pipefail`.

package_release_archive() {
	local target=$1
	local binary_name=$2
	local archive_path=$3
	local release_dir="target/$target/release"
	if [[ "$target" == *-apple-darwin ]]; then
		sign_and_notarize_macos_binary "$release_dir/$binary_name"
	fi

	# Stage the binary plus the shared man page set into a temp dir so the
	# archive layout matches what install.sh expects (vtcode at root,
	# man/man1/*.1 alongside).
	local stage_dir
	stage_dir=$(mktemp -d)
	cp "$release_dir/$binary_name" "$stage_dir/"
	# binaries_dir is main's local in release.sh, visible here via dynamic scope.
	# shellcheck disable=SC2154
	if [[ -d "$binaries_dir/man" ]]; then
		cp -R "$binaries_dir/man" "$stage_dir/"
	fi
	tar -C "$stage_dir" -czf "$archive_path" .
	rm -rf "$stage_dir"
	if [[ "$target" == *-apple-darwin ]]; then
		verify_macos_release_archive "$archive_path"
	fi
}

# Build the macOS desktop .app and stage its distribution zip + checksum.
#
#   build_macos_desktop_archive <version> <binaries-dir>
#
# Runs scripts/package-desktop-macos.sh for the host target (reusing the
# just-built arch CLI cache via CARGO_BUILD_TARGET), Developer ID signs +
# notarizes + staples when signing credentials are configured (ad hoc
# otherwise), and writes vtcode-desktop-<v>-<target>.zip + .sha256 into
# <binaries-dir>, where generate_checksums_manifest and the upload queue pick
# them up automatically. No compatibility asset: the legacy updater bridge is
# CLI-only (the zip holds a bundle, not a root vtcode executable).
build_macos_desktop_archive() {
	local version=$1
	local binaries_dir=$2
	if [[ "$(uname -s)" != "Darwin" ]]; then
		print_error "Desktop app release packaging requires macOS"
		return 1
	fi
	local host_target
	host_target=$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')
	if [[ "$host_target" != *-apple-darwin ]]; then
		print_error "Cannot determine the macOS host target (got '${host_target:-unknown}')"
		return 1
	fi
	local stage_dir
	if ! stage_dir=$(mktemp -d "${TMPDIR:-/tmp}/vtcode-desktop-release.XXXXXX"); then
		print_error "Could not create the desktop staging directory"
		return 1
	fi
	local staged_app="$stage_dir/VT Code.app"
	# SCRIPT_DIR is release.sh's local, visible here via dynamic scope.
	# shellcheck disable=SC2154
	if ! env CARGO_BUILD_TARGET="$host_target" "$SCRIPT_DIR/package-desktop-macos.sh" "$staged_app"; then
		print_error "Desktop app packaging failed"
		rm -rf "$stage_dir"
		return 1
	fi
	if ! macos_release_signing_preflight; then
		rm -rf "$stage_dir"
		return 1
	fi
	local archive_name
	archive_name=$(desktop_archive_name "$version" "$host_target")
	local archive_path="$binaries_dir/$archive_name"
	# _vtcode_macos_release_signing_mode is set by the sourced signing lib.
	# shellcheck disable=SC2154
	if [[ "$_vtcode_macos_release_signing_mode" == "notarized" ]]; then
		if ! sign_macos_app_bundle "$staged_app"; then
			rm -rf "$stage_dir"
			return 1
		fi
		local notary_zip="$stage_dir/notarization.zip"
		if ! ditto -c -k --sequesterRsrc --keepParent "$staged_app" "$notary_zip"; then
			print_error "Could not create the desktop notarization archive"
			rm -rf "$stage_dir"
			return 1
		fi
		if ! notarize_macos_archive "$notary_zip"; then
			rm -rf "$stage_dir"
			return 1
		fi
		if ! staple_macos_app_bundle "$staged_app"; then
			rm -rf "$stage_dir"
			return 1
		fi
		if ! verify_macos_app_bundle "$staged_app"; then
			rm -rf "$stage_dir"
			return 1
		fi
		rm -f "$notary_zip"
		# Re-zip after stapling so the ticket ships inside the bundle.
		if ! ditto -c -k --sequesterRsrc --keepParent "$staged_app" "$archive_path"; then
			print_error "Could not create the desktop distribution archive: $archive_path"
			rm -rf "$stage_dir"
			return 1
		fi
	else
		# Ad hoc build: ship the FIRST-LAUNCH.txt Gatekeeper guidance the
		# packaging script wrote beside the bundle.
		if ! ditto -c -k --sequesterRsrc "$stage_dir" "$archive_path"; then
			print_error "Could not create the desktop distribution archive: $archive_path"
			rm -rf "$stage_dir"
			return 1
		fi
	fi
	rm -rf "$stage_dir"
	if ! shasum -a 256 "$archive_path" >"$archive_path.sha256"; then
		print_error "Could not write the desktop checksum sidecar"
		return 1
	fi
	print_success "Staged desktop archive: $archive_name"
}

# Trigger the Linux/Windows CI workflow for a released version and wait for
# its builds to complete. Records the workflow run id in CI_RUN_ID for the
# later artifact download.
#
#   trigger_ci_builds <released-version> <dry-run>
trigger_ci_builds() {
	local released_version=$1
	local dry_run=$2
		print_info "Step 3.5: Triggering CI for Linux and Windows builds..."

		if [[ "$dry_run" == 'true' ]]; then
			print_info "Dry run - would trigger CI workflow for $released_version"
		else
			# Push tags to ensure CI can checkout the correct ref
			print_info "Pushing tags to GitHub..."
			git push origin --tags --no-verify 2>/dev/null || true

			# Trigger the build-linux-windows workflow
			# Timestamp the dispatch (minus a 5-minute skew margin for local
			# clock vs GitHub server drift): run selection below only accepts
			# runs created after this point, so a delayed new run is never
			# confused with the previous release's run. The margin keeps a
			# fast local clock from excluding the real run.
			local trigger_epoch trigger_time
			trigger_epoch=$(($(date +%s) - 300))
			if [[ "$(uname -s)" == "Darwin" ]]; then
				trigger_time=$(date -u -r "$trigger_epoch" +%Y-%m-%dT%H:%M:%SZ)
			else
				trigger_time=$(date -u -d "@$trigger_epoch" +%Y-%m-%dT%H:%M:%SZ)
			fi
			if gh workflow run build-linux-windows.yml --field tag="$released_version"; then
				print_success "CI workflow triggered for $released_version"

				# Wait for CI to complete.
				print_info "Waiting for CI builds to complete..."
				local run_id=""

				# Get the workflow run ID - wait for it to appear. GitHub can
				# take several minutes to register a dispatched run, so poll
				# for up to ~6 minutes before giving up.
				local find_run_attempts=0
				local max_find_attempts=36 # 36 x 10s ~= 6 minutes
				while [[ -z "$run_id" && $find_run_attempts -lt $max_find_attempts ]]; do
					sleep 10
					# Newest run of this workflow created after the dispatch
					# above (ISO-8601 Zulu timestamps sort lexicographically).
					# $since is a jq variable, deliberately shielded from shell expansion.
				# shellcheck disable=SC2016
				run_id=$(gh run list --workflow build-linux-windows.yml --limit 5 --json databaseId,createdAt --jq --arg since "$trigger_time" '[.[] | select(.createdAt > $since)] | .[0].databaseId // empty' 2>/dev/null || true)
					find_run_attempts=$((find_run_attempts + 1))
				done

				if [[ -z "$run_id" ]]; then
					print_warning "Could not find CI workflow run - will use macOS binaries only"
				# `gh run watch` blocks until the run completes and exits
				# nonzero unless the conclusion is success. CI jobs carry
				# 45m timeouts, so a stalled run is bounded server-side.
				elif gh run watch "$run_id" --exit-status >/dev/null 2>&1; then
					print_success "CI builds completed successfully"
					# Store run_id for later download
					# (consumed by the artifact download later in release.sh main).
					# shellcheck disable=SC2034
					CI_RUN_ID="$run_id"
				else
					local conclusion
					conclusion=$(gh run view "$run_id" --json conclusion --jq '.conclusion' 2>/dev/null || echo "unknown")
					print_warning "CI build failed with conclusion: $conclusion - will use macOS binaries only"
					gh run view "$run_id" --log || true
				fi
			else
				print_warning "Failed to trigger CI workflow - will use macOS binaries only"
			fi
		fi
}

trigger_docs_rs_rebuild() {
	local version=$1
	local dry_run_flag=$2

	if [[ "$dry_run_flag" == 'true' ]]; then
		print_info "Dry run - would trigger docs.rs rebuild for version $version"
		return 0
	fi

	print_distribution "Triggering docs.rs rebuild for version $version..."
	local crates=(
		vtcode-diff vtcode-commons vtcode-auth vtcode-exec-events vtcode-desktop-protocol vtcode-webmcp vtcode-memory vtcode-macros
		vtcode-config vtcode-indexer vtcode-bash-runner vtcode-utility-tool-specs vtcode-eval
		vtcode-safety vtcode-a2a vtcode-llm vtcode-skills vtcode-agent-plugins vtcode-ui vtcode-mcp
		vtcode-core vtcode-acp vtcode
	)
	for crate in "${crates[@]}"; do
		# Fire-and-forget rebuild pings; each is an independent HTTPS GET,
		# so all 22 run concurrently instead of sequentially.
		curl -s -o /dev/null "https://docs.rs/${crate}/${version}" || true &
	done
	wait 2>/dev/null || true
}
