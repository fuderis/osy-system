pub mod disk;
pub mod info;
pub mod infra;
pub mod media;
pub mod music;
pub mod power;
pub mod theme;
pub mod tunnel;

use crate::prelude::*;
use osy_skill::AgentSkill;

/// System agent skills.
#[derive(AgentSkill, Clone, Copy, Debug, Display, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
#[display(rename_all = "snake_case")]
pub enum SkillKind {
    #[skill(
        module = "info",
        description = "Hardware information, live system metrics and connected devices."
    )]
    #[tool(
        name = "system",
        description = "Returns static system information including operating system, CPU, GPU, RAM, motherboard, storage devices, and other hardware details.",
        no_action
    )]
    #[tool(
        name = "metrics",
        description = "Returns current live system metrics including CPU usage, memory usage, temperatures, disk usage, network activity and other runtime statistics.",
        no_action
    )]
    #[tool(
        name = "devices",
        description = "Returns a formatted list of currently connected hardware devices.",
        no_action
    )]
    Info,

    #[skill(
        module = "media",
        description = "Media control (volume, play/pause, stop, next/prev track, playback)."
    )]
    #[tool(
        name = "get",
        description = "Returns the current system audio volume percentage (0-100).",
        no_action
    )]
    #[tool(
        name = "set",
        description = "Sets the system audio volume to the specified percentage."
    )]
    #[tool(
        name = "volume",
        description = "Adjusts audio volume up (positive amount) or down (negative amount)."
    )]
    #[tool(
        name = "is_muted",
        description = "Checks if the system audio is currently muted. Returns a boolean representation.",
        no_action
    )]
    #[tool(name = "mute", description = "Mutes the system audio.", no_action)]
    #[tool(name = "unmute", description = "Unmutes the system audio.", no_action)]
    #[tool(
        name = "play_pause",
        description = "Toggles between media play and pause.",
        no_action
    )]
    #[tool(name = "stop", description = "Stops media playback.", no_action)]
    #[tool(name = "next", description = "Skips to the next track.", no_action)]
    #[tool(
        name = "previous",
        description = "Returns to the previous track.",
        no_action
    )]
    #[cfg(target_os = "linux")]
    #[tool(
        name = "seek",
        description = "Seeks media playback forward (positive seconds) or backward (negative seconds)."
    )]
    #[cfg(target_os = "linux")]
    #[tool(
        name = "metadata",
        description = "Returns metadata for the currently playing media.",
        no_action
    )]
    #[cfg(target_os = "linux")]
    #[tool(
        name = "position",
        description = "Returns the current playback position.",
        no_action
    )]
    #[cfg(target_os = "linux")]
    #[tool(
        name = "duration",
        description = "Returns the duration of the current media.",
        no_action
    )]
    Media,

    #[skill(
        module = "music",
        description = "Local music library management (search/play)."
    )]
    #[tool(
        name = "search",
        description = "Searches the local music library without starting playback. (If you need to play the music immediately, it’s better to use the play_music tool)."
    )]
    #[tool(
        name = "play",
        description = "Searches the local music library and immediately starts playback.",
        action = "music::SearchAction"
    )]
    Music,

    #[skill(
        module = "power",
        description = "Power scheduling (shutdown, suspend, reboot, lock, cancel power action)."
    )]
    #[tool(
        name = "schedule",
        description = "Schedules or immediately executes a system power action."
    )]
    #[tool(
        name = "cancel",
        description = "Cancels the currently scheduled power action if one exists.",
        no_action
    )]
    #[tool(
        name = "status",
        description = "Returns the currently scheduled power action and its execution time, if any.",
        no_action
    )]
    Power,

    #[skill(
        module = "theme",
        description = "Desktop appearance and theme management (set, get)."
    )]
    #[tool(name = "get", description = "Gets current system theme.", no_action)]
    #[tool(name = "set", description = "Changes the system appearance theme.")]
    Theme,

    #[skill(
        module = "disk",
        description = "Disk management (list, mount, unmount, format, backup).",
        prompt = "You can safely run the necessary commands - the user will still receive a prompt for confirmation."
    )]
    #[tool(
        name = "list",
        description = "Lists all available storage devices, block devices, and their partition tables.",
        no_action
    )]
    #[tool(
        name = "info",
        description = "Displays detailed information, parameters, and partition structure for a specific storage device or partition."
    )]
    #[tool(
        name = "mount",
        description = "Mounts a specific disk partition or block device to a target mount point."
    )]
    #[tool(
        name = "umount",
        description = "Unmounts a mounted disk partition or device."
    )]
    #[tool(
        name = "repair",
        description = "Checks and attempts to repair file system errors on a partition."
    )]
    #[tool(
        name = "format",
        description = "Formats a disk partition or drive with the specified file system. WARNING: Deletes all data on target."
    )]
    #[tool(
        name = "backup",
        description = "Backs up the current working directory or a specified file/directory to a target storage device, excluding build artifacts (e.g. target, node_modules)."
    )]
    Disk,

    #[skill(
        module = "infra",
        description = "Remote VPS infrastructure management (diagnostics, users, rsync file transfer, config sync).",
        prompt = "You can safely run the necessary commands - the user will still receive a prompt for confirmation."
    )]
    #[tool(
        name = "connect",
        description = "Opens an interactive SSH session to a remote VPS in a new terminal window."
    )]
    #[tool(
        name = "info",
        description = "Fetches diagnostics, CPU/RAM usage, active services, and OS stats from a remote VPS."
    )]
    #[tool(
        name = "user",
        description = "Comprehensive user and SSH key lifecycle management on target Linux VPS."
    )]
    #[tool(
        name = "transfer",
        description = "Uploads or downloads files/directories over SSH."
    )]
    #[tool(
        name = "sync",
        description = "Synchronizes editor configurations (Helix, Neovim, Vim) between local machine and VPS."
    )]
    #[tool(
        name = "ping",
        description = "Measures round-trip latency to a target host using ICMP echo requests."
    )]
    #[tool(
        name = "trace",
        description = "Traces the layer-3 network path/hops to a remote host."
    )]
    #[tool(
        name = "route",
        description = "Performs continuous network route quality analysis using MTR."
    )]
    Infra,

    #[skill(
        module = "tunnel",
        description = "Manages persistent SOCKS5 SSH proxy tunnels.",
        prompt = "You can safely run the necessary commands - the user will still receive a prompt for confirmation."
    )]
    #[tool(
        name = "start",
        description = "Starts a persistent SOCKS5 SSH proxy tunnel on a local port.",
        action = "tunnel::TunnelAction"
    )]
    #[tool(
        name = "stop",
        description = "Stops an active SOCKS5 proxy tunnel on a specified local port.",
        action = "tunnel::TunnelAction"
    )]
    #[tool(
        name = "restart",
        description = "Restarts an active SOCKS5 proxy tunnel on a local port.",
        action = "tunnel::TunnelAction"
    )]
    #[tool(
        name = "status",
        description = "Checks the active status and availability of a SOCKS5 proxy tunnel on a local port.",
        action = "tunnel::TunnelAction"
    )]
    Tunnel,
}
