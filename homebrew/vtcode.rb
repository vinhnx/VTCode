class Vtcode < Formula
  desc "A Rust-based terminal coding agent with modular architecture"
  homepage "https://github.com/vinhnx/vtcode"
  version "0.42.16"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "62ca4c630581cae8806dfa6fc50af02857f8392dbea88fe86e09b82277855795"
    else
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "7634cf9a11a7f4ddf55455041fbd97f9c0126334d2443798a003feb872981986"
    end
  end

  def install
    bin.install "vtcode"
  end

  test do
    system "#{bin}/vtcode", "--version"
  end
end