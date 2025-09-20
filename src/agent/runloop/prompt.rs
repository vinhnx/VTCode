use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::{factory::create_provider_with_config, provider as uni};

pub(crate) async fn refine_user_prompt_if_enabled(
    raw: &str,
    cfg: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> String {
    if std::env::var("VTCODE_PROMPT_REFINER_STUB").is_ok() {
        return format!("[REFINED] {}", raw);
    }
    let Some(vtc) = vt_cfg else {
        return raw.to_string();
    };
    if !vtc.agent.refine_prompts_enabled {
        return raw.to_string();
    }

    let provider_name = if cfg.provider.trim().is_empty() {
        "gemini".to_string()
    } else {
        cfg.provider.to_lowercase()
    };

    let refiner_model = if !vtc.agent.refine_prompts_model.is_empty() {
        vtc.agent.refine_prompts_model.clone()
    } else {
        match provider_name.as_str() {
            "openai" => vtcode_core::config::constants::models::openai::GPT_5_MINI.to_string(),
            _ => cfg.model.clone(),
        }
    };

    let Ok(refiner) = create_provider_with_config(
        &provider_name,
        Some(cfg.api_key.clone()),
        None,
        Some(refiner_model.clone()),
    ) else {
        return raw.to_string();
    };

    let req = uni::LLMRequest {
        messages: vec![uni::Message::user(raw.to_string())],
        system_prompt: None,
        tools: None,
        model: refiner_model,
        max_tokens: Some(800),
        temperature: Some(0.3),
        stream: false,
        tool_choice: Some(uni::ToolChoice::none()),
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: Some(vtc.agent.reasoning_effort.clone()),
    };

    match refiner
        .generate(req)
        .await
        .map(|response| response.content.unwrap_or_default())
    {
        Ok(text) if !text.trim().is_empty() => text,
        _ => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_prompt_refinement_applies_to_gemini_when_flag_disabled() {
        unsafe {
            std::env::set_var("VTCODE_PROMPT_REFINER_STUB", "1");
        }

        let cfg = CoreAgentConfig {
            model: vtcode_core::config::constants::models::google::GEMINI_2_5_FLASH_PREVIEW
                .to_string(),
            api_key: "test".to_string(),
            provider: "gemini".to_string(),
            workspace: std::env::current_dir().unwrap(),
            verbose: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        };

        let mut vt = VTCodeConfig::default();
        vt.agent.refine_prompts_enabled = true;

        let raw = "make me a list of files";
        let out = refine_user_prompt_if_enabled(raw, &cfg, Some(&vt)).await;

        assert!(out.starts_with("[REFINED] "));

        unsafe {
            std::env::remove_var("VTCODE_PROMPT_REFINER_STUB");
        }
    }
}
