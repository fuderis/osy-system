use crate::prelude::*;

use anylm::api::{Schema, Tool};
use music_index::{MusicIndexer, SearchIntent};
use system_utils::{AudioControl, MediaControl};

static MUSIC_INDEX: State<Option<MusicIndexer>> = State::default();

pub fn tools_list() -> Vec<Tool> {
    vec![
        // ________________________________________
        // MEDIA CONTROl
        //

        #[cfg(target_os = "linux")]
        Tool::new("media_play", "Starts media playback."),
        #[cfg(target_os = "linux")]
        Tool::new("media_pause", "Pauses media playback."),
        Tool::new("media_play_pause", "Toggles between play and pause."),
        Tool::new("media_stop", "Stops media playback."),
        Tool::new("media_next_track", "Skips to the next track."),
        Tool::new("media_previous_track", "Returns to the previous track."),
        #[cfg(target_os = "linux")]
        Tool::new(
            "media_seek_forward",
            "Seeks forward by the specified number of seconds.",
        )
        .required_property(
            "seconds",
            Schema::integer("Number of seconds to seek forward."),
        ),
        #[cfg(target_os = "linux")]
        Tool::new(
            "media_seek_backward",
            "Seeks backward by the specified number of seconds.",
        )
        .required_property(
            "seconds",
            Schema::integer("Number of seconds to seek backward."),
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

        // ________________________________________
        // AUDIO CONTROL
        // 

        Tool::new(
            "set_volume",
            "Sets the system audio volume to the specified percentage.",
        )
        .required_property(
            "volume",
            Schema::integer("Target audio volume percentage (0-100)."),
        ),
        Tool::new(
            "increase_volume",
            "Increases the system audio volume by the specified percentage.",
        )
        .required_property(
            "amount",
            Schema::integer("Amount to increase the audio volume by."),
        ),
        Tool::new(
            "decrease_volume",
            "Decreases the system audio volume by the specified percentage.",
        )
        .required_property(
            "amount",
            Schema::integer("Amount to decrease the audio volume by."),
        ),
        Tool::new(
            "get_volume",
            "Returns the current system audio volume percentage (0-100).",
        ),

        Tool::new(
            "is_muted",
            "Checks if the system audio is currently muted. Returns a boolean representation.",
        ),

        Tool::new(
            "set_mute",
            "Mutes or unmutes the system audio based on the provided boolean flag.",
        )
        .required_property(
            "mute",
            Schema::boolean("True to mute the audio, false to unmute it."),
        ),

        // ________________________________________
        // MUSIC INDEX
        //

        Tool::new(
            "search_music",
            "Searches the local music library without starting playback. (If you need to play the music immediately, it’s better to use the play_music tool).",
        )
        .optional_property("band", Schema::string("Artist or band name."))
        .optional_property("album", Schema::string("Album title."))
        .optional_property("track", Schema::string("Track title."))
        .optional_property("genre", Schema::string("Music genre.")),

        Tool::new(
            "play_music",
            "Searches the local music library and immediately starts playback.",
        )
        .optional_property("band", Schema::string("Artist or band name."))
        .optional_property("album", Schema::string("Album title."))
        .optional_property("track", Schema::string("Track title."))
        .optional_property("genre", Schema::string("Music genre.")),
    ]
}

#[derive(Deserialize)]
pub struct SeekAction {
    seconds: u32,
}

#[cfg(target_os = "linux")]
#[log()]
pub async fn handle_media_play(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
    match MediaControl::play().await {
        Ok(_) => {
            let msg = "Media playback started successfully.";
            info!("{msg}");
            tx.send(Event::Answer(msg.into()))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to start media playback: {e:?}").into()),
    }
}

#[cfg(target_os = "linux")]
#[log()]
pub async fn handle_media_pause(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
    match MediaControl::pause().await {
        Ok(_) => {
            let msg = "Media playback paused successfully.";
            info!("{msg}");
            tx.send(Event::Answer(msg.into()))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to pause media playback: {e:?}").into()),
    }
}

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

#[derive(Deserialize)]
pub struct SetVolumeAction {
    volume: u32,
}

#[derive(Deserialize)]
pub struct DeltaVolumeAction {
    amount: u32,
}

#[log(volume = %action.volume)]
pub async fn handle_set_volume(tx: Sender<Bytes>, action: SetVolumeAction) -> Result<()> {
    match AudioControl::set_volume(action.volume as u32).await {
        Ok(_) => {
            let msg = str!(
                "Audio volume updated successfully. Current volume: {}%.",
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
pub async fn handle_increase_volume(tx: Sender<Bytes>, action: DeltaVolumeAction) -> Result<()> {
    match AudioControl::increase_volume(action.amount).await {
        Ok(volume) => {
            let msg =
                format!("The audio volume increased successfully. Current volume: {volume}%.",);
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to update the audio volume: {e:?}").into()),
    }
}

#[log(amount = %action.amount)]
pub async fn handle_decrease_volume(tx: Sender<Bytes>, action: DeltaVolumeAction) -> Result<()> {
    match AudioControl::decrease_volume(action.amount).await {
        Ok(volume) => {
            let msg =
                format!("The audio volume decreased successfully. Current volume: {volume}%.",);
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to update the audio volume: {e:?}").into()),
    }
}

#[log()]
pub async fn handle_get_volume(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
    match AudioControl::get_volume().await {
        Ok(volume) => {
            let msg = format!("The current audio volume level is {volume}%.");
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to get the audio volume: {e:?}").into()),
    }
}

#[derive(Deserialize)]
pub struct MuteAction {
    mute: bool,
}

#[log(mute = %action.mute)]
pub async fn handle_set_mute(tx: Sender<Bytes>, action: MuteAction) -> Result<()> {
    match AudioControl::set_mute(action.mute).await {
        Ok(_) => {
            let msg = if action.mute {
                "The audio muted successfully."
            } else {
                "The audio unmuted successfully."
            };
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

#[derive(Deserialize)]
pub struct MusicAction {
    pub query: Option<String>,
    pub band: Option<String>,
    pub album: Option<String>,
    pub track: Option<String>,
    pub genre: Option<String>,
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

#[log(query = %action.query.clone().unwrap_or_default())]
pub async fn handle_search_music(tx: Sender<Bytes>, mut action: MusicAction) -> Result<()> {
    let music_index = music_index().await?;

    let intent = if let Some(query) = action.query {
        SearchIntent::Global(query)
    } else if action.band.is_none()
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
    };

    let target = music_index.search(intent);
    let tracks = target.tracks();

    let msg = if tracks.is_empty() {
        format!("No matching music was found.")
    } else {
        // Берем первые 10-15 треков для контекста LLM
        let limit = 15;
        let preview: Vec<_> = tracks
            .iter()
            .take(limit)
            .map(|t| format!("- {} — {} ({})", t.band, t.name, t.path.display()))
            .collect();

        let extra_count = tracks.len().saturating_sub(limit);
        let extra_msg = if extra_count > 0 {
            format!("\n...and {} more tracks.", extra_count)
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

#[log(query = %action.query.clone().unwrap_or_default())]
pub async fn handle_play_music(tx: Sender<Bytes>, mut action: MusicAction) -> Result<()> {
    let music_index = music_index().await?;

    let intent = if let Some(query) = action.query {
        SearchIntent::Global(query)
    } else if action.band.is_none()
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
    };

    let target = music_index.search(intent);
    let tracks = target.tracks();

    if tracks.is_empty() {
        let msg = format!("No matching music was found.");
        info!("{msg}");
        tx.send(Event::Answer(msg))?;
        return Ok(());
    }

    music_index
        .play(target, path!("/tmp/osy/playlist.m3u"))
        .await?;

    let msg = str!(
        "Started playback of {count} track(s).",
        count = tracks.len()
    );

    info!("{msg}");
    tx.send(Event::Answer(msg))?;

    Ok(())
}
