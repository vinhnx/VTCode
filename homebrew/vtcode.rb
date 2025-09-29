class Vtcode < Formula
  desc "A Rust-based terminal coding agent with modular architecture"
  homepage "https://github.com/vinhnx/vtcode"
  version "0.15.2"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "4e5abe428ec12cb8079f1c5a3a146b7854f407174ccd832a8f8ebf590bdbc39c"
    else
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "4e5abe428ec12cb8079f1c5a3a146b7854f407174ccd832a8f8ebf590bdbc39c"
    end
  end

  def install
    bin.install "vtcode"
  end

  test do
    system "#{bin}/vtcode", "--version"
  end
end