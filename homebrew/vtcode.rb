class Vtcode < Formula
  desc "Rust-based terminal coding agent with semantic code intelligence"
  homepage "https://github.com/vinhnx/vtcode"
  license "MIT"
  version "0.174.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/vinhnx/vtcode/releases/download/#{version}/vtcode-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "4095852bed1fb90a989397e433471840a8f6d46d77ec03d58cc68720f8b91c0a"
    else
      url "https://github.com/vinhnx/vtcode/releases/download/#{version}/vtcode-#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "8e063bbe6d69f9b50ba14538d2957281f876f2f19f23e8b0cad7fa3f27dfcb31"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/vinhnx/vtcode/releases/download/#{version}/vtcode-#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "ab77663ff1efd78bad21ac62a08e8ef885505ea8946895525e958d6e737207b2"
    elsif Hardware::CPU.arm?
      url "https://github.com/vinhnx/vtcode/releases/download/#{version}/vtcode-#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "9122a1265cab91dcfaf0140b4501c82ce44a0ea7a80573137584dd89c555d352"
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
