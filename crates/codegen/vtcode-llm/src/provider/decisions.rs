//! Text-only, single-question Decisions choice contract.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use vtcode_commons::tool_types::CompactStr;

use super::{LLMError, Usage};

/// The model supported by the experimental Decisions endpoint.
pub const DECISIONS_MODEL: &str = "gpt-6-luna";

/// One supplied text category and its meaning.
#[derive(Debug, Clone, Serialize)]
pub struct DecisionChoiceOption {
    pub value: CompactStr,
    pub description: CompactStr,
}

/// Evidence and instructions for exactly one named choice question.
/// Deliberately omits images, predicates, scores, and generation controls.
#[derive(Clone)]
pub struct ChoiceDecisionRequest {
    pub input: String,
    pub name: CompactStr,
    pub instructions: String,
    pub choices: Vec<DecisionChoiceOption>,
}

impl ChoiceDecisionRequest {
    pub(crate) fn payload(&self) -> Result<Value, LLMError> {
        if self.name.is_empty()
            || self.choices.len() < 2
            || self.choices.iter().enumerate().any(|(index, choice)| {
                choice.value.is_empty() || self.choices[..index].iter().any(|previous| previous.value == choice.value)
            })
        {
            return Err(LLMError::Provider {
                message: "Invalid Decisions choice question".to_owned(),
                metadata: None,
            });
        }
        Ok(json!({
            "model": DECISIONS_MODEL,
            "input": self.input,
            "questions": [{"type": "choice", "name": self.name, "instructions": self.instructions, "choices": self.choices}]
        }))
    }
}

/// A probability for one supplied category.
#[derive(Debug, Clone, Deserialize)]
pub struct DecisionProbability {
    pub value: CompactStr,
    pub probability: f64,
}

/// A validated choice answer. Confidence is retained without a threshold.
#[derive(Debug, Clone, Deserialize)]
pub struct ChoiceDecisionAnswer {
    pub name: CompactStr,
    pub choice: CompactStr,
    pub confidence: f64,
    pub probabilities: Vec<DecisionProbability>,
}

/// An inconclusive answer still retains independently parsed billed usage.
#[derive(Debug, Clone)]
pub struct ChoiceDecisionResponse {
    pub answer: Option<ChoiceDecisionAnswer>,
    pub usage: Option<Usage>,
}

impl ChoiceDecisionResponse {
    pub(crate) fn from_payload(payload: &Value, request: &ChoiceDecisionRequest) -> Self {
        let usage = parse_usage(payload.get("usage"));
        let answer = (|| {
            let answers = payload.get("answers")?.as_array()?;
            if answers.len() != 1 || answers.first()?.get("type")?.as_str()? != "choice" {
                return None;
            }
            let answer: ChoiceDecisionAnswer = serde_json::from_value(answers.first()?.clone()).ok()?;
            let allowed = |value: &str| request.choices.iter().any(|option| option.value == value);
            if answer.name != request.name
                || !allowed(&answer.choice)
                || !unit_interval(answer.confidence)
                || answer.probabilities.len() != request.choices.len()
                || answer.probabilities.iter().enumerate().any(|(index, probability)| {
                    !allowed(&probability.value)
                        || !unit_interval(probability.probability)
                        || answer.probabilities[..index]
                            .iter()
                            .any(|previous| previous.value == probability.value)
                })
            {
                return None;
            }
            Some(answer)
        })();
        Self { answer, usage }
    }
}

fn unit_interval(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn parse_usage(value: Option<&Value>) -> Option<Usage> {
    let value = value?;
    let input = u32::try_from(value.get("input_tokens")?.as_u64()?).ok()?;
    let output = u32::try_from(value.get("output_tokens")?.as_u64()?).ok()?;
    let total = u32::try_from(value.get("total_tokens")?.as_u64()?).ok()?;
    if input.checked_add(output)? != total {
        return None;
    }
    Some(Usage {
        prompt_tokens: input,
        completion_tokens: output,
        total_tokens: total,
        cached_prompt_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        iterations: None,
    })
}

#[cfg(test)]
mod tests;
