class Vtcode < Formula
  desc "A Rust-based terminal coding agent with modular architecture"
  homepage "https://github.com/vinhnx/vtcode"
  version "0.42.15"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "383d0935cf96854e62ffc5f1120cf4773b6c1daf2d91a85b1494bc4ae43a5105"
    else
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "2dbbf6f833a6ba435ce063f7384c06a6f3decae51a2e85e6e4d2a360e744ff1a"
    end
  end

  def install
    bin.install "vtcode"
  end

  test do
    system "#{bin}/vtcode", "--version"
  end
end