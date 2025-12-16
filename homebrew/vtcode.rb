class Vtcode < Formula
  desc "Rust-based terminal coding agent with semantic code intelligence"
  homepage "https://github.com/vinhnx/vtcode"
  license "MIT"
  version "0.50.2"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "882f0c34e2e1f7036122335cd84c7093e454d17e2da4768b4d05fb4d33eca8d5"
    else
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "0e7a1dee98e07246d8a9c0769f03e9c88dab96b34dad0cdc5f031063fb885cd2"
    end
  end

  on_linux do
    if Hardware::CPU.arm? && Hardware::CPU.is_64_bit?
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "placeholder_aarch64_linux"
    else
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "placeholder_x86_64_linux"
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