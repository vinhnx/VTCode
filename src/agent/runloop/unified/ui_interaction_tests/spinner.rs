use super::*;

#[tokio::test]
async fn placeholder_spinner_restores_previous_input_status() {
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    handle.set_input_status(Some("provider".to_string()), Some("model".to_string()));

    {
        let spinner = PlaceholderSpinner::new(
            &handle,
            Some("provider".to_string()),
            Some("model".to_string()),
            "Loading model lists...",
        );
        spinner.finish();
    }

    let mut last_status: Option<(Option<String>, Option<String>)> = None;
    while let Ok(command) = rx.try_recv() {
        if let InlineCommand::SetInputStatus { left, right } = command {
            last_status = Some((left, right));
        }
    }

    assert_eq!(last_status, Some((Some("provider".to_string()), Some("model".to_string()))));
}
#[tokio::test]
async fn start_loading_status_uses_current_input_status_for_restore() {
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let state = InputStatusState {
        left: Some("provider".to_string()),
        right: Some("model".to_string()),
        ..Default::default()
    };

    {
        let spinner = start_loading_status(&handle, &state, "Saving memory note...");
        spinner.finish();
    }

    let mut saw_loading = false;
    let mut last_status: Option<(Option<String>, Option<String>)> = None;
    while let Ok(command) = rx.try_recv() {
        if let InlineCommand::SetInputStatus { left, right } = command {
            if left.as_deref().map(|text| text.contains("Saving memory note")).unwrap_or(false) {
                saw_loading = true;
            }
            last_status = Some((left, right));
        }
    }

    assert!(saw_loading);
    assert_eq!(last_status, Some((Some("provider".to_string()), Some("model".to_string()))));
}
#[tokio::test]
async fn placeholder_spinner_applies_message_updates_before_next_tick() {
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::with_progress(&handle, None, None, "Loading...", None);

    let _ = rx.recv().await;
    spinner.update_message("Still working");

    let updated_status = tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            match rx.recv().await {
                Some(InlineCommand::SetInputStatus { left, .. })
                    if left.as_deref().map(|text| text.contains("Still working")).unwrap_or(false) =>
                {
                    return left;
                }
                Some(_) => continue,
                None => panic!("spinner status channel closed unexpectedly"),
            }
        }
    })
    .await
    .expect("spinner message update should not wait for the polling interval");

    spinner.finish();

    assert!(
        updated_status
            .as_deref()
            .map(|text| text.contains("Still working"))
            .unwrap_or(false)
    );
}
#[tokio::test]
async fn placeholder_spinner_uses_latest_message_update() {
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::with_progress(&handle, None, None, "Loading...", None);

    let _ = rx.recv().await;
    spinner.update_message("First update");
    spinner.update_message("Final update");

    let updated_status = tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            match rx.recv().await {
                Some(InlineCommand::SetInputStatus { left, .. })
                    if left.as_deref().map(|text| text.contains("Final update")).unwrap_or(false) =>
                {
                    return left;
                }
                Some(_) => continue,
                None => panic!("spinner status channel closed unexpectedly"),
            }
        }
    })
    .await
    .expect("spinner should surface the latest message update");

    spinner.finish();

    assert!(
        updated_status
            .as_deref()
            .map(|text| text.contains("Final update"))
            .unwrap_or(false)
    );
}
