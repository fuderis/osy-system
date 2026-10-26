use crate::prelude::*;

use anylm::Schema;
use system_utils::{AudioControl, MediaControl};

#[derive(Debug, Deserialize, Schema)]
pub struct SetAction {
    /// Target audio volume percentage (0-100).
    pub volume: u32,
}

#[derive(Debug, Deserialize, Schema)]
pub struct VolumeAction {
    /// Amount to change the audio volume by.
    pub amount: i32,
}

#[derive(Debug, Deserialize, Schema)]
pub struct SeekAction {
    /// Number of seconds to seek.
    pub seconds: i32,
}

#[log(volume = %query.payload.volume)]
pub async fn handle_set(tx: Sender<Bytes>, query: ToolQuery<SetAction>) -> Result<()> {
    let ToolQuery { payload, .. } = query;

    match AudioControl::set_volume(payload.volume).await {
        Ok(_) => {
            let msg = str!(
                "Audio volume updated successfully. Current volume: `{}%`.",
                payload.volume
            );
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to update audio volume: {e:?}").into()),
    }
}

#[log(amount = %query.payload.amount)]
pub async fn handle_volume(tx: Sender<Bytes>, query: ToolQuery<VolumeAction>) -> Result<()> {
    let ToolQuery { payload, .. } = query;
    let amount = payload.amount;

    let res = if amount >= 0 {
        AudioControl::increase_volume(amount.unsigned_abs() as u32).await
    } else {
        AudioControl::decrease_volume(amount.unsigned_abs() as u32).await
    };

    match res {
        Ok(volume) => {
            let action_type = if amount >= 0 {
                "increased"
            } else {
                "decreased"
            };
            let msg = format!(
                "The audio volume {action_type} successfully. Current volume: `{volume}%`."
            );
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to update the audio volume: {e:?}").into()),
    }
}

#[log()]
pub async fn handle_get(tx: Sender<Bytes>) -> Result<()> {
    match AudioControl::get_volume().await {
        Ok(volume) => {
            let msg = format!("The current audio volume level is `{volume}%`.");
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to get the audio volume: {e:?}").into()),
    }
}

#[log()]
pub async fn handle_mute(tx: Sender<Bytes>) -> Result<()> {
    match AudioControl::set_mute(true).await {
        Ok(_) => {
            let msg = "The audio muted successfully.";
            info!("{msg}");
            tx.send(Event::Answer(msg.into()))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to update audio mute state: {e:?}").into()),
    }
}

#[log()]
pub async fn handle_unmute(tx: Sender<Bytes>) -> Result<()> {
    match AudioControl::set_mute(false).await {
        Ok(_) => {
            let msg = "The audio unmuted successfully.";
            info!("{msg}");
            tx.send(Event::Answer(msg.into()))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to update audio mute state: {e:?}").into()),
    }
}

#[log()]
pub async fn handle_is_muted(tx: Sender<Bytes>) -> Result<()> {
    match AudioControl::is_muted().await {
        Ok(is_muted) => {
            let msg = if is_muted {
                "The audio is currently muted."
            } else {
                "The audio is currently unmuted."
            };
            info!("{msg}");
            tx.send(Event::Answer(msg.into()))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to get audio mute state: {e:?}").into()),
    }
}

// #[cfg(target_os = "linux")]
// #[log()]
// pub async fn handle_play(tx: Sender<Bytes>, ) -> Result<()> {
//     match MediaControl::play().await {
//         Ok(_) => {
//             let msg = "Media playback started successfully.";
//             info!("{msg}");
//             tx.send(Event::Answer(msg.into()))?;
//             Ok(())
//         }
//         Err(e) => Err(format!("Failed to start media playback: {e:?}").into()),
//     }
// }

// #[cfg(target_os = "linux")]
// #[log()]
// pub async fn handle_pause(tx: Sender<Bytes>, ) -> Result<()> {
//     match MediaControl::pause().await {
//         Ok(_) => {
//             let msg = "Media playback paused successfully.";
//             info!("{msg}");
//             tx.send(Event::Answer(msg.into()))?;
//             Ok(())
//         }
//         Err(e) => Err(format!("Failed to pause media playback: {e:?}").into()),
//     }
// }

#[log()]
pub async fn handle_play_pause(tx: Sender<Bytes>) -> Result<()> {
    match MediaControl::play_pause().await {
        Ok(_) => {
            let msg = "Media playback toggled successfully.";
            info!("{msg}");
            tx.send(Event::Answer(msg.into()))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to toggle media playback: {e:?}").into()),
    }
}

#[log()]
pub async fn handle_stop(tx: Sender<Bytes>) -> Result<()> {
    match MediaControl::stop().await {
        Ok(_) => {
            let msg = "Media playback stopped successfully.";
            info!("{msg}");
            tx.send(Event::Answer(msg.into()))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to stop media playback: {e:?}").into()),
    }
}

#[log()]
pub async fn handle_next(tx: Sender<Bytes>) -> Result<()> {
    match MediaControl::next_track().await {
        Ok(_) => {
            let msg = "Skipped to the next track successfully.";
            info!("{msg}");
            tx.send(Event::Answer(msg.into()))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to skip to the next track: {e:?}").into()),
    }
}

#[log()]
pub async fn handle_previous(tx: Sender<Bytes>) -> Result<()> {
    match MediaControl::previous_track().await {
        Ok(_) => {
            let msg = "Returned to the previous track successfully.";
            info!("{msg}");
            tx.send(Event::Answer(msg.into()))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to return to the previous track: {e:?}").into()),
    }
}

#[log(secs = %query.payload.seconds)]
pub async fn handle_seek(tx: Sender<Bytes>, query: ToolQuery<SeekAction>) -> Result<()> {
    let ToolQuery { payload, .. } = query;
    let secs = payload.seconds;

    let res = if secs >= 0 {
        MediaControl::seek_forward(secs.unsigned_abs() as u32).await
    } else {
        MediaControl::seek_backward(secs.unsigned_abs() as u32).await
    };

    match res {
        Ok(_) => {
            let msg = if secs >= 0 {
                str!("Media playback advanced by {} seconds successfully.", secs)
            } else {
                str!(
                    "Media playback rewound by {} seconds successfully.",
                    secs.abs()
                )
            };
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to seek media: {e:?}").into()),
    }
}

#[log()]
pub async fn handle_metadata(tx: Sender<Bytes>) -> Result<()> {
    match MediaControl::metadata().await {
        Ok(metadata) => {
            let msg = str!(metadata);

            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to retrieve media metadata: {e:?}").into()),
    }
}

#[log()]
pub async fn handle_position(tx: Sender<Bytes>) -> Result<()> {
    match MediaControl::position().await {
        Ok(position) => {
            let msg = format!("Current playback position: {:?}.", position);
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to retrieve playback position: {e:?}").into()),
    }
}

#[log()]
pub async fn handle_duration(tx: Sender<Bytes>) -> Result<()> {
    match MediaControl::duration().await {
        Ok(duration) => {
            let msg = format!("Current media duration: {:?}.", duration);
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to retrieve media duration: {e:?}").into()),
    }
}
