use crate::prelude::*;

use system_utils::SystemMonitor;

static SYSTEM_MONITOR: State<SystemMonitor> = State::default();

#[log()]
pub async fn handle_system(tx: Sender<Bytes>) -> Result<()> {
    let info = SYSTEM_MONITOR.lock().await.info();
    let msg = str!(info);

    tx.send(Event::Answer(msg))?;
    Ok(())
}

#[log()]
pub async fn handle_metrics(tx: Sender<Bytes>) -> Result<()> {
    let metrics = SYSTEM_MONITOR
        .lock()
        .await
        .refresh_metrics_with_interval(Duration::from_secs(10));
    let msg = str!(metrics);

    info!("System metrics collected.");
    tx.send(Event::Answer(msg))?;
    Ok(())
}

#[log()]
pub async fn handle_devices(tx: Sender<Bytes>) -> Result<()> {
    let devices = SYSTEM_MONITOR
        .lock()
        .await
        .refresh_devices_with_interval(Duration::from_secs(60));
    let msg = str!(devices);

    info!("Connected devices enumerated.");
    tx.send(Event::Answer(msg))?;
    Ok(())
}
