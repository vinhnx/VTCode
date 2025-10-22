use std::error::Error;

use vtcode_exec_events::{
    CommandExecutionItem, CommandExecutionStatus, ItemCompletedEvent, ItemStartedEvent,
    ItemUpdatedEvent, ThreadEvent, ThreadItem, ThreadItemDetails, ThreadStartedEvent,
    TurnCompletedEvent, TurnStartedEvent, Usage,
};

fn main() -> Result<(), Box<dyn Error>> {
    let timeline = sample_timeline();

    println!("# execution timeline (JSONL)");
    for event in &timeline {
        let json = serde_json::to_string(event)?;
        println!("{}", json);
    }

    println!("\n# completed commands");
    for event in &timeline {
        if let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = event {
            if let ThreadItemDetails::CommandExecution(command) = &item.details {
                println!(
                    "{} => status={} exit_code={:?}",
                    command.command,
                    status_label(&command.status),
                    command.exit_code
                );
            }
        }
    }

    Ok(())
}

fn sample_timeline() -> Vec<ThreadEvent> {
    vec![
        ThreadEvent::ThreadStarted(ThreadStartedEvent {
            thread_id: "workspace.setup".into(),
        }),
        ThreadEvent::TurnStarted(TurnStartedEvent::default()),
        ThreadEvent::ItemStarted(ItemStartedEvent {
            item: ThreadItem {
                id: "command.git-init".into(),
                details: ThreadItemDetails::CommandExecution(CommandExecutionItem {
                    command: "git init".into(),
                    aggregated_output: String::new(),
                    exit_code: None,
                    status: CommandExecutionStatus::InProgress,
                }),
            },
        }),
        ThreadEvent::ItemUpdated(ItemUpdatedEvent {
            item: ThreadItem {
                id: "command.git-init".into(),
                details: ThreadItemDetails::CommandExecution(CommandExecutionItem {
                    command: "git init".into(),
                    aggregated_output: "Initialized empty Git repository".into(),
                    exit_code: None,
                    status: CommandExecutionStatus::InProgress,
                }),
            },
        }),
        ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: "command.git-init".into(),
                details: ThreadItemDetails::CommandExecution(CommandExecutionItem {
                    command: "git init".into(),
                    aggregated_output: "Initialized empty Git repository".into(),
                    exit_code: Some(0),
                    status: CommandExecutionStatus::Completed,
                }),
            },
        }),
        ThreadEvent::TurnCompleted(TurnCompletedEvent {
            usage: Usage {
                input_tokens: 128,
                cached_input_tokens: 0,
                output_tokens: 32,
            },
        }),
    ]
}

fn status_label(status: &CommandExecutionStatus) -> &'static str {
    match status {
        CommandExecutionStatus::Completed => "completed",
        CommandExecutionStatus::Failed => "failed",
        CommandExecutionStatus::InProgress => "in_progress",
    }
}
