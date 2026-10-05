use crate::{
    prelude::*,
    utils::{self, Device},
};

use anylm::{Schema, api::Tool};
use atoman::process::Command;
use pearce::stream::futures::FutureExt;
use std::process::Stdio;

pub fn tools_list() -> Vec<Tool> {
    vec![
        Tool::new(
            "list",
            "Lists all available storage devices, block devices, and their partition tables.",
        ),
        Tool::typed::<InfoAction>(
            "info",
            "Displays detailed information, parameters, and partition structure for a specific storage device or partition.",
        ),
        Tool::typed::<MountAction>(
            "mount",
            "Mounts a specific disk partition or block device to a target mount point.",
        ),
        Tool::typed::<UnmountAction>("unmount", "Unmounts a mounted disk partition or device."),
        Tool::typed::<RepairAction>(
            "repair",
            "Checks and attempts to repair file system errors on a partition.",
        ),
        Tool::typed::<FormatAction>(
            "format",
            "Formats a disk partition or drive with the specified file system. WARNING: Deletes all data on target.",
        ),
    ]
}

// ============================================================================
// DTO STRUCTS
// ============================================================================

#[derive(Debug, Deserialize, Schema)]
pub struct InfoAction {
    /// Path, name, UUID, label, or mount point of the target device (e.g., 'sdb1', '/dev/sdb1', or 'DATA').
    pub target: String,
}

#[derive(Debug, Deserialize, Schema)]
pub struct MountAction {
    /// Path, name, UUID, or label of the block device or partition (e.g., 'sdb1', '/dev/sdb1', or 'DATA').
    pub target: String,
    /// Optional mount point directory. If omitted, a default path under /run/media/$USER/ will be used.
    pub point: Option<String>,
}

#[derive(Debug, Deserialize, Schema)]
pub struct UnmountAction {
    /// Path, name, UUID, label, or mount point of the device to unmount.
    pub target: String,
}

#[derive(Debug, Deserialize, Schema)]
pub struct RepairAction {
    /// Path, name, UUID, or label of the block device/partition to check or repair.
    pub target: String,
}

#[derive(Debug, Deserialize, Schema)]
pub struct FormatAction {
    /// Path, name, UUID, or label of the target partition/device (e.g., 'sdb1').
    pub target: String,
    /// Type of file system to apply.
    #[schema(variants = ["ext4", "btrfs", "ntfs", "vfat", "exfat"])]
    pub fs: String,
    /// Optional volume label/name for the formatted partition.
    pub label: Option<String>,
}

// ============================================================================
// DISPLAY STRUCTURES & HELPERS
// ============================================================================

struct DisplayDevice<'a> {
    name: &'a str,
    label: Option<&'a str>,
    fstype: Option<&'a str>,
    size: Option<&'a str>,
    used: Option<String>,
    free: Option<String>,
    mount: Option<&'a str>,
    children: &'a [Device],
}

struct ToolPkg {
    tool: &'static str,
    package: &'static str,
}

fn display_device(dev: &Device) -> DisplayDevice<'_> {
    fn map_fstype(fstype: Option<&str>) -> Option<&str> {
        match fstype {
            Some("crypto_LUKS") => Some("luks"),
            other => other,
        }
    }

    if dev.fstype.as_deref() == Some("crypto_LUKS") && dev.children.len() == 1 {
        let child = &dev.children[0];

        return DisplayDevice {
            name: &dev.name,
            label: child.label.as_deref(),
            fstype: map_fstype(child.fstype.as_deref()),
            size: child.size.as_deref(),
            used: used(child),
            free: free(child),
            mount: child.mountpoint.as_deref(),
            children: &[],
        };
    }

    DisplayDevice {
        name: &dev.name,
        label: dev.label.as_deref(),
        fstype: map_fstype(dev.fstype.as_deref()),
        size: dev.size.as_deref(),
        used: used(dev),
        free: free(dev),
        mount: dev.mountpoint.as_deref(),
        children: &dev.children,
    }
}

fn used(dev: &Device) -> Option<String> {
    match (&dev.fsused, &dev.fsuse_percent) {
        (Some(used), Some(percent)) => Some(format!("{used} ({percent})")),
        (Some(used), None) => Some(used.clone()),
        _ => None,
    }
}

fn free(dev: &Device) -> Option<String> {
    match (&dev.fsavail, &dev.fsuse_percent) {
        (Some(free), Some(percent)) => {
            let p = percent.trim_end_matches('%');

            if let Ok(v) = p.parse::<u8>() {
                Some(format!("{free} ({}%)", 100 - v))
            } else {
                Some(free.clone())
            }
        }
        (Some(free), None) => Some(free.clone()),
        _ => None,
    }
}

fn format_disk_tree(devices: &[Device]) -> String {
    let mut out = String::new();
    out.push_str("| Name | Label | FS | Size | Used | Free | Mount Point |\n");
    out.push_str("|---|---|---|---|---|---|---|\n");

    let len = devices.len();
    for (i, dev) in devices.iter().enumerate() {
        let is_last = i + 1 == len;
        append_device_rows(&mut out, dev, 0, is_last, "");
    }

    out
}

fn append_device_rows(out: &mut String, dev: &Device, depth: usize, is_last: bool, prefix: &str) {
    let d = display_device(dev);

    let branch = if depth > 0 {
        if is_last { "└ " } else { "├ " }
    } else {
        ""
    };

    let name_field = format!("{}{}{}", prefix, branch, d.name);
    let is_parent = depth == 0;

    out.push_str(&format!(
        "| {} | {} | {} | {} | {} | {} | {} |\n",
        if is_parent {
            format!("**`{name_field}`**")
        } else {
            name_field
        },
        if !is_parent {
            d.label.unwrap_or("—")
        } else {
            ""
        },
        if !is_parent {
            d.fstype.map(|s| format!("`{s}`")).unwrap_or("—".into())
        } else {
            "".into()
        },
        if !is_parent {
            d.size.unwrap_or("—")
        } else {
            ""
        },
        if !is_parent {
            d.used.as_deref().unwrap_or("—")
        } else {
            ""
        },
        if !is_parent {
            d.free.as_deref().unwrap_or("—")
        } else {
            ""
        },
        if !is_parent {
            d.mount.map(|s| format!("`{s}`")).unwrap_or("—".into())
        } else {
            "".into()
        },
    ));

    let next_prefix = if depth > 0 {
        if is_last {
            format!("{}    ", prefix)
        } else {
            format!("{}│   ", prefix)
        }
    } else {
        "".to_string()
    };

    let children = d.children;
    let children_len = children.len();
    for (i, child) in children.iter().enumerate() {
        let child_is_last = i + 1 == children_len;
        append_device_rows(out, child, depth + 1, child_is_last, &next_prefix);
    }
}

fn format_device_info(dev: &Device) -> String {
    let mut out = String::new();

    let name = &dev.name;
    let path = dev.path.as_deref().unwrap_or("—");
    let label = dev.label.as_deref().unwrap_or("—");
    let uuid = dev.uuid.as_deref().unwrap_or("—");
    let fstype = dev.fstype.as_deref().unwrap_or("—");
    let size = dev.size.as_deref().unwrap_or("—");
    let mountpoint = dev.mountpoint.as_deref().unwrap_or("—");
    let is_sys = if utils::is_system_disk(dev) {
        "**Yes** (Protected)"
    } else {
        "No"
    };

    let used_val = dev.fsused.as_deref().unwrap_or("—");
    let usage_pct = dev.fsuse_percent.as_deref().unwrap_or("");
    let used_str = if usage_pct.is_empty() {
        used_val.to_string()
    } else {
        format!("{used_val} ({usage_pct})")
    };
    let avail_str = dev.fsavail.as_deref().unwrap_or("—");

    out.push_str("| Property | Value |\n");
    out.push_str("|---|---|\n");
    out.push_str(&format!("| **Device Name** | `{name}` |\n"));
    out.push_str(&format!("| **Device Path** | `{path}` |\n"));
    out.push_str(&format!("| **Label** | **{label}** |\n"));
    out.push_str(&format!("| **UUID** | `{uuid}` |\n"));
    out.push_str(&format!("| **Filesystem** | `{fstype}` |\n"));
    out.push_str(&format!("| **Total Size** | {size} |\n"));
    out.push_str(&format!("| **Used Space** | {used_str} |\n"));
    out.push_str(&format!("| **Available Space** | {avail_str} |\n"));
    out.push_str(&format!("| **Mount Point** | `{mountpoint}` |\n"));
    out.push_str(&format!("| **System Partition** | {is_sys} |\n"));

    if !dev.children.is_empty() {
        out.push_str("\n#### Partitions / Sub-devices\n\n");
        out.push_str("| Name | Label | FS | Size | Mount Point |\n");
        out.push_str("|---|---|---|---|---|\n");
        for child in &dev.children {
            out.push_str(&format!(
                "| `{}` | {} | `{}` | {} | `{}` |\n",
                child.name,
                child.label.as_deref().unwrap_or("—"),
                child.fstype.as_deref().unwrap_or("—"),
                child.size.as_deref().unwrap_or("—"),
                child.mountpoint.as_deref().unwrap_or("—")
            ));
        }
    }

    out
}

fn build_mount_path(dev: &Device, custom_point: Option<&str>) -> String {
    if let Some(path) = custom_point {
        return path.to_string();
    }

    let mount_name = dev
        .label
        .as_deref()
        .filter(|s: &&str| !s.is_empty())
        .unwrap_or(&dev.name);

    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    format!("/run/media/{user}/{mount_name}")
}

#[cfg(target_os = "linux")]
async fn ensure_mount_dir(mount_path: &str) -> Result<()> {
    let status = Command::new("sudo")
        .args(["-n", "mkdir", "-p", mount_path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .status()
        .await?;

    if !status.success() {
        return Err(Error::Custom(str!("Failed to create mount directory.")).into());
    }

    Ok(())
}

#[cfg(target_os = "linux")]
async fn try_mount_rw(dev_path: &str, mount_path: &str) -> Result<()> {
    ensure_mount_dir(mount_path).await?;

    let status = Command::new("sudo")
        .args(["-n", "mount", dev_path, mount_path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .status()
        .await?;

    if status.success() {
        return Ok(());
    }

    Err(Error::Custom(str!("Read-write mount failed.")).into())
}

#[cfg(target_os = "linux")]
async fn try_mount_ro(dev_path: &str, mount_path: &str) -> Result<()> {
    ensure_mount_dir(mount_path).await?;

    let status = Command::new("sudo")
        .args(["-n", "mount", "-o", "ro", dev_path, mount_path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .status()
        .await?;

    if status.success() {
        return Ok(());
    }

    Err(Error::Custom(str!("Read-only mount failed.")).into())
}

#[cfg(target_os = "linux")]
async fn ensure_tool(repair: &ToolPkg) -> Result<()> {
    let status = Command::new("sh")
        .args(["-c", &format!("command -v {}", repair.tool)])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .status()
        .await?;

    if status.success() {
        return Ok(());
    }

    utils::install_package(repair.package).await?;

    let status = Command::new("sh")
        .args(["-c", &format!("command -v {}", repair.tool)])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .status()
        .await?;

    if !status.success() {
        return Err(Error::Custom(str!("Failed to install `{}`.", repair.tool)).into());
    }

    Ok(())
}

#[cfg(target_os = "linux")]
async fn perform_unmount(target: &str) -> Result<String> {
    let dev = utils::find_disk(target).await?;

    if utils::is_system_disk(&dev) {
        return Err(Error::Custom(format!(
            "Access denied: `{target}` contains current OS system partitions."
        ))
        .into());
    }

    let mountpoint = match dev.mountpoint.clone() {
        Some(mp) => mp,
        None => {
            return Err(Error::Custom(format!("Device `{}` is not mounted.", target)).into());
        }
    };

    let dev_path = match dev.path.as_deref() {
        Some(path) => path,
        None => return Err(Error::Custom(str!("Device path is missing.")).into()),
    };

    let output = Command::new("sudo")
        .args(["-n", "umount", dev_path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        if stderr.contains("not mounted") {
            return Err(Error::Custom(format!("Device `{}` is not mounted.", target)).into());
        }

        return Err(
            Error::Custom(format!("Failed to unmount `{}`: {}", target, stderr.trim())).into(),
        );
    }

    if mountpoint.starts_with("/run/media/") && Path::new(&mountpoint).exists() {
        let _ = Command::new("sudo")
            .args(["-n", "rmdir", &mountpoint])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .status()
            .await;
    }

    Ok(format!(
        "Successfully unmounted `{dev_path}` from `{mountpoint}`."
    ))
}

#[cfg(target_os = "linux")]
async fn perform_repair(target: &str) -> Result<String> {
    let dev = utils::find_disk(target).await?;

    if utils::is_system_disk(&dev) {
        return Err(Error::Custom(format!(
            "Access denied: Cannot run repair on system device `{target}`."
        ))
        .into());
    }

    let dev_path = match dev.path.as_deref() {
        Some(path) => path.to_string(),
        None => return Err(Error::Custom(str!("Device path is missing.")).into()),
    };

    // remember if disk was mounted and where
    let original_mountpoint = dev.mountpoint.clone();

    // unmount if it was mounted
    if original_mountpoint.is_some() {
        perform_unmount(target).await?;
    }

    let repair = match dev.fstype.as_deref() {
        Some("ntfs") => ToolPkg {
            tool: "ntfsfix",
            package: "ntfsprogs",
        },
        Some("ext4") => ToolPkg {
            tool: "e2fsck",
            package: "e2fsprogs",
        },
        Some("exfat") => ToolPkg {
            tool: "fsck.exfat",
            package: "exfatprogs",
        },
        Some("btrfs") => ToolPkg {
            tool: "btrfs",
            package: "btrfs-progs",
        },
        Some("f2fs") => ToolPkg {
            tool: "fsck.f2fs",
            package: "f2fs-tools",
        },
        Some(fs) => {
            return Err(
                Error::Custom(str!("Automatic repair for `{}` is not supported.", fs)).into(),
            );
        }
        None => {
            return Err(Error::Custom(str!("Could not detect filesystem type.")).into());
        }
    };

    ensure_tool(&repair).await?;

    let mut cmd = Command::new("sudo");
    cmd.args(["-n", repair.tool]);
    match repair.tool {
        "ntfsfix" => {
            cmd.args(["-b", "-d", &dev_path]);
        }
        "e2fsck" => {
            cmd.args(["-p", &dev_path]);
        }
        "fsck.exfat" => {
            cmd.args([&dev_path]);
        }
        "btrfs" => {
            cmd.args(["check", "--repair", &dev_path]);
        }
        "fsck.f2fs" => {
            cmd.args(["-a", &dev_path]);
        }
        _ => {
            return Err(
                Error::Custom(str!("Unsupported repair utility '{}'.", repair.tool)).into(),
            );
        }
    }

    let status = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .status()
        .await?;
    let mut code = status.code().unwrap_or(1);

    if repair.tool == "e2fsck" && code == 1 {
        code = 0;
    }

    if code != 0 {
        return Err(Error::Custom(str!("Repair utility exited with code {}.", code)).into());
    }

    // if disk was mounted before the repair, mount it back.
    let mut mount_msg = String::new();
    if let Some(ref target_mount) = original_mountpoint {
        let remount_result = try_mount_rw(&dev_path, target_mount).await.or_else(|_| {
            try_mount_ro(&dev_path, target_mount)
                .now_or_never()
                .unwrap_or(Err(Error::Custom(str!("Mount failed")).into()))
        });

        match remount_result {
            Ok(_) => mount_msg = format!(" and remounted at `{target_mount}`"),
            Err(_) => mount_msg = format!(", but failed to remount at `{target_mount}`"),
        }
    }

    Ok(format!(
        "Filesystem on `{dev_path}` successfully repaired{mount_msg}."
    ))
}

// ============================================================================
// HANDLERS
// ============================================================================

#[log()]
pub async fn handle_list(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
    match utils::list_disks().await {
        Ok(devices) => {
            let msg = format_disk_tree(&devices);
            info!("Disk list fetched successfully");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

#[log(target = %action.target)]
pub async fn handle_info(tx: Sender<Bytes>, action: InfoAction) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let dev = utils::find_disk(&action.target).await?;
        let msg = format_device_info(&dev);
        info!("Disk info fetched for target {}", action.target);
        tx.send(Event::Answer(msg))?;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::UnsupportedOS.into())
    }
}

#[log(target = %action.target)]
pub async fn handle_mount(tx: Sender<Bytes>, action: MountAction) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let dev = utils::find_disk(&action.target).await?;

        if utils::is_system_disk(&dev) {
            return Err(Error::Custom(format!(
                "Access denied: `{target}` is part of the system drive.",
                target = action.target
            ))
            .into());
        }

        let dev_path = match dev.path.as_deref() {
            Some(path) => path,
            None => return Err(Error::Custom(str!("Device path is missing.")).into()),
        };

        if let Some(mount) = dev.mountpoint.as_deref() {
            let msg = format!("Device `{dev_path}` is already mounted at `{mount}`.");
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            return Ok(());
        }

        let mount_path = build_mount_path(&dev, action.point.as_deref());

        // 1. RW attempt
        if try_mount_rw(dev_path, &mount_path).await.is_ok() {
            let msg = format!("Mounted `{dev_path}` at `{mount_path}`.");
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            return Ok(());
        }

        // 2. Automatic repair
        let _ = perform_repair(&action.target).await;

        // 3. Retry RW
        if try_mount_rw(dev_path, &mount_path).await.is_ok() {
            let msg = format!("Mounted `{dev_path}` at `{mount_path}` after repair.");
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            return Ok(());
        }

        // 4. Fallback RO
        if try_mount_ro(dev_path, &mount_path).await.is_ok() {
            let msg = format!("Mounted `{dev_path}` read-only at `{mount_path}`.");
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            return Ok(());
        }

        Err(Error::Custom(str!("Failed to mount device after repair attempts.")).into())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::UnsupportedOS.into())
    }
}

#[log(target = %action.target)]
pub async fn handle_unmount(tx: Sender<Bytes>, action: UnmountAction) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let msg = perform_unmount(&action.target).await?;
        info!("{msg}");
        tx.send(Event::Answer(msg))?;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::UnsupportedOS.into())
    }
}

#[log(target = %action.target)]
pub async fn handle_repair(tx: Sender<Bytes>, action: RepairAction) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let msg = perform_repair(&action.target).await?;
        info!("{msg}");
        tx.send(Event::Answer(msg))?;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::UnsupportedOS.into())
    }
}

#[log(fs = %action.fs)]
pub async fn handle_format(tx: Sender<Bytes>, action: FormatAction) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        // check device before requesting confirmation
        let dev = utils::find_disk(&action.target).await?;

        if utils::is_system_disk(&dev) {
            return Err(Error::Custom(format!(
                "Cannot format system drive '{target}'!",
                target = action.target
            ))
            .into());
        }

        let dev_path = match dev.path.as_deref() {
            Some(path) => path,
            None => return Err(Error::Custom(str!("Device path is missing.")).into()),
        };

        // creating confirmation dialog
        let d_event_id = Id::new().to_string();
        let d_event = DialogEvent::Confirm {
            id: d_event_id.clone(),
            prompt: format!(
                "Are you sure to format device `{dev_path}` as `{fs}`? **ALL DATA WILL BE LOST!**",
                fs = action.fs,
            ),
            default: Some(Confirmation::No),
        };

        // register callback and send Event::Dialog
        let mut callback = Callback::register(&d_event_id).await;
        tx.send(Event::Dialog(d_event))?;

        let confirmation = atoman::select! {
            _ = tx.closed() => return Err(Error::ConnectionClosed.into()),
            res = callback.recv::<Confirmation>(Duration::from_secs(120)) => res?,
        };

        match confirmation {
            None | Some(Confirmation::No) => {
                let msg = "Format operation was cancelled by the user.".to_string();
                tx.send(Event::Answer(msg))?;
                return Ok(());
            }
            _ => {} // User has confirmed — continue the operation
        }

        // performing unmounting and formatting
        if dev.mountpoint.is_some() {
            let _ = perform_unmount(&action.target).await;
        }

        let (tool, pkg) = match action.fs.as_str() {
            "ext4" => ("mkfs.ext4", "e2fsprogs"),
            "btrfs" => ("mkfs.btrfs", "btrfs-progs"),
            "ntfs" => ("mkfs.ntfs", "ntfsprogs"),
            "vfat" => ("mkfs.vfat", "dosfstools"),
            "exfat" => ("mkfs.exfat", "exfatprogs"),
            fs => return Err(Error::Custom(format!("Unsupported filesystem format: {fs}")).into()),
        };

        ensure_tool(&ToolPkg { tool, package: pkg }).await?;

        let mut args = vec!["-n", tool];
        if let Some(ref label) = action.label {
            match action.fs.as_str() {
                "ext4" | "btrfs" | "ntfs" | "vfat" | "exfat" => {
                    args.push("-L");
                    args.push(label.as_str());
                }
                _ => {}
            }
        }
        args.push(dev_path);

        let status = Command::new("sudo")
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .status()
            .await?;

        if !status.success() {
            return Err(
                Error::Custom(format!("Failed to format `{dev_path}` as {}", action.fs)).into(),
            );
        }

        let msg = format!("Successfully formatted `{dev_path}` as {}.", action.fs);
        info!("{msg}");
        tx.send(Event::Answer(msg))?;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::UnsupportedOS.into())
    }
}
