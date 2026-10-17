//! Native Anthropic speed step shared by inline, ratatui, and typed flows.

use super::*;
use vtcode_config::AnthropicSpeed;
use vtcode_ui::tui::ui::interactive_list::{SelectionEntry, run_interactive_selection};

const ACTION_PREFIX: &str = "anthropic-speed:";
struct SpeedChoice {
    value: &'static str,
    label: &'static str,
    description: &'static str,
}

const SPEED_CHOICES: [SpeedChoice; 3] = [
    SpeedChoice {
        value: "keep",
        label: "Keep current",
        description: "Retain the saved Claude speed preference.",
    },
    SpeedChoice {
        value: "standard",
        label: "Standard",
        description: "Use standard Claude speed and pricing.",
    },
    SpeedChoice {
        value: "fast",
        label: "Fast",
        description: "Faster responses at premium pricing. Switching speeds affects prompt-cache reuse.",
    },
];

impl ModelPickerState {
    pub(super) fn anthropic_speed_eligible(&self) -> bool {
        self.selection.as_ref().is_some_and(|selection| {
            let supported =
                |cfg: &VTCodeConfig| cfg.anthropic_speed_supported(&selection.provider_key, &selection.model_id);
            self.settings
                .vt_cfg
                .as_ref()
                .map_or_else(|| supported(&VTCodeConfig::default()), supported)
        })
    }

    pub(super) fn prompt_anthropic_speed(&mut self, renderer: &mut AnsiRenderer) -> Result<ModelPickerProgress> {
        let current = self
            .settings
            .vt_cfg
            .as_ref()
            .and_then(|cfg| cfg.provider.anthropic.speed)
            .unwrap_or(AnthropicSpeed::Standard)
            .as_str();
        let instructions = format!(
            "Claude speed (current: {current}). Fast uses premium pricing; switching speeds affects prompt-cache reuse."
        );
        if self.settings.inline_enabled {
            let mut items = vec![rendering::back_to_model_list_row()];
            for choice in &SPEED_CHOICES {
                items.push(vtcode_ui::design::list::choice(
                    choice.label,
                    Some(choice.description.to_owned()),
                    Some(InlineListSelection::ConfigAction(format!("{ACTION_PREFIX}{}", choice.value))),
                ));
            }
            renderer.show_list_modal_with_footer(
                "Claude speed",
                vec![instructions],
                items,
                Some(InlineListSelection::ConfigAction(format!("{ACTION_PREFIX}keep"))),
                None,
                Some("Esc cancels without saving.".to_owned()),
            );
            return Ok(ModelPickerProgress::InProgress);
        }
        if !self.plain_mode_active {
            let entries: Vec<_> = SPEED_CHOICES
                .iter()
                .map(|choice| SelectionEntry::new(choice.label.to_owned(), Some(choice.description.to_owned())))
                .collect();
            match run_interactive_selection("Claude speed", &instructions, &entries, 0) {
                Ok(Some(index)) => {
                    let choice = SPEED_CHOICES
                        .get(index)
                        .ok_or_else(|| anyhow!("Invalid Claude speed selection index"))?;
                    return self.handle_anthropic_speed(renderer, choice.value);
                }
                Ok(None) => return Ok(ModelPickerProgress::Cancelled),
                Err(err) if err.is::<SelectionInterrupted>() => return Err(err),
                Err(err) => {
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Interactive Claude speed selector unavailable ({err}). Falling back to manual input."
                        ),
                    )?;
                    self.plain_mode_active = true;
                }
            }
        }
        renderer.line(MessageStyle::Info, &format!("{instructions} Choose keep, standard, or fast; cancel aborts."))?;
        Ok(ModelPickerProgress::InProgress)
    }

    pub(super) fn handle_anthropic_speed(
        &mut self,
        renderer: &mut AnsiRenderer,
        input: &str,
    ) -> Result<ModelPickerProgress> {
        let input = input.strip_prefix(ACTION_PREFIX).unwrap_or(input);
        let speed = match input.to_ascii_lowercase().as_str() {
            "keep" | "skip" => None,
            "standard" => Some(AnthropicSpeed::Standard),
            "fast" => Some(AnthropicSpeed::Fast),
            _ => {
                renderer.line(MessageStyle::Error, "Choose keep, standard, or fast; cancel aborts.")?;
                return Ok(ModelPickerProgress::InProgress);
            }
        };
        if !self.anthropic_speed_eligible() {
            renderer.line(MessageStyle::Error, "Claude speed requires native Anthropic with Opus 5.5, 5, or 4.8. Select an eligible route with /model.")?;
            return Ok(ModelPickerProgress::InProgress);
        }
        self.selected_anthropic_speed = speed;
        self.finish_after_speed(renderer)
    }
}
