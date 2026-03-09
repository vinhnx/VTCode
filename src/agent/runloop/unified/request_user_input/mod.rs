mod modal;
mod options;
mod schema;
mod suggestions;

pub(crate) use modal::execute_request_user_input_tool;

#[cfg(test)]
mod tests {
    use super::modal::build_question_items;
    use super::options::{resolve_question_options, sanitize_provided_options};
    use super::schema::{
        RequestUserInputOption, RequestUserInputQuestion, normalize_request_user_input_args,
    };
    use super::suggestions::generate_suggested_options;
    use serde_json::json;
    use vtcode_tui::{InlineListSelection, WizardModalMode};

    fn prompt_question_with_hints() -> RequestUserInputQuestion {
        RequestUserInputQuestion {
            id: "system_prompt_plan".to_string(),
            header: "Direction".to_string(),
            question: "Which area should we prioritize to improve plan mode behavior?".to_string(),
            options: None,
            focus_area: Some("system prompt".to_string()),
            analysis_hints: vec!["navigation loop".to_string(), "stream timeout".to_string()],
        }
    }

    #[test]
    fn generates_prompt_specific_suggestions() {
        let question = prompt_question_with_hints();
        let options = generate_suggested_options(&question).expect("expected generated options");

        assert!((1..=3).contains(&options.len()));
        assert!(options[0].label.contains("(Recommended)"));
        assert!(
            options
                .iter()
                .any(|option| option.label.contains("fallback") || option.label.contains("Loop"))
        );
    }

    #[test]
    fn generates_weakness_aware_prompt_options() {
        let question = RequestUserInputQuestion {
            id: "prompt_improvement".to_string(),
            header: "Direction".to_string(),
            question: "Which system prompt improvement should we prioritize?".to_string(),
            options: None,
            focus_area: Some("system_prompt".to_string()),
            analysis_hints: vec![
                "Redundancy exists between prompt variants".to_string(),
                "Missing explicit guidance for failure patterns".to_string(),
            ],
        };

        let options = generate_suggested_options(&question).expect("expected generated options");
        assert!((1..=3).contains(&options.len()));
        assert!(options.iter().any(|opt| {
            opt.label.contains("redundancy")
                || opt.label.contains("Failure pattern")
                || opt.label.contains("Prompt")
        }));
    }

    #[test]
    fn generates_planning_options_for_goal_constraints_questions() {
        let question = RequestUserInputQuestion {
            id: "constraints".to_string(),
            header: "Plan".to_string(),
            question: "Break the work into 3-7 composable steps. For each step include target file(s) and a concrete expected outcome.".to_string(),
            options: None,
            focus_area: None,
            analysis_hints: Vec::new(),
        };

        let options = generate_suggested_options(&question).expect("expected planning options");
        assert!((1..=3).contains(&options.len()));
        assert!(options[0].label.contains("(Recommended)"));
    }

    #[test]
    fn generates_distinct_options_for_outcome_steps_and_verification_questions() {
        let outcome_question = RequestUserInputQuestion {
            id: "q1".to_string(),
            header: "Q1".to_string(),
            question: "What user-visible outcome should this change deliver, and what constraints or non-goals must be respected?".to_string(),
            options: None,
            focus_area: Some("system_prompt".to_string()),
            analysis_hints: vec![
                "Redundancy exists between prompt variants".to_string(),
                "Missing explicit guidance for failure patterns".to_string(),
            ],
        };
        let steps_question = RequestUserInputQuestion {
            id: "q2".to_string(),
            header: "Q2".to_string(),
            question: "Break the work into 3-7 composable steps. For each step include target file(s) and a concrete expected outcome.".to_string(),
            options: None,
            focus_area: Some("system_prompt".to_string()),
            analysis_hints: vec![
                "Redundancy exists between prompt variants".to_string(),
                "Missing explicit guidance for failure patterns".to_string(),
            ],
        };
        let verification_question = RequestUserInputQuestion {
            id: "q3".to_string(),
            header: "Q3".to_string(),
            question: "For each step, what exact command or manual check proves it is complete?"
                .to_string(),
            options: None,
            focus_area: Some("system_prompt".to_string()),
            analysis_hints: vec![
                "Redundancy exists between prompt variants".to_string(),
                "Missing explicit guidance for failure patterns".to_string(),
            ],
        };

        let outcome = generate_suggested_options(&outcome_question).expect("outcome options");
        let steps = generate_suggested_options(&steps_question).expect("step options");
        let verification =
            generate_suggested_options(&verification_question).expect("verification options");

        let outcome_labels = outcome
            .iter()
            .map(|opt| opt.label.clone())
            .collect::<Vec<_>>();
        let step_labels = steps
            .iter()
            .map(|opt| opt.label.clone())
            .collect::<Vec<_>>();
        let verification_labels = verification
            .iter()
            .map(|opt| opt.label.clone())
            .collect::<Vec<_>>();

        assert_ne!(outcome_labels, step_labels);
        assert_ne!(step_labels, verification_labels);
        assert_ne!(outcome_labels, verification_labels);

        assert!(outcome[0].label.contains("Recommended"));
        assert!(steps[0].label.contains("Recommended"));
        assert!(verification[0].label.contains("Recommended"));
    }

    #[test]
    fn provided_duplicate_options_are_regenerated_per_question() {
        let duplicate_options = vec![
            RequestUserInputOption {
                label: "Minimal implementation slice (Recommended)".to_string(),
                description: "Ship only the smallest possible scope.".to_string(),
            },
            RequestUserInputOption {
                label: "Balanced implementation".to_string(),
                description: "Ship medium scope with moderate risk.".to_string(),
            },
            RequestUserInputOption {
                label: "Comprehensive implementation".to_string(),
                description: "Ship full scope with deeper validation.".to_string(),
            },
        ];

        let questions = vec![
            RequestUserInputQuestion {
                id: "goal".to_string(),
                header: "Goal".to_string(),
                question: "What user-visible outcome should this change deliver, and what constraints or non-goals must be respected?".to_string(),
                options: Some(duplicate_options.clone()),
                focus_area: None,
                analysis_hints: Vec::new(),
            },
            RequestUserInputQuestion {
                id: "constraints".to_string(),
                header: "Plan".to_string(),
                question: "Break the work into 3-7 composable steps. For each step include target file(s) and a concrete expected outcome.".to_string(),
                options: Some(duplicate_options.clone()),
                focus_area: None,
                analysis_hints: Vec::new(),
            },
            RequestUserInputQuestion {
                id: "verification".to_string(),
                header: "Verification".to_string(),
                question: "For each step, what exact command or manual check proves it is complete?"
                    .to_string(),
                options: Some(duplicate_options),
                focus_area: None,
                analysis_hints: Vec::new(),
            },
        ];

        let resolved = resolve_question_options(&questions);
        assert_eq!(resolved.len(), 3);

        let goal_labels = resolved[0]
            .as_ref()
            .expect("goal options should resolve")
            .iter()
            .map(|option| option.label.clone())
            .collect::<Vec<_>>();
        let step_labels = resolved[1]
            .as_ref()
            .expect("step options should resolve")
            .iter()
            .map(|option| option.label.clone())
            .collect::<Vec<_>>();
        let verification_labels = resolved[2]
            .as_ref()
            .expect("verification options should resolve")
            .iter()
            .map(|option| option.label.clone())
            .collect::<Vec<_>>();

        assert_ne!(goal_labels, step_labels);
        assert_ne!(step_labels, verification_labels);
        assert_ne!(goal_labels, verification_labels);
    }

    #[test]
    fn valid_provided_options_are_preserved() {
        let provided_options = vec![
            RequestUserInputOption {
                label: "Outcome KPI (Recommended)".to_string(),
                description: "Define one measurable user-visible result.".to_string(),
            },
            RequestUserInputOption {
                label: "Constraint checklist".to_string(),
                description: "Lock boundaries before implementation.".to_string(),
            },
            RequestUserInputOption {
                label: "MVP boundary".to_string(),
                description: "Limit scope to the smallest deliverable.".to_string(),
            },
        ];

        let questions = vec![RequestUserInputQuestion {
            id: "goal".to_string(),
            header: "Goal".to_string(),
            question: "What user-visible outcome should this change deliver, and what constraints or non-goals must be respected?".to_string(),
            options: Some(provided_options.clone()),
            focus_area: None,
            analysis_hints: Vec::new(),
        }];

        let resolved = resolve_question_options(&questions);
        let resolved_options = resolved[0]
            .as_ref()
            .expect("provided options should be preserved");

        assert_eq!(resolved_options.len(), provided_options.len());
        for (resolved_option, provided_option) in resolved_options.iter().zip(provided_options) {
            assert_eq!(resolved_option.label, provided_option.label);
            assert_eq!(resolved_option.description, provided_option.description);
        }
    }

    #[test]
    fn id_keyword_does_not_override_question_text_intent() {
        let question = RequestUserInputQuestion {
            id: "constraints".to_string(),
            header: "Plan".to_string(),
            question: "For each step, what exact command or manual check proves it is complete?"
                .to_string(),
            options: None,
            focus_area: None,
            analysis_hints: Vec::new(),
        };

        let options = generate_suggested_options(&question).expect("verification options");
        let labels = options
            .iter()
            .map(|option| option.label.to_lowercase())
            .collect::<Vec<_>>();

        assert!(
            labels
                .iter()
                .any(|label| label.contains("command-based proof"))
        );
        assert!(
            !labels
                .iter()
                .any(|label| label.contains("dependency-first slices"))
        );
    }

    #[test]
    fn option_questions_add_explicit_custom_note_choice() {
        let question = RequestUserInputQuestion {
            id: "scope".to_string(),
            header: "Scope".to_string(),
            question: "Pick direction".to_string(),
            options: Some(vec![
                RequestUserInputOption {
                    label: "Option A".to_string(),
                    description: "A".to_string(),
                },
                RequestUserInputOption {
                    label: "Option B".to_string(),
                    description: "B".to_string(),
                },
            ]),
            focus_area: None,
            analysis_hints: Vec::new(),
        };

        let items = build_question_items(&question);
        assert_eq!(items.len(), 3);
        assert!(items[2].title.contains("Custom note"));

        let selection = items[2]
            .selection
            .clone()
            .expect("expected selection for other choice");
        match selection {
            InlineListSelection::RequestUserInputAnswer {
                selected, other, ..
            } => {
                assert!(selected.is_empty());
                assert_eq!(other, Some(String::new()));
            }
            _ => panic!("expected request_user_input selection"),
        }
    }

    #[test]
    fn falls_back_to_generic_options_when_no_suggestions_apply() {
        let question = RequestUserInputQuestion {
            id: "env".to_string(),
            header: "Env".to_string(),
            question: "What environment are you using?".to_string(),
            options: None,
            focus_area: None,
            analysis_hints: Vec::new(),
        };

        let items = build_question_items(&question);
        assert_eq!(items.len(), 4);
        assert!(items[0].title.contains("(Recommended)"));
        assert!(items[3].title.contains("Custom note"));
    }

    #[test]
    fn structured_payload_normalizes_to_multi_step_mode() {
        let args = json!({
            "questions": [
                {
                    "id": "scope",
                    "header": "Scope",
                    "question": "Which direction should we take?",
                    "options": [
                        {"label": "Minimal (Recommended)", "description": "Smallest viable slice"},
                        {"label": "Full", "description": "Complete implementation"}
                    ]
                }
            ]
        });

        let normalized = normalize_request_user_input_args(&args).expect("normalize structured");
        assert_eq!(normalized.args.questions.len(), 1);
        assert_eq!(normalized.wizard_mode, WizardModalMode::MultiStep);
        assert_eq!(normalized.current_step, 0);
        assert_eq!(normalized.title_override, None);
        assert_eq!(normalized.freeform_label.as_deref(), Some("Custom note"));
        assert_eq!(
            normalized.freeform_placeholder.as_deref(),
            Some("Type your response...")
        );
    }

    #[test]
    fn legacy_payload_is_rejected() {
        let legacy_args = json!({
            "question": "Choose one",
            "tabs": [
                {
                    "id": "scope",
                    "title": "Scope",
                    "items": [
                        {"id": "minimal", "title": "Minimal scope"},
                        {"id": "full", "title": "Full scope"}
                    ]
                }
            ]
        });
        let result = normalize_request_user_input_args(&legacy_args);
        assert!(result.is_err());
    }

    #[test]
    fn normalize_rejects_non_snake_case_ids() {
        let args = json!({
            "questions": [
                {
                    "id": "GoalQuestion",
                    "header": "Goal",
                    "question": "What outcome matters most?"
                }
            ]
        });

        let result = normalize_request_user_input_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn normalize_rejects_headers_over_twelve_chars() {
        let args = json!({
            "questions": [
                {
                    "id": "goal",
                    "header": "HeaderTooLong",
                    "question": "What outcome matters most?"
                }
            ]
        });

        let result = normalize_request_user_input_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn sanitize_provided_options_drops_other_and_duplicates() {
        let options = vec![
            RequestUserInputOption {
                label: "A (Recommended)".to_string(),
                description: "Choice A".to_string(),
            },
            RequestUserInputOption {
                label: "Other".to_string(),
                description: "Custom response".to_string(),
            },
            RequestUserInputOption {
                label: "A".to_string(),
                description: "Duplicate A".to_string(),
            },
            RequestUserInputOption {
                label: "B".to_string(),
                description: "Choice B".to_string(),
            },
        ];

        let sanitized = sanitize_provided_options(&options);
        let labels = sanitized
            .iter()
            .map(|option| option.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["A (Recommended)", "B"]);
    }
}
