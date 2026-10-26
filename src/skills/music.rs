use crate::prelude::*;

use anylm::Schema;
use music_index::{MusicIndexer, SearchIntent};

static MUSIC_INDEX: State<Option<MusicIndexer>> = State::default();

#[derive(Debug, Deserialize, Schema)]
pub struct SearchAction {
    /// Artist or band name.
    pub band: Option<String>,
    /// Album title.
    pub album: Option<String>,
    /// Track title.
    pub track: Option<String>,
    /// Music genre.
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

impl From<SearchAction> for SearchIntent {
    fn from(mut action: SearchAction) -> Self {
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
pub async fn handle_search(tx: Sender<Bytes>, query: ToolQuery<SearchAction>) -> Result<()> {
    let ToolQuery { payload, .. } = query;

    let music_index = music_index().await?;

    let target = music_index.search(payload.into());
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
pub async fn handle_play(tx: Sender<Bytes>, query: ToolQuery<SearchAction>) -> Result<()> {
    let ToolQuery { payload, .. } = query;

    let music_index = music_index().await?;

    let target = music_index.search(payload.into());
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
