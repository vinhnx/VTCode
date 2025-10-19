use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use crossterm::cursor::Show;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use std::time::{SystemTime, UNIX_EPOCH};
use vtcode_core::cli::args::{Cli, Commands};
use vtcode_core::config::constants::{model_helpers, models};
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::models::{ModelId, Provider};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use vtcode_core::utils::dot_config::{WorkspaceTrustLevel, WorkspaceTrustRecord, get_dot_manager};
use vtcode_core::{initialize_dot_folder, update_model_preference};

/// Drive the first-run interactive setup wizard when a workspace lacks VT Code artifacts.
pub fn maybe_run_first_run_setup(
    args: &Cli,
    workspace: &Path,
    config: &mut VTCodeConfig,
) -> Result<bool> {
    if !is_fresh_workspace(workspace) {
        return Ok(false);
    }

    if args.provider.is_some() || args.model.is_some() {
        return Ok(false);
    }

    if let Some(command) = &args.command {
        match command {
            Commands::Chat | Commands::ChatVerbose => {}
            _ => return Ok(false),
        }
    }

    let full_auto_requested = args.full_auto.is_some();
    let non_interactive = args.skip_confirmations || full_auto_requested;
    let mode = if non_interactive {
        SetupMode::NonInteractive {
            full_auto: full_auto_requested,
        }
    } else {
        SetupMode::Interactive
    };

    run_first_run_setup(workspace, config, mode)?;
    Ok(true)
}

enum SetupMode {
    Interactive,
    NonInteractive { full_auto: bool },
}

fn is_fresh_workspace(workspace: &Path) -> bool {
    let config_path = workspace.join("vtcode.toml");
    let dot_dir = workspace.join(".vtcode");
    !config_path.exists() && !dot_dir.exists()
}

fn run_first_run_setup(workspace: &Path, config: &mut VTCodeConfig, mode: SetupMode) -> Result<()> {
    initialize_dot_folder().ok();

    if !workspace.exists() {
        return Err(anyhow!(
            "Workspace '{}' does not exist for setup",
            workspace.display()
        ));
    }

    let workspace_dot_dir = workspace.join(".vtcode");
    if !workspace_dot_dir.exists() {
        fs::create_dir_all(&workspace_dot_dir).with_context(|| {
            format!(
                "Failed to create workspace .vtcode directory at {}",
                workspace_dot_dir.display()
            )
        })?;
    }

    let mut renderer = AnsiRenderer::stdout();
    renderer.line(
        MessageStyle::Info,
        "┌────────────────────────────────────────────┐",
    )?;
    renderer.line(
        MessageStyle::Info,
        "│        VT Code first-time setup wizard      │",
    )?;
    renderer.line(
        MessageStyle::Info,
        "└────────────────────────────────────────────┘",
    )?;
    let (provider, model, trust) = match mode {
        SetupMode::Interactive => {
            renderer.line(
                MessageStyle::Status,
                "Let's configure your default provider, model, and workspace trust.",
            )?;
            renderer.line(
                MessageStyle::Status,
                "Press Enter to accept the suggested value in brackets.",
            )?;
            renderer.line(MessageStyle::Info, "")?;

            let provider = resolve_initial_provider(config);
            let provider = prompt_provider(&mut renderer, provider)?;
            renderer.line(MessageStyle::Info, "")?;

            let default_model = default_model_for_provider(provider);
            let model = prompt_model(&mut renderer, provider, default_model)?;
            renderer.line(MessageStyle::Info, "")?;

            let trust = prompt_trust(&mut renderer, WorkspaceTrustLevel::ToolsPolicy)?;
            renderer.line(MessageStyle::Info, "")?;

            (provider, model, trust)
        }
        SetupMode::NonInteractive { full_auto } => {
            renderer.line(
                MessageStyle::Status,
                "Non-interactive setup flags detected. Applying defaults without prompts.",
            )?;
            renderer.line(MessageStyle::Info, "")?;

            let provider = resolve_initial_provider(config);
            let default_model = default_model_for_provider(provider);
            let model = default_model.to_string();
            let trust = if full_auto {
                WorkspaceTrustLevel::FullAuto
            } else {
                WorkspaceTrustLevel::ToolsPolicy
            };

            renderer.line(
                MessageStyle::Info,
                &format!("Provider: {}", provider.label()),
            )?;
            renderer.line(MessageStyle::Info, &format!("Model: {}", model))?;
            renderer.line(
                MessageStyle::Info,
                &format!("Workspace trust: {}", trust_label(trust)),
            )?;
            renderer.line(MessageStyle::Info, "")?;

            (provider, model, trust)
        }
    };

    renderer.line(
        MessageStyle::Status,
        "Saving your configuration to vtcode.toml ...",
    )?;

    apply_selection(config, provider, &model);

    let config_path = workspace.join("vtcode.toml");
    ConfigManager::save_config_to_path(&config_path, config).with_context(|| {
        format!(
            "Failed to write initial configuration to {}",
            config_path.display()
        )
    })?;

    update_model_preference(&provider.to_string(), &model).ok();

    persist_workspace_trust(workspace, trust).with_context(|| {
        format!(
            "Failed to persist workspace trust level for {}",
            workspace.display()
        )
    })?;

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(
        MessageStyle::Status,
        &format!(
            "Setup complete. Provider: {} • Model: {} • Trust: {}",
            provider.label(),
            model,
            trust_label(trust)
        ),
    )?;
    renderer.line(MessageStyle::Info, "")?;
    renderer.line(
        MessageStyle::Status,
        "Tip: run `/init` anytime to rerun this setup and refresh other workspace defaults.",
    )?;
    renderer.line(MessageStyle::Info, "")?;

    Ok(())
}

fn resolve_initial_provider(config: &VTCodeConfig) -> Provider {
    let configured = config.agent.provider.trim();
    if configured.is_empty() {
        Provider::OpenAI
    } else {
        Provider::from_str(configured).unwrap_or(Provider::OpenAI)
    }
}

fn prompt_provider(renderer: &mut AnsiRenderer, default: Provider) -> Result<Provider> {
    renderer.line(MessageStyle::Status, "Choose your default provider:")?;
    let providers = Provider::all_providers();

    match select_provider_with_ratatui(&providers, default) {
        Ok(provider) => Ok(provider),
        Err(error) => {
            if error.is::<SetupInterrupted>() {
                return Err(error);
            }

            renderer.line(
                MessageStyle::Info,
                &format!("Falling back to manual input ({error})."),
            )?;
            prompt_provider_text(renderer, &providers, default)
        }
    }
}

fn prompt_provider_text(
    renderer: &mut AnsiRenderer,
    providers: &[Provider],
    default: Provider,
) -> Result<Provider> {
    for (index, provider) in providers.iter().enumerate() {
        renderer.line(
            MessageStyle::Info,
            &format!("  {}) {}", index + 1, provider.label()),
        )?;
    }

    let default_label = default.to_string();

    loop {
        let input = prompt_with_placeholder(&format!("Provider [{}]", default_label))?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(default);
        }

        if let Ok(index) = trimmed.parse::<usize>()
            && let Some(provider) = providers.get(index - 1)
        {
            return Ok(*provider);
        }

        match Provider::from_str(trimmed) {
            Ok(provider) => return Ok(provider),
            Err(err) => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("{err}. Please choose a valid provider."),
                )?;
            }
        }
    }
}

fn select_provider_with_ratatui(providers: &[Provider], default: Provider) -> Result<Provider> {
    if providers.is_empty() {
        return Err(anyhow!("No providers available for selection"));
    }

    let entries: Vec<SelectionEntry> = providers
        .iter()
        .enumerate()
        .map(|(index, provider)| {
            SelectionEntry::new(
                format!("{:>2}. {}", index + 1, provider.label()),
                provider.label().to_string(),
            )
        })
        .collect();

    let default_index = providers
        .iter()
        .position(|provider| *provider == default)
        .unwrap_or(0);

    let instructions = format!(
        "Default: {}. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default.",
        default.label()
    );

    let selected_index =
        run_ratatui_selection("Providers", &instructions, &entries, default_index)?;
    Ok(providers[selected_index])
}

#[derive(Debug)]
struct SetupInterrupted;

impl fmt::Display for SetupInterrupted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("setup interrupted by Ctrl+C")
    }
}

impl std::error::Error for SetupInterrupted {}

#[derive(Debug, Clone)]
struct SelectionEntry {
    display: String,
    summary: String,
}

impl SelectionEntry {
    fn new(display: String, summary: String) -> Self {
        Self { display, summary }
    }
}

fn run_ratatui_selection(
    title: &str,
    instructions: &str,
    entries: &[SelectionEntry],
    default_index: usize,
) -> Result<usize> {
    if entries.is_empty() {
        return Err(anyhow!("No options available for selection"));
    }

    if !io::stdout().is_terminal() {
        return Err(anyhow!("Terminal UI is unavailable"));
    }

    let mut stdout = io::stdout();
    let mut terminal_guard = TerminalModeGuard::new(title);
    terminal_guard.enable_raw_mode()?;
    terminal_guard.enter_alternate_screen(&mut stdout)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)
        .with_context(|| format!("Failed to initialize Ratatui terminal for {title} selector"))?;
    terminal_guard.hide_cursor(&mut terminal)?;

    let selection_result = (|| -> Result<usize> {
        let total = entries.len();
        let mut selected_index = default_index.min(total.saturating_sub(1));

        loop {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    let layout = Layout::default()
                        .direction(Direction::Vertical)
                        .margin(1)
                        .constraints([
                            Constraint::Length(3),
                            Constraint::Min(3),
                            Constraint::Length(1),
                        ])
                        .split(area);

                    let instructions = Paragraph::new(instructions).wrap(Wrap { trim: true });
                    frame.render_widget(instructions, layout[0]);

                    let items: Vec<ListItem> = entries
                        .iter()
                        .map(|entry| ListItem::new(entry.display.clone()))
                        .collect();

                    let list = List::new(items)
                        .block(Block::default().title(title).borders(Borders::ALL))
                        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                        .highlight_symbol("▶ ");

                    let mut state = ListState::default();
                    state.select(Some(selected_index));
                    frame.render_stateful_widget(list, layout[1], &mut state);

                    let current_label = &entries[selected_index].summary;
                    frame.render_widget(
                        Paragraph::new(format!("Selected: {current_label}")),
                        layout[2],
                    );
                })
                .with_context(|| format!("Failed to draw {title} selector UI"))?;

            match event::read()
                .with_context(|| format!("Failed to read terminal input for {title} selector"))?
            {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if selected_index == 0 {
                            selected_index = total - 1;
                        } else {
                            selected_index -= 1;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        selected_index = (selected_index + 1) % total;
                    }
                    KeyCode::Home => selected_index = 0,
                    KeyCode::End => selected_index = total - 1,
                    KeyCode::Enter => return Ok(selected_index),
                    KeyCode::Esc => return Ok(default_index.min(total - 1)),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Err(SetupInterrupted.into());
                    }
                    KeyCode::Char(c) => {
                        if let Some(index) = c
                            .to_digit(10)
                            .map(|digit| digit as usize)
                            .filter(|index| (1..=total).contains(index))
                        {
                            selected_index = index - 1;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    })();

    let cleanup_result = terminal_guard.restore_with_terminal(&mut terminal);
    cleanup_result?;
    selection_result
}

struct TerminalModeGuard {
    label: String,
    raw_mode_enabled: bool,
    alternate_screen: bool,
    cursor_hidden: bool,
}

impl TerminalModeGuard {
    fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            raw_mode_enabled: false,
            alternate_screen: false,
            cursor_hidden: false,
        }
    }

    fn enable_raw_mode(&mut self) -> Result<()> {
        enable_raw_mode()
            .with_context(|| format!("Failed to enable raw mode for {} selector", self.label))?;
        self.raw_mode_enabled = true;
        Ok(())
    }

    fn enter_alternate_screen(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        execute!(stdout, EnterAlternateScreen).with_context(|| {
            format!(
                "Failed to enter alternate screen for {} selector",
                self.label
            )
        })?;
        self.alternate_screen = true;
        Ok(())
    }

    fn hide_cursor(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        terminal
            .hide_cursor()
            .with_context(|| format!("Failed to hide cursor for {} selector", self.label))?;
        self.cursor_hidden = true;
        Ok(())
    }

    fn restore_with_terminal(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        if self.raw_mode_enabled {
            disable_raw_mode().with_context(|| {
                format!("Failed to disable raw mode after {} selector", self.label)
            })?;
            self.raw_mode_enabled = false;
        }

        if self.alternate_screen {
            execute!(terminal.backend_mut(), LeaveAlternateScreen).with_context(|| {
                format!(
                    "Failed to leave alternate screen after {} selector",
                    self.label
                )
            })?;
            self.alternate_screen = false;
        }

        if self.cursor_hidden {
            terminal
                .show_cursor()
                .with_context(|| format!("Failed to show cursor after {} selector", self.label))?;
            self.cursor_hidden = false;
        }

        Ok(())
    }

    fn restore_without_terminal(&mut self) {
        if self.raw_mode_enabled {
            let _ = disable_raw_mode();
            self.raw_mode_enabled = false;
        }

        if self.alternate_screen {
            let mut stdout = io::stdout();
            let _ = execute!(stdout, LeaveAlternateScreen);
            self.alternate_screen = false;
        }

        if self.cursor_hidden {
            let mut stdout = io::stdout();
            let _ = execute!(stdout, Show);
            self.cursor_hidden = false;
        }
    }
}

impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        self.restore_without_terminal();
    }
}

fn prompt_model(
    renderer: &mut AnsiRenderer,
    provider: Provider,
    default_model: &'static str,
) -> Result<String> {
    renderer.line(
        MessageStyle::Status,
        &format!(
            "Enter the default model for {} (Enter to accept {}).",
            provider.label(),
            default_model
        ),
    )?;

    let options = model_options(provider, default_model);

    match select_model_with_ratatui(&options, default_model) {
        Ok(model) => Ok(model),
        Err(error) => {
            if error.is::<SetupInterrupted>() {
                return Err(error);
            }

            renderer.line(
                MessageStyle::Info,
                &format!("Falling back to manual input ({error})."),
            )?;
            prompt_model_text(renderer, provider, default_model, &options)
        }
    }
}

fn prompt_trust(
    renderer: &mut AnsiRenderer,
    default: WorkspaceTrustLevel,
) -> Result<WorkspaceTrustLevel> {
    renderer.line(
        MessageStyle::Status,
        "Workspace trust determines which actions are allowed.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  [1] Tools policy – prompts before running elevated actions (recommended)",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  [2] Full auto – allow unattended execution without prompts",
    )?;

    match select_trust_with_ratatui(default) {
        Ok(level) => Ok(level),
        Err(error) => {
            if error.is::<SetupInterrupted>() {
                return Err(error);
            }

            renderer.line(
                MessageStyle::Info,
                &format!("Falling back to manual input ({error})."),
            )?;
            prompt_trust_text(renderer, default)
        }
    }
}

fn prompt_with_placeholder(prompt: &str) -> Result<String> {
    print!("{}: ", prompt);
    io::stdout()
        .flush()
        .context("Failed to flush prompt to stdout")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("Failed to read setup input")?;
    Ok(input)
}

fn model_options(provider: Provider, default_model: &'static str) -> Vec<String> {
    let mut options: Vec<String> = model_helpers::supported_for(&provider.to_string())
        .map(|list| list.iter().map(|model| (*model).to_string()).collect())
        .unwrap_or_default();

    if options.is_empty() {
        options.push(default_model.to_string());
    }

    if !options.iter().any(|model| model == default_model) {
        options.insert(0, default_model.to_string());
    }

    let mut seen = HashSet::new();
    options.retain(|model| seen.insert(model.clone()));
    options
}

fn select_model_with_ratatui(options: &[String], default_model: &'static str) -> Result<String> {
    if options.is_empty() {
        return Err(anyhow!("No models available for selection"));
    }

    let entries: Vec<SelectionEntry> = options
        .iter()
        .enumerate()
        .map(|(index, model)| {
            SelectionEntry::new(format!("{:>2}. {}", index + 1, model), model.clone())
        })
        .collect();

    let default_index = options
        .iter()
        .position(|model| model == default_model)
        .unwrap_or(0);

    let instructions = format!(
        "Default: {}. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default.",
        default_model
    );

    let selected_index = run_ratatui_selection("Models", &instructions, &entries, default_index)?;
    Ok(entries[selected_index].summary.clone())
}

fn prompt_model_text(
    renderer: &mut AnsiRenderer,
    provider: Provider,
    default_model: &'static str,
    options: &[String],
) -> Result<String> {
    if !options.is_empty() {
        renderer.line(
            MessageStyle::Info,
            &format!("Suggested {} models:", provider.label()),
        )?;
        for (index, model) in options.iter().enumerate() {
            renderer.line(
                MessageStyle::Info,
                &format!("  {:>2}. {}", index + 1, model),
            )?;
        }
    }

    let input = prompt_with_placeholder(&format!("Model [{}]", default_model))?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(default_model.to_string());
    }

    match trimmed.parse::<ModelId>() {
        Ok(id) => Ok(id.as_str().to_string()),
        Err(_) => {
            renderer.line(
                MessageStyle::Info,
                "Unrecognized model identifier. It will be saved as entered.",
            )?;
            Ok(trimmed.to_string())
        }
    }
}

fn prompt_trust_text(
    renderer: &mut AnsiRenderer,
    default: WorkspaceTrustLevel,
) -> Result<WorkspaceTrustLevel> {
    let default_choice = match default {
        WorkspaceTrustLevel::ToolsPolicy => "1",
        WorkspaceTrustLevel::FullAuto => "2",
    };

    loop {
        let input = prompt_with_placeholder(&format!("Trust level [{}]", default_choice))?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(default);
        }

        match trimmed {
            "1" | "tools" | "tool" => return Ok(WorkspaceTrustLevel::ToolsPolicy),
            "2" | "full" | "auto" | "full-auto" => return Ok(WorkspaceTrustLevel::FullAuto),
            _ => {
                renderer.line(
                    MessageStyle::Error,
                    "Please choose 1 for Tools policy or 2 for Full auto.",
                )?;
            }
        }
    }
}

fn select_trust_with_ratatui(default: WorkspaceTrustLevel) -> Result<WorkspaceTrustLevel> {
    let entries = Vec::from([
        (
            WorkspaceTrustLevel::ToolsPolicy,
            SelectionEntry::new(
                " 1. Tools policy – prompts before running elevated actions (recommended)"
                    .to_string(),
                "Tools policy – prompts before running elevated actions (recommended)".to_string(),
            ),
        ),
        (
            WorkspaceTrustLevel::FullAuto,
            SelectionEntry::new(
                " 2. Full auto – allow unattended execution without prompts".to_string(),
                "Full auto – allow unattended execution without prompts".to_string(),
            ),
        ),
    ]);

    let default_index = match default {
        WorkspaceTrustLevel::ToolsPolicy => 0,
        WorkspaceTrustLevel::FullAuto => 1,
    };

    let selection_entries: Vec<SelectionEntry> =
        entries.iter().map(|(_, entry)| entry.clone()).collect();
    let default_summary = &selection_entries[default_index].summary;
    let instructions = format!(
        "Default: {}. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default.",
        default_summary
    );

    let selected_index = run_ratatui_selection(
        "Workspace trust",
        &instructions,
        &selection_entries,
        default_index,
    )?;
    Ok(entries[selected_index].0)
}

fn default_model_for_provider(provider: Provider) -> &'static str {
    match provider {
        Provider::Gemini => models::google::DEFAULT_MODEL,
        Provider::OpenAI => models::openai::DEFAULT_MODEL,
        Provider::Anthropic => models::anthropic::DEFAULT_MODEL,
        Provider::DeepSeek => models::deepseek::DEFAULT_MODEL,
        Provider::OpenRouter => models::openrouter::DEFAULT_MODEL,
        Provider::Ollama => models::ollama::DEFAULT_MODEL,
        Provider::Moonshot => models::moonshot::DEFAULT_MODEL,
        Provider::XAI => models::xai::DEFAULT_MODEL,
        Provider::ZAI => models::zai::DEFAULT_MODEL,
    }
}

fn apply_selection(config: &mut VTCodeConfig, provider: Provider, model: &str) {
    let provider_key = provider.to_string();
    config.agent.provider = provider_key.clone();
    config.agent.api_key_env = provider.default_api_key_env().to_string();
    config.agent.default_model = model.to_string();
    config.router.models.simple = model.to_string();
    config.router.models.standard = model.to_string();
    config.router.models.complex = model.to_string();
    config.router.models.codegen_heavy = model.to_string();
    config.router.models.retrieval_heavy = model.to_string();
}

fn trust_label(level: WorkspaceTrustLevel) -> &'static str {
    match level {
        WorkspaceTrustLevel::ToolsPolicy => "Tools policy",
        WorkspaceTrustLevel::FullAuto => "Full auto",
    }
}

fn persist_workspace_trust(workspace: &Path, level: WorkspaceTrustLevel) -> Result<()> {
    let canonical = workspace
        .canonicalize()
        .with_context(|| {
            format!(
                "Failed to canonicalize workspace path {} for trust setup",
                workspace.display()
            )
        })?
        .to_string_lossy()
        .into_owned();

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let manager = get_dot_manager();
    let guard = manager
        .lock()
        .expect("workspace trust dot manager mutex poisoned");
    guard
        .update_config(|cfg| {
            cfg.workspace_trust.entries.insert(
                canonical.clone(),
                WorkspaceTrustRecord {
                    level,
                    trusted_at: timestamp,
                },
            );
        })
        .context("Failed to update workspace trust in dot config")
}
