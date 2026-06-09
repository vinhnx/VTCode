class Vtcode < Formula
  desc "Rust-based terminal coding agent with semantic code intelligence"
  homepage "https://github.com/vinhnx/vtcode"
  license "MIT"
  version "0.123.10"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/vinhnx/vtcode/releases/download/#{version}/vtcode-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "8e97053f595d73b325b65c87e8c462509b5800d2f4f537f7de38e8126303dbae"
    else
      url "https://github.com/vinhnx/vtcode/releases/download/#{version}/vtcode-#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "5768221eb3bb52d0d4311cd55543aa6872dc5e8d8f478c5e2f6aa7e346142e59"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/vinhnx/vtcode/releases/download/#{version}/vtcode-#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "ab77663ff1efd78bad21ac62a08e8ef885505ea8946895525e958d6e737207b2"
    elsif Hardware::CPU.arm?
      url "https://github.com/vinhnx/vtcode/releases/download/#{version}/vtcode-#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "5ab3fb82bea7bea1ce0bb6f0167c4f6ee177b00159b1ea1ff460e6a625e899f8"
    else
      odie "VT Code #{version} does not support this Linux architecture"
    end
  end

  def install
    bin.install "vtcode"
  end

  def caveats
    <<~EOS
      VT Code is now installed! To get started:

      1. Set your API key environment variable:
         export OPENAI_API_KEY="sk-..."
         (or use ANTHROPIC_API_KEY, GEMINI_API_KEY, etc.)

      2. Launch VT Code:
         vtcode

      Supported providers:
        • OpenAI (OPENAI_API_KEY)
        • Anthropic (ANTHROPIC_API_KEY)
        • Google Gemini (GEMINI_API_KEY)
        • xAI (XAI_API_KEY)
        • DeepSeek (DEEPSEEK_API_KEY)
        • OpenRouter (OPENROUTER_API_KEY)
        • Ollama (local)

      For more information, visit:
        https://github.com/vinhnx/vtcode
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/vtcode --version")
  end
end
