use crate::prelude::*;

use anylm::api::Tool;
use system_utils::SystemMonitor;

static SYSTEM_MONITOR: State<SystemMonitor> = State::default();

pub fn tools_list() -> Vec<Tool> {
    vec![
        // ________________________________________
        //              BASIC INFO
        Tool::new(
            "system",
            "Returns static system information including operating system, CPU, GPU, RAM, motherboard, storage devices, and other hardware details.",
        ),
        // ________________________________________
        //              SYSTEM METRICS
        Tool::new(
            "metrics",
            "Returns current live system metrics including CPU usage, memory usage, temperatures, disk usage, network activity and other runtime statistics.",
        ),
        // ________________________________________
        //              DEVICES LIST
        Tool::new(
            "devices",
            "Returns a formatted list of currently connected hardware devices.",
        ),
    ]
}

#[log()]
pub async fn handle_system(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
    let info = SYSTEM_MONITOR.lock().await.info();
    let msg = str!(info);

    tx.send(Event::Answer(msg))?;
    Ok(())
}

#[log()]
pub async fn handle_metrics(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
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
pub async fn handle_devices(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
    let devices = SYSTEM_MONITOR
        .lock()
        .await
        .refresh_devices_with_interval(Duration::from_secs(60));
    let msg = str!(devices);

    info!("Connected devices enumerated.");
    tx.send(Event::Answer(msg))?;
    Ok(())
}
