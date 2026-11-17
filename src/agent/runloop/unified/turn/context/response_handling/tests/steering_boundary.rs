use super::*;
use vtcode_core::core::agent::steering::{FollowUpSteer, SteerOutcome, SteeringMessage};

#[tokio::test]
async fn steer_arriving_during_the_final_response_is_applied_and_admits_another_request() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let (steer, receipt) = FollowUpSteer::channel("one more thing".to_string());
    sender.send(SteeringMessage::FollowUpInput(steer)).expect("in-flight steer");
    backing.set_steering_receiver(receiver);

    let (admitted, outcome, steered, carries_intent) = {
        let mut ctx = backing.turn_processing_context();
        let admitted = ctx.finalize_steering_before_completion().expect("final steering drain");
        let outcome = receipt.await.expect("receipt resolved at the final boundary");
        let intent_id = match &outcome {
            SteerOutcome::Accepted { intent_id } => intent_id.clone(),
            other => panic!("a steer received before the drain must be accepted, got {other:?}"),
        };
        let steered: Vec<String> = ctx
            .working_history
            .iter()
            .filter(|message| message.role == uni::MessageRole::User)
            .map(|message| message.content.as_text().into_owned())
            .collect();
        let carries_intent = ctx.working_history.iter().any(|message| {
            message.metadata.as_ref().and_then(|metadata| metadata.intent_id()) == Some(intent_id.as_str())
        });
        (admitted, outcome, steered, carries_intent)
    };

    assert!(admitted, "the final drain must report a steer so the turn continues");
    assert!(matches!(outcome, SteerOutcome::Accepted { .. }));
    assert_eq!(steered, vec!["one more thing".to_string()]);
    assert!(carries_intent, "the admitted steer must reach model history with its intent identity");

    // Admission is closed atomically with the final drain: a later arrival is
    // rejected instead of being stranded in the channel.
    let (late_steer, late_receipt) = FollowUpSteer::channel("too late".to_string());
    sender.send(SteeringMessage::FollowUpInput(late_steer)).expect("late steer");
    let mut ctx = backing.turn_processing_context();
    assert!(!ctx.finalize_steering_before_completion().expect("second final drain"));
    assert_eq!(
        late_receipt.await.expect("late receipt resolved"),
        SteerOutcome::Rejected {
            reason: vtcode_core::core::agent::steering::SteerRejection::TurnClosing
        }
    );
}

#[tokio::test]
async fn no_steer_at_the_final_boundary_leaves_history_untouched() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender.send(SteeringMessage::Resume).expect("unrelated control message");
    backing.set_steering_receiver(receiver);

    let (admitted, empty) = {
        let mut ctx = backing.turn_processing_context();
        let admitted = ctx.finalize_steering_before_completion().expect("final steering drain");
        (admitted, ctx.working_history.is_empty())
    };
    assert!(!admitted);
    assert!(empty);
}
