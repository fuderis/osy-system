pub mod disk;
pub mod info;
pub mod infra;
pub mod media;
pub mod power;
pub mod theme;

use crate::prelude::*;

use anylm::api::Tool;
use osy_skill::AgentSkill;
use pearce::{Bytes, Sender};

/// System agent skills.
#[derive(AgentSkill, Clone, Copy, Debug, Display, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
#[display(rename_all = "snake_case")]
pub enum SkillKind {
    #[skill(
        module = "info",
        description = "Hardware information, live system metrics and connected devices."
    )]
    #[tools("system", "metrics", "devices")]
    Info,

    #[skill(
        module = "media",
        description = "Media control (volume, play/pause, stop, next/prev track, search/play music)."
    )]
    #[tools(
        "get_volume",
        "set_volume",
        "increase_volume",
        "decrease_volume",
        "is_muted",
        "set_mute",
        "media_play",
        "media_pause",
        "media_play_pause",
        "media_stop",
        "media_next_track",
        "media_previous_track",
        "media_seek_forward",
        "media_seek_backward",
        "media_metadata",
        "media_position",
        "media_duration",
        "search_music",
        "play_music"
    )]
    Media,

    #[skill(
        module = "power",
        description = "Power scheduling (shutdown, suspend, reboot, lock, cancel power action)."
    )]
    #[tools("schedule", "cancel", "status")]
    Power,

    #[skill(
        module = "theme",
        description = "Desktop appearance and theme management (set, get)."
    )]
    #[tools("set", "get")]
    Theme,

    #[skill(
        module = "disk",
        description = "Disk management (list, mount, unmount, format).",
        prompt = "You can safely run the necessary commands - the user will still receive a prompt for confirmation."
    )]
    #[tools("list", "mount", "unmount", "repair", "format")]
    Disk,

    #[skill(
        module = "infra",
        description = "Remote VPS infrastructure management (diagnostics, users, autossh tunnels, rsync file transfer, config sync).",
        prompt = "You can safely run the necessary commands - the user will still receive a prompt for confirmation."
    )]
    #[tools("info", "user", "tunnel", "transfer", "sync")]
    Infra,
}
