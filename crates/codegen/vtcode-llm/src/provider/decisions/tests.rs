use super::*;

fn request() -> ChoiceDecisionRequest {
    ChoiceDecisionRequest {
        input: "bounded evidence".to_owned(),
        name: "tool_output_injection".into(),
        instructions: "Classify".to_owned(),
        choices: vec![
            DecisionChoiceOption { value: "SAFE".into(), description: "benign".into() },
            DecisionChoiceOption {
                value: "SUSPECT".into(),
                description: "injection".into(),
            },
        ],
    }
}

fn response(choice: &str) -> Value {
    json!({"answers": [{"type": "choice", "name": "tool_output_injection", "choice": choice,
        "confidence": 0.2, "probabilities": [{"value":"SAFE", "probability": 0.1}, {"value":"SUSPECT", "probability": 0.9}]}],
        "usage": {"input_tokens": 137, "output_tokens": 0, "total_tokens": 137}})
}

#[test]
fn decisions_choice_validates_categories_without_a_confidence_threshold() {
    for choice in ["SAFE", "SUSPECT"] {
        let parsed = ChoiceDecisionResponse::from_payload(&response(choice), &request());
        assert_eq!(parsed.answer.unwrap().choice, choice);
        assert_eq!(parsed.usage.unwrap().prompt_tokens, 137);
    }
    assert!(!unit_interval(f64::NAN));
    assert!(!unit_interval(f64::INFINITY));
    assert!(unit_interval(0.0));
    assert!(unit_interval(1.0));
}

#[test]
fn decisions_choice_rejects_refusal_missing_duplicate_and_malformed_answers_but_keeps_usage() {
    let valid = response("SAFE");
    let mut cases = vec![
        json!([]),
        json!([{"type":"refusal", "name":"tool_output_injection"}]),
        json!([valid["answers"][0].clone(), valid["answers"][0].clone()]),
    ];
    for (field, value) in [
        ("type", json!("predicate")),
        ("name", json!("other")),
        ("choice", json!("UNKNOWN")),
        ("choice", json!(true)),
        ("confidence", json!(-0.01)),
        ("confidence", json!(1.01)),
        ("confidence", json!(null)),
        ("probabilities", json!([{"value":"SAFE", "probability":0.3}, {"value":"SAFE", "probability":0.7}])),
        (
            "probabilities",
            json!([{"value":"SAFE", "probability":-0.1}, {"value":"SUSPECT", "probability":1.1}]),
        ),
        (
            "probabilities",
            json!([{"value":"OTHER", "probability":0.5}, {"value":"SUSPECT", "probability":0.5}]),
        ),
        ("probabilities", json!([])),
    ] {
        let mut answer = valid["answers"][0].clone();
        answer[field] = value;
        cases.push(json!([answer]));
    }
    let mut answer = valid["answers"][0].clone();
    answer.as_object_mut().unwrap().remove("confidence");
    cases.push(json!([answer]));
    for answers in cases {
        let payload = json!({"answers": answers, "usage": valid["usage"]});
        let parsed = ChoiceDecisionResponse::from_payload(&payload, &request());
        assert!(parsed.answer.is_none(), "accepted {answers}");
        assert_eq!(parsed.usage.unwrap().prompt_tokens, 137);
    }
}

#[test]
fn decisions_missing_or_invalid_usage_stays_unknown() {
    for usage in [
        Value::Null,
        json!({}),
        json!({"input_tokens":-1,"output_tokens":0,"total_tokens":0}),
        json!({"input_tokens":137,"output_tokens":0,"total_tokens":138}),
    ] {
        let mut payload = response("SUSPECT");
        payload["usage"] = usage;
        let parsed = ChoiceDecisionResponse::from_payload(&payload, &request());
        assert!(parsed.answer.is_some());
        assert!(parsed.usage.is_none());
    }
}

#[test]
fn decisions_request_is_one_named_text_choice_and_rejects_duplicate_options() {
    let mut request = request();
    let payload = request.payload().unwrap();
    assert_eq!(payload["model"], "gpt-6-luna");
    assert_eq!(payload["input"], "bounded evidence");
    assert_eq!(payload["questions"].as_array().unwrap().len(), 1);
    assert_eq!(payload["questions"][0]["type"], "choice");
    assert_eq!(payload["questions"][0]["name"], "tool_output_injection");
    request.choices[1].value = "SAFE".into();
    assert!(request.payload().is_err());
}
