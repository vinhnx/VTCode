use super::*;

/// Validate grouped choice identities against their owning wizard step.
pub fn host_interaction_response_grouped(
    interaction: &HostInteraction,
    groups: &[Vec<String>],
    answer_texts: &[Option<String>],
    cancelled: bool,
) -> Option<InlineEvent> {
    if cancelled {
        return host_interaction_response(interaction, &[], &[], true);
    }
    if !interaction.wizard || interaction.steps.is_empty() || groups.len() != interaction.steps.len() {
        return None;
    }
    let mut selections = Vec::new();
    for (index, step) in interaction.steps.iter().enumerate() {
        let ids = groups.get(index)?;
        if !step.multi_select && ids.len() > 1 {
            return None;
        }
        if ids.iter().any(|id| !step.choices.iter().any(|choice| &choice.id == id)) {
            return None;
        }
        let text = answer_texts
            .get(index)
            .and_then(Option::as_deref)
            .map(str::trim)
            .filter(|text| !text.is_empty());
        if text.is_some() && !step.allow_freeform {
            return None;
        }
        if !step.multi_select {
            let selected = ids.first().and_then(|id| step.choices.iter().find(|choice| &choice.id == id));
            let mut selection = if let Some(choice) = selected {
                choice.selection.clone()
            } else {
                let question_id = step.choices.iter().find_map(|choice| match &choice.selection {
                    InlineListSelection::RequestUserInputAnswer { question_id, .. } => Some(question_id.clone()),
                    _ => None,
                })?;
                text?;
                InlineListSelection::RequestUserInputAnswer { question_id, selected: Vec::new(), other: None }
            };
            if let Some(text) = text {
                apply_freeform_answer(&mut selection, text);
            }
            selections.push(selection);
            continue;
        }
        let question_id = step.choices.iter().find_map(|choice| match &choice.selection {
            InlineListSelection::RequestUserInputAnswer { question_id, .. } => Some(question_id.clone()),
            _ => None,
        })?;
        let mut selected = Vec::new();
        for choice in &step.choices {
            if ids.contains(&choice.id) {
                let InlineListSelection::RequestUserInputAnswer { selected: values, .. } = &choice.selection else {
                    return None;
                };
                for value in values {
                    if !selected.contains(value) {
                        selected.push(value.clone());
                    }
                }
            }
        }
        if selected.is_empty() && text.is_none() {
            return None;
        }
        selections.push(InlineListSelection::RequestUserInputAnswer {
            question_id,
            selected,
            other: text.map(str::to_string),
        });
    }
    Some(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Wizard(selections))))
}
