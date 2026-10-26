use crate::prelude::*;

use anylm::Schema;
use system_utils::{PowerManager, power::PowerMode};

#[derive(Deserialize, Schema)]
pub struct ScheduleAction {
    /// Power action to perform.
    #[schema(serde_json)]
    #[schema(variants = ["shutdown", "reboot", "suspend", "lock", "logout"])]
    mode: PowerMode,
    /// Optional ISO-8601 UTC datetime. If omitted, the action is executed immediately.
    #[schema(serde_json)]
    timestamp: Option<DateTime<Utc>>,
}

#[log(mode = %query.payload.mode)]
pub async fn handle_schedule(tx: Sender<Bytes>, query: ToolQuery<ScheduleAction>) -> Result<()> {
    let ToolQuery { payload, .. } = query;

    let local = payload
        .timestamp
        .map(|utc| {
            utc.with_timezone(&Local)
                .format("%A, %I:%M:%S %p (%:z), %B %d, %Y")
                .to_string()
        })
        .unwrap_or("now".into());

    match PowerManager::schedule(payload.mode, payload.timestamp).await {
        Ok(_) => {
            let msg = format!(
                "Scheduled power action: {mode}. Execution time: {local}.",
                mode = payload.mode
            );

            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }

        Err(e) => Err(format!("Power operation failed: {e}").into()),
    }
}

#[log()]
pub async fn handle_cancel(tx: Sender<Bytes>) -> Result<()> {
    let msg = match PowerManager::cancel().await {
        Some(mode) => format!("Scheduled power action canceled. Canceled action: {mode}."),
        None => "There is no scheduled power payload.".into(),
    };

    info!("{msg}");
    tx.send(Event::Answer(msg))?;
    Ok(())
}

#[log()]
pub async fn handle_status(tx: Sender<Bytes>) -> Result<()> {
    let msg = match PowerManager::status().await {
        Some(task) => {
            format!(
                "Scheduled {mode}. Execution time: {local}",
                mode = task.mode,
                local = task
                    .execute_at
                    .with_timezone(&Local)
                    .format("%A, %I:%M:%S %p (%:z), %B %d, %Y"),
            )
        }

        None => "No power action is currently scheduled.".into(),
    };

    info!("{msg}");
    tx.send(Event::Answer(msg))?;
    Ok(())
}
