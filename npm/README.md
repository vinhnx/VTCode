# VT Code

A Rust-based terminal coding agent with semantic code understanding and an enhanced TUI experience.

## Installation

Install globally with npm:

```shell
npm install -g vtcode-ai
```

Alternatively, you can install with Cargo:

```shell
cargo install vtcode
```

Or with Homebrew (macOS):

```shell
brew install vinhnx/tap/vtcode
```

## Quickstart

After installation, simply run `vtcode` to get started:

```shell
vtcode
```

## Configuration

Set your API key for your preferred provider:

```shell
export OPENAI_API_KEY="your_key_here"
# or
export ANTHROPIC_API_KEY="your_key_here"
# or
export XAI_API_KEY="your_key_here"
# or
export GEMINI_API_KEY="your_key_here"
# or
export OPENROUTER_API_KEY="your_key_here"
```

Alternatively, create a `.env` file in your project directory:

```shell
# .env file
OPENAI_API_KEY=your_openai_key_here
ANTHROPIC_API_KEY=your_anthropic_key_here
XAI_API_KEY=your_anthropic_key_here
GEMINI_API_KEY=your_gemini_key_here
OPENROUTER_API_KEY=your_openrouter_key_here
```

## Features

- **Multi-Provider AI Support**: Gemini, OpenAI, Anthropic, xAI, OpenRouter, and DeepSeek integration
- **Enhanced Terminal User Interface**: Modern TUI with mouse support and text selection
- **Advanced Code Intelligence**: Tree-sitter parsing for multiple languages
- **Enterprise Security**: Workspace boundary enforcement and configurable command allowlists
- **Cross-Platform**: Works on macOS, Linux, and Windows

## Documentation

- [**Getting Started**](https://github.com/vinhnx/vtcode/blob/main/docs/user-guide/getting-started.md) - Installation and basic usage
- [**Configuration**](https://github.com/vinhnx/vtcode/blob/main/docs/project/) - Advanced configuration options
- [**API Reference**](https://docs.rs/vtcode) - Complete API documentation

## Publishing to GitHub Packages

To publish this package to GitHub Packages:

1. Configure your `.npmrc` file with GitHub registry settings:
   ```bash
   # Copy the example configuration
   cp .npmrc.example .npmrc
   ```

2. Create a GitHub personal access token with the required scopes:
   - Go to GitHub Settings → Developer settings → Personal access tokens → Tokens (classic)
   - Create a new token with at least these scopes:
     - `write:packages` (to publish packages)
     - `read:packages` (to download packages)
     - `delete:packages` (optional, to delete packages)
     - `repo` (to link packages to your repositories)

3. Set your GitHub personal access token as an environment variable:
   ```bash
   export GITHUB_TOKEN=your_github_personal_access_token_here
   ```

4. Publish the package:
   ```bash
   npm publish
   ```

See the [GitHub Packages documentation](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-npm-registry) for more details.

## License

This project is licensed under the MIT License - see [LICENSE](https://github.com/vinhnx/vtcode/blob/main/LICENSE) for details.