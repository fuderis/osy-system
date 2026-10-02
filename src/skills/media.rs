use crate::prelude::*;

use anylm::{Schema, api::Tool};
use music_index::{MusicIndexer, SearchIntent};
use system_utils::{AudioControl, MediaControl};

static MUSIC_INDEX: State<Option<MusicIndexer>> = State::default();

pub fn tools_list() -> Vec<Tool> {
    vec![
        // ________________________________________
        // AUDIO CONTROL
        //
        Tool::typed::<SetVolumeAction>(
            "set",
            "Sets the system audio volume to the specified percentage.",
        ),
        Tool::new(
            "get",
            "Returns the current system audio volume percentage (0-100).",
        ),
        Tool::typed::<DeltaVolumeAction>(
            "increase",
            "Increases the system audio volume by the specified percentage.",
        ),
        Tool::typed::<DeltaVolumeAction>(
            "decrease",
            "Decreases the system audio volume by the specified percentage.",
        ),
        Tool::new(
            "is_muted",
            "Checks if the system audio is currently muted. Returns a boolean representation.",
        ),
        Tool::new("mute", "Mutes the system audio."),
        Tool::new("unmute", "Unmutes the system audio."),
        // ________________________________________
        // MUSIC INDEX
        //
        Tool::typed::<MusicAction>(
            "search",
            "Searches the local music library without starting playback. (If you need to play the music immediately, it’s better to use the play_music tool).",
        ),
        Tool::typed::<MusicAction>(
            "play",
            "Searches the local music library and immediately starts playback.",
        ),
        // ________________________________________
        // MEDIA CONTROL
        //
        // #[cfg(target_os = "linux")]
        // Tool::new("media_play", "Starts media playback."),
        // #[cfg(target_os = "linux")]
        // Tool::new("media_pause", "Pauses media playback."),
        Tool::new("media_play_pause", "Toggles between play and pause."),
        Tool::new("media_stop", "Stops media playback."),
        Tool::new("media_next_track", "Skips to the next track."),
        Tool::new("media_previous_track", "Returns to the previous track."),
        #[cfg(target_os = "linux")]
        Tool::typed::<SeekAction>(
            "media_seek_forward",
            "Seeks forward by the specified number of seconds.",
        ),
        #[cfg(target_os = "linux")]
        Tool::typed::<SeekAction>(
            "media_seek_backward",
            "Seeks backward by the specified number of seconds.",
        ),
        #[cfg(target_os = "linux")]
        Tool::new(
            "media_metadata",
            "Returns metadata for the currently playing media.",
        ),
        #[cfg(target_os = "linux")]
        Tool::new("media_position", "Returns the current playback position."),
        #[cfg(target_os = "linux")]
        Tool::new(
            "media_duration",
            "Returns the duration of the current media.",
        ),
    ]
}

// ============================================================================
// DTO STRUCTS
// ============================================================================

#[derive(Debug, Deserialize, Schema)]
pub struct SetVolumeAction {
    /// Target audio volume percentage (0-100).
    pub volume: u32,
}

#[derive(Debug, Deserialize, Schema)]
pub struct DeltaVolumeAction {
    /// Amount to change the audio volume by.
    pub amount: u32,
}

#[derive(Debug, Deserialize, Schema)]
pub struct MusicAction {
    /// Artist or band name.
    pub band: Option<String>,
    /// Album title.
    pub album: Option<String>,
    /// Track title.
    pub track: Option<String>,
    /// Music genre.
    pub genre: Option<String>,
}

#[derive(Debug, Deserialize, Schema)]
pub struct SeekAction {
    /// Number of seconds to seek.
    pub seconds: u32,
}

#[log(volume = %action.volume)]
pub async fn handle_set(tx: Sender<Bytes>, action: SetVolumeAction) -> Result<()> {
    match AudioControl::set_volume(action.volume).await {
        Ok(_) => {
            let msg = str!(
                "Audio volume updated successfully. Current volume: `{}%`.",
                action.volume
            );
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to update audio volume: {e:?}").into()),
    }
}

#[log(amount = %action.amount)]
pub async fn handle_increase(tx: Sender<Bytes>, action: DeltaVolumeAction) -> Result<()> {
    match AudioControl::increase_volume(action.amount).await {
        Ok(volume) => {
            let msg =
                format!("The audio volume increased successfully. Current volume: `{volume}%`.",);
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to update the audio volume: {e:?}").into()),
    }
}

#[log(amount = %action.amount)]
pub async fn handle_decrease(tx: Sender<Bytes>, action: DeltaVolumeAction) -> Result<()> {
    match AudioControl::decrease_volume(action.amount).await {
        Ok(volume) => {
            let msg =
                format!("The audio volume decreased successfully. Current volume: `{volume}%`.",);
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to update the audio volume: {e:?}").into()),
    }
}

#[log()]
pub async fn handle_get(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
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
pub async fn handle_mute(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
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
pub async fn handle_unmute(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
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
pub async fn handle_is_muted(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
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

async fn music_index() -> Result<MusicIndexer> {
    if MUSIC_INDEX.get().is_none() {
        let index = MusicIndexer::scan_default(path!("$cache$/music-index.json")).await?;
        MUSIC_INDEX.set(Some(index)).await;
    }

    MUSIC_INDEX
        .get()
        .as_ref()
        .clone()
        .ok_or_else(|| format!("Failed to initialize music indexer").into())
}

impl From<MusicAction> for SearchIntent {
    fn from(mut action: MusicAction) -> Self {
        if action.band.is_none()
            && action.album.is_none()
            && action.genre.is_none()
            && action.track.is_some()
        {
            SearchIntent::Global(action.track.take().unwrap())
        } else {
            SearchIntent::Targeted {
                band: action.band,
                album: action.album,
                track: action.track,
                genre: action.genre,
            }
        }
    }
}

#[log()]
pub async fn handle_search(tx: Sender<Bytes>, action: MusicAction) -> Result<()> {
    let music_index = music_index().await?;

    let target = music_index.search(action.into());
    let tracks = target.tracks();

    let msg = if tracks.is_empty() {
        format!("No matching music was found.")
    } else {
        let limit = 30;
        let preview: Vec<_> = tracks
            .iter()
            .take(limit)
            .map(|t| format!("- {} — {} ({})", t.band, t.name, t.path.display()))
            .collect();

        let extra_count = tracks.len().saturating_sub(limit);
        let extra_msg = if extra_count > 0 {
            format!("\n...and {extra_count} more tracks.")
        } else {
            String::new()
        };

        str!(
            "Found {} matching track(s):\n{}{}",
            tracks.len(),
            preview.join("\n"),
            extra_msg
        )
    };

    info!("{msg}");
    tx.send(Event::Answer(msg))?;

    Ok(())
}

#[log()]
pub async fn handle_play(tx: Sender<Bytes>, action: MusicAction) -> Result<()> {
    let music_index = music_index().await?;

    let target = music_index.search(action.into());
    let tracks = target.tracks();

    if tracks.is_empty() {
        let msg = format!("No matching music was found.");
        info!("{msg}");
        tx.send(Event::Answer(msg))?;
        return Ok(());
    }

    music_index
        .play(target, path!("$cache$/playlist.m3u"))
        .await?;

    let msg = str!(
        "Started playback of `{count}` track(s).",
        count = tracks.len()
    );

    info!("{msg}");
    tx.send(Event::Answer(msg))?;

    Ok(())
}

// #[cfg(target_os = "linux")]
// #[log()]
// pub async fn handle_media_play(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
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
// pub async fn handle_media_pause(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
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
pub async fn handle_media_play_pause(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
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
pub async fn handle_media_stop(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
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
pub async fn handle_media_next_track(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
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
pub async fn handle_media_previous_track(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
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

#[log(secs = %action.seconds)]
pub async fn handle_media_seek_forward(tx: Sender<Bytes>, action: SeekAction) -> Result<()> {
    match MediaControl::seek_forward(action.seconds).await {
        Ok(_) => {
            let msg = str!(
                "Media playback advanced by {} seconds successfully.",
                action.seconds
            );
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to seek forward: {e:?}").into()),
    }
}

#[log(secs = %action.seconds)]
pub async fn handle_media_seek_backward(tx: Sender<Bytes>, action: SeekAction) -> Result<()> {
    match MediaControl::seek_backward(action.seconds).await {
        Ok(_) => {
            let msg = str!(
                "Media playback rewound by {} seconds successfully.",
                action.seconds
            );
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to seek backward: {e:?}").into()),
    }
}

#[log()]
pub async fn handle_media_metadata(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
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
pub async fn handle_media_position(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
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
pub async fn handle_media_duration(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
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
