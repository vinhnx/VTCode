use serde_json::Value;
/// Integration test to ensure constants.rs stays in sync with docs/models.json
/// This test enforces the "Always check ./docs/models.json" rule from the project guidelines.
use std::collections::HashSet;
use std::fs;
use vtcode_core::config::constants::{model_helpers, models};

#[test]
#[ignore]
fn constants_cover_models_json() {
    let json = fs::read_to_string("docs/models.json").expect(
        "Failed to read docs/models.json. Make sure you're running tests from the project root.",
    );

    let models_data: Value =
        serde_json::from_str(&json).expect("Failed to parse docs/models.json as valid JSON");

    let providers = models_data
        .as_object()
        .expect("docs/models.json should be a JSON object");

    // Track which providers we validate to ensure we don't miss any
    let mut validated_providers = HashSet::new();

    for (provider_name, provider_spec) in providers {
        let provider_models = provider_spec.get("models").and_then(|m| m.as_object());

        if provider_models.is_none() {
            continue; // Skip providers without models section
        }

        let provider_models = provider_models.unwrap();
        let model_ids_from_json: HashSet<&str> =
            provider_models.keys().map(|s| s.as_str()).collect();

        // Check our supported providers
        let constants_models = match provider_name.as_str() {
            "openai" => {
                validated_providers.insert("openai");
                Some(models::openai::SUPPORTED_MODELS)
            }
            "anthropic" => {
                validated_providers.insert("anthropic");
                Some(models::anthropic::SUPPORTED_MODELS)
            }
            "google" => {
                validated_providers.insert("google");
                Some(models::google::SUPPORTED_MODELS)
            }
            "zai" => {
                validated_providers.insert("zai");
                Some(models::zai::SUPPORTED_MODELS)
            }
            _ => None, // Skip providers we don't have constants for yet
        };

        if let Some(constants_models) = constants_models {
            let model_ids_from_constants: HashSet<&str> =
                constants_models.iter().copied().collect();

            // Check for missing models in constants
            let missing_in_constants: Vec<_> = model_ids_from_json
                .difference(&model_ids_from_constants)
                .collect();

            // Check for extra models in constants (not in JSON)
            let extra_in_constants: Vec<_> = model_ids_from_constants
                .difference(&model_ids_from_json)
                .collect();

            assert!(
                missing_in_constants.is_empty(),
                "Missing models in constants.rs for provider '{}': {:?}\n\
                 Add these models to models::{}::SUPPORTED_MODELS in vtcode-core/src/config/constants.rs",
                provider_name,
                missing_in_constants,
                provider_name
            );

            assert!(
                extra_in_constants.is_empty(),
                "Extra models in constants.rs for provider '{}' not found in docs/models.json: {:?}\n\
                 Remove these models from models::{}::SUPPORTED_MODELS or add them to docs/models.json",
                provider_name,
                extra_in_constants,
                provider_name
            );

            // Validate that model_helpers functions work correctly
            for &model_id in constants_models {
                assert!(
                    model_helpers::is_valid(provider_name, model_id),
                    "Model validation failed for {}:{} - model_helpers::is_valid should return true",
                    provider_name,
                    model_id
                );
            }

            // Validate default model
            let default_model = model_helpers::default_for(provider_name).unwrap_or_else(|| {
                panic!("No default model found for provider '{}'", provider_name)
            });

            assert!(
                model_helpers::is_valid(provider_name, default_model),
                "Default model '{}' for provider '{}' is not in the supported models list",
                default_model,
                provider_name
            );

            assert!(
                constants_models.contains(&default_model),
                "Default model '{}' for provider '{}' is not in SUPPORTED_MODELS",
                default_model,
                provider_name
            );

            println!(
                "[SUCCESS] Provider '{}': {} models validated",
                provider_name,
                constants_models.len()
            );
        }
    }

    // Ensure we validated the expected providers
    let expected_providers = ["openai", "anthropic", "google", "openrouter", "zai"];
    for expected in &expected_providers {
        assert!(
            validated_providers.contains(expected),
            "Expected to validate provider '{}' but it was not found in docs/models.json or was skipped",
            expected
        );
    }

    println!(
        "[SUCCESS] All {} providers validated against docs/models.json",
        validated_providers.len()
    );
}

#[test]
fn model_helpers_validation_edge_cases() {
    // Test invalid provider
    assert_eq!(model_helpers::supported_for("nonexistent"), None);
    assert_eq!(model_helpers::default_for("nonexistent"), None);
    assert!(!model_helpers::is_valid("nonexistent", "any-model"));

    // Test invalid models for valid providers
    assert!(!model_helpers::is_valid("openai", "invalid-model-id"));
    assert!(!model_helpers::is_valid("anthropic", "invalid-model-id"));
    assert!(!model_helpers::is_valid("google", "invalid-model-id"));
    assert!(!model_helpers::is_valid("openrouter", "invalid-model-id"));

    // Test valid models for valid providers
    assert!(model_helpers::is_valid(
        "openai",
        models::openai::DEFAULT_MODEL
    ));
    assert!(model_helpers::is_valid(
        "anthropic",
        models::anthropic::DEFAULT_MODEL
    ));
    assert!(model_helpers::is_valid(
        "google",
        models::google::DEFAULT_MODEL
    ));
    assert!(model_helpers::is_valid(
        "openrouter",
        models::openrouter::DEFAULT_MODEL
    ));
    assert!(model_helpers::is_valid("zai", models::zai::DEFAULT_MODEL));
}

#[test]
fn backwards_compatibility_constants() {
    // Ensure old constant names still work
    assert!(!models::GEMINI_2_5_FLASH.is_empty());
    assert!(!models::GPT_5.is_empty());
    assert!(!models::CLAUDE_SONNET_4_5.is_empty());
    assert!(!models::CLAUDE_SONNET_4_20250514.is_empty());

    // Test that backwards compatibility constants match the new structure
    assert_eq!(models::GEMINI_2_5_FLASH, models::google::GEMINI_2_5_FLASH);
    assert_eq!(models::GPT_5, models::openai::GPT_5);
    assert_eq!(
        models::CLAUDE_SONNET_4_5,
        models::anthropic::CLAUDE_SONNET_4_5
    );
    assert_eq!(
        models::CLAUDE_SONNET_4_20250514,
        models::anthropic::CLAUDE_SONNET_4_20250514
    );
}
