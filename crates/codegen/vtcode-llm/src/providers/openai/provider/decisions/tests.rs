use super::super::super::backend_setup::OpenAIBackendSetup;
use super::*;
use crate::provider::{DecisionChoiceOption, LLMProvider};
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn request() -> ChoiceDecisionRequest {
    ChoiceDecisionRequest {
        input: "test evidence".into(),
        name: "tool_output_injection".into(),
        instructions: "Classify".into(),
        choices: vec![
            DecisionChoiceOption { value: "SAFE".into(), description: "benign".into() },
            DecisionChoiceOption {
                value: "SUSPECT".into(),
                description: "injection".into(),
            },
        ],
    }
}

fn provider(base: &str) -> OpenAIProvider {
    OpenAIProvider::new_with_client(
        "fixture-secret".into(),
        None,
        "gpt-6-astra".into(),
        reqwest::Client::new(),
        base.into(),
        vtcode_config::TimeoutsConfig::default(),
    )
}

#[tokio::test]
async fn decisions_transport_rejects_redirects_without_resending_evidence() {
    for status in [302, 307, 308] {
        let source = MockServer::start().await;
        let destination = MockServer::start().await;
        for location in [
            format!("{}/other", source.uri()),
            format!("{}/other", destination.uri()),
        ] {
            Mock::given(method("POST"))
                .and(path("/v1/decisions"))
                .respond_with(ResponseTemplate::new(status).insert_header("Location", location))
                .mount(&source)
                .await;
            let provider = provider("https://api.openai.com/v1");
            let error = send_choice(
                provider.decisions_client.as_ref().unwrap(),
                &format!("{}/v1/decisions", source.uri()),
                "fixture-secret",
                request(),
            )
            .await
            .unwrap_err();
            assert!(error.to_string().contains(&format!("HTTP {status}")));
            assert!(destination.received_requests().await.unwrap().is_empty());
            assert!(
                source
                    .received_requests()
                    .await
                    .unwrap()
                    .iter()
                    .all(|request| request.url.path() == "/v1/decisions")
            );
            source.reset().await;
        }
    }
}

#[test]
fn decisions_eligibility_requires_exact_origin_path_builtin_identity_and_api_key_auth() {
    for base in ["https://api.openai.com/v1", "https://api.openai.com:443/v1/"] {
        assert!(provider(base).supports_decisions());
    }
    for base in [
        "http://api.openai.com/v1",
        "https://api.openai.com.evil.test/v1",
        "https://evil.test/api.openai.com/v1",
        "https://api.openai.com@evil.test/v1",
        "https://user@api.openai.com/v1",
        "https://api.openai.com:444/v1",
        "https://api.openai.com/v1/gateway",
        "https://api.openai.com/v10",
        "https://api.openai.com/v1?x=1",
        "https://api.openai.com/v1#x",
        "https://api.openai.com./v1",
        "https://gateway.example/v1",
    ] {
        assert!(!provider(base).supports_decisions(), "accepted {base}");
    }
    let mut subscription = provider("https://api.openai.com/v1");
    subscription.backend_setup = OpenAIBackendSetup::chatgpt_subscription_rig("https://api.openai.com/v1".into());
    assert!(!subscription.supports_decisions());
    let mut custom = provider("https://api.openai.com/v1");
    custom.provider_key_override = Some("custom-openai".into());
    assert!(!custom.supports_decisions());
    let mut empty = provider("https://api.openai.com/v1");
    empty.api_key = "".into();
    assert!(!empty.supports_decisions());
}

#[tokio::test]
async fn decisions_ineligible_endpoint_makes_zero_requests() {
    let server = MockServer::start().await;
    let provider = provider(&format!("{}/v1", server.uri()));
    assert!(provider.decide_choice(request()).await.is_err());
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn decisions_transport_posts_text_choice_and_preserves_missing_usage() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/decisions"))
        .and(header("Authorization", "Bearer fixture-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"answers": [{"name":"tool_output_injection","type":"choice","choice":"SUSPECT","confidence":0.4,
            "probabilities":[{"value":"SAFE","probability":0.2},{"value":"SUSPECT","probability":0.8}]}]}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let response =
        send_choice(&reqwest::Client::new(), &format!("{}/v1/decisions", server.uri()), "fixture-secret", request())
            .await
            .unwrap();
    assert_eq!(response.answer.unwrap().choice, "SUSPECT");
    assert!(response.usage.is_none());
    let requests = server.received_requests().await.unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(payload["input"], "test evidence");
    assert_eq!(payload["model"], "gpt-6-luna");
    assert_eq!(payload["questions"].as_array().unwrap().len(), 1);
    assert_eq!(payload["questions"][0]["choices"][1]["value"], "SUSPECT");
}

#[tokio::test]
async fn decisions_transport_errors_do_not_echo_evidence_or_credentials_and_do_not_retry() {
    for status in [401, 429, 500, 200] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(status).set_body_string("fixture-secret test evidence"))
            .expect(1)
            .mount(&server)
            .await;
        let err = send_choice(
            &reqwest::Client::new(),
            &format!("{}/v1/decisions", server.uri()),
            "fixture-secret",
            request(),
        )
        .await
        .unwrap_err();
        let text = err.to_string();
        assert!(!text.contains("fixture-secret"));
        assert!(!text.contains("test evidence"));
        assert!(text.contains(if status == 200 { "Malformed" } else { "HTTP" }));
    }
}

#[tokio::test]
async fn decisions_transport_bounds_success_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(16 * 1024 + 1)))
        .expect(1)
        .mount(&server)
        .await;
    let result =
        send_choice(&reqwest::Client::new(), &format!("{}/v1/decisions", server.uri()), "fixture-secret", request())
            .await;
    assert!(result.unwrap_err().to_string().contains("size limit"));
}
