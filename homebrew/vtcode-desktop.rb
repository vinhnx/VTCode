cask "vtcode-desktop" do
  version "0.181.6"
  sha256 "dd195b8e843ff24bc3c005e845c206a026bf280774d3bc98e7cfd60f50c6e929"

  url "https://github.com/vinhnx/vtcode/releases/download/#{version}/vtcode-desktop-#{version}-aarch64-apple-darwin.zip"
  name "VT Code"
  desc "Desktop app for the VT Code terminal coding agent"
  homepage "https://github.com/vinhnx/vtcode"

  depends_on macos: :sequoia
  depends_on arch: :arm64

  app "VT Code.app"

  # Only the GUI's own state is removed. `~/Library/Application Support/com.vinhnx.vtcode`
  # and `~/Library/Caches/com.vinhnx.vtcode` belong to the shared CLI runtime
  # (sessions, credentials, config), so `--zap` must leave them alone.
  zap trash: [
    "~/.vtcode/desktop",
    "~/Library/Preferences/com.vinhnx.vtcode.desktop.plist",
    "~/Library/Preferences/vtcode-desktop.plist",
    "~/Library/Saved Application State/com.vinhnx.vtcode.desktop.savedState",
  ]

  caveats <<~EOS
    VT Code Desktop reuses the VT Code CLI configuration and provider credentials.
    Install the CLI too if you want terminal access:

      brew install vinhnx/tap/vtcode

    ONE-TIME macOS SECURITY STEP REQUIRED

    Release builds are ad hoc signed unless the release notes say they are notarized,
    and Homebrew quarantines the download, so Gatekeeper blocks the first launch.

    To allow it, once:
      1. Open the app from Applications so macOS records the block
      2. Open System Settings > Privacy & Security
      3. Click "Open Anyway" next to the VT Code message
      4. Open the app again (from Finder, or run: vtcode desktop)

    "Open Anyway" only appears for about an hour after a blocked launch. macOS 15+
    has no right-click > Open shortcut for unnotarized apps. Notarized releases
    open without this step.

    The desktop cask currently ships Apple Silicon builds; Intel Macs can still
    install from source with `vtcode desktop --install`.
  EOS
end
