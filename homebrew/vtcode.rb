class Vtcode < Formula
  desc "A Rust-based terminal coding agent with modular architecture"
  homepage "https://github.com/vinhnx/vtcode"
  version "0.42.11"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "dc835a1fb47f8a5acdf17ccde422ffb020beb2e02409cc28251b71eb75c0dcc2"
    else
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "5cf60ea93855279e29c130e3912d67ec7bf507f9c46642c8c0736b96062c2f84"
    end
  end

  def install
    bin.install "vtcode"
  end

  test do
    system "#{bin}/vtcode", "--version"
  end
end