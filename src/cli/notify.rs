use anyhow::Result;
use vtcode_core::notifications::{
    NotificationEvent, apply_global_notification_config_from_vtcode, send_global_notification,
};

use crate::startup::StartupContext;

pub async fn handle_notify_command(
    startup: &StartupContext,
    title: Option<String>,
    message: String,
) -> Result<()> {
    apply_global_notification_config_from_vtcode(&startup.config)?;
    send_global_notification(NotificationEvent::Custom {
        title: title.unwrap_or_default(),
        message,
    })
    .await?;
    Ok(())
}
