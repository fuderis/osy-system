use crate::prelude::*;

use anylm::api::{Schema, Tool};
use atoman::process::Command;
use pearce::stream::futures::FutureExt;
use std::path::Path;

// ============================================================================
// TOOLS DEFINITION
// ============================================================================

pub fn tools_list() -> Vec<Tool> {
    vec![
        // ________________________________________
        //               LIST DISKS
        Tool::new(
            "list",
            "Lists all available storage devices, block devices, and their partition tables.",
        ),
        // ________________________________________
        //               MOUNT DISK
        Tool::new(
            "mount",
            "Mounts a specific disk partition or block device to a target mount point.",
        )
        .required_property(
            "target",
            Schema::string("Path, name, UUID, or label of the block device or partition (e.g., 'sdb1', '/dev/sdb1', or 'DATA')."),
        )
        .optional_property(
            "point",
            Schema::string("Optional mount point directory. If omitted, a default path under /run/media/$USER/ will be used."),
        ),
        // ________________________________________
        //              UNMOUNT DISK
        Tool::new(
            "unmount",
            "Unmounts a mounted disk partition or device.",
        )
        .required_property(
            "target",
            Schema::string("Path, name, UUID, label, or mount point of the device to unmount."),
        ),
        // ________________________________________
        //               REPAIR DISK
        Tool::new(
            "repair",
            "Checks and attempts to repair file system errors on a partition.",
        )
        .required_property(
            "target",
            Schema::string("Path, name, UUID, or label of the block device/partition to check or repair."),
        ),
        // ________________________________________
        //               FORMAT DISK (DISABLED FOR NOW)
        Tool::new(
            "format",
            "Formats a disk partition or drive with the specified file system. WARNING: Deletes all data on target.",
        )
        .required_property(
            "target",
            Schema::string("Path, name, UUID, or label of the target partition/device (e.g., 'sdb1')."),
        )
        .required_property(
            "fs",
            Schema::string("Type of file system to apply.").variants(set![
                "ext4".into(),
                "btrfs".into(),
                "ntfs".into(),
                "vfat".into(),
                "exfat".into(),
            ]),
        )
        .optional_property(
            "label",
            Schema::string("Optional volume label/name for the formatted partition."),
        ),
    ]
}

// ============================================================================
// DATA STRUCTURES (lsblk)
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct Output {
    #[serde(rename = "blockdevices")]
    pub devices: Vec<Device>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Device {
    pub name: String,
    pub path: Option<String>,
    pub label: Option<String>,
    pub uuid: Option<String>,
    pub fstype: Option<String>,
    pub size: Option<String>,
    pub mountpoint: Option<String>,
    #[serde(default)]
    pub children: Vec<Device>,
    pub fsused: Option<String>,
    #[serde(rename = "fsavail")]
    pub fsavail: Option<String>,
    #[serde(rename = "fsuse%")]
    pub fsuse_percent: Option<String>,
}

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

// ============================================================================
// DTO STRUCTS
// ============================================================================

#[derive(Deserialize)]
pub struct MountAction {
    target: String,
    point: Option<String>,
}

#[derive(Deserialize)]
pub struct UnmountAction {
    target: String,
}

#[derive(Deserialize)]
pub struct RepairAction {
    target: String,
}

#[derive(Deserialize)]
pub struct FormatAction {
    target: String,
    fs: String,
    label: Option<String>,
}

// ============================================================================
// INTERNAL HELPERS
// ============================================================================

pub async fn list() -> Result<Vec<Device>> {
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("lsblk")
            .args(["--json", "-O"])
            .output()
            .await?;

        if !output.status.success() {
            return Err(str!("lsblk failed").into());
        }

        let output: Output = serde_json::from_slice(&output.stdout)?;
        Ok(output.devices)
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::UnsupportedOS.into())
    }
}

pub async fn find(name: &str) -> Result<Device> {
    let devices = list().await?;

    find_recursive(&devices, name)
        .cloned()
        .ok_or(str!("Device '{name}' not found").into())
}

fn find_recursive<'a>(devices: &'a [Device], target: &str) -> Option<&'a Device> {
    for device in devices {
        if device.path.as_deref() == Some(target)
            || device.label.as_deref() == Some(target)
            || device.uuid.as_deref() == Some(target)
            || device.mountpoint.as_deref() == Some(target)
            || device.name == target
        {
            return Some(device);
        }

        if let Some(found) = find_recursive(&device.children, target) {
            return Some(found);
        }
    }
    None
}

/// Checks if the device or any of its child partitions are root/system mounts (`/` or `/boot`).
fn is_system_device(dev: &Device) -> bool {
    if let Some(ref mp) = dev.mountpoint {
        if mp == "/" || mp.starts_with("/boot") {
            return true;
        }
    }

    for child in &dev.children {
        if is_system_device(child) {
            return true;
        }
    }

    false
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
    out.push_str(&format!(
        "{:<16} {:<14} {:<8} {:<8} {:<14} {:<16} {}\n",
        "NAME", "LABEL", "FS", "SIZE", "USED", "FREE", "MOUNT"
    ));
    out.push_str(&format!("{}\n", "─".repeat(110)));

    for (i, dev) in devices.iter().enumerate() {
        out.push_str(&format!("{}\n", dev.name));
        append_children(&mut out, &dev.children, "");

        if i + 1 != devices.len() {
            out.push('\n');
        }
    }

    out
}

fn append_children(out: &mut String, devices: &[Device], prefix: &str) {
    for (i, dev) in devices.iter().enumerate() {
        let last = i + 1 == devices.len();
        let d = display_device(dev);

        out.push_str(&format!(
            "{}{}{:<14} {:<14} {:<8} {:<8} {:<14} {:<16} {}\n",
            prefix,
            if last { "└ " } else { "├ " },
            d.name,
            d.label.unwrap_or("—"),
            d.fstype.unwrap_or("—"),
            d.size.unwrap_or("—"),
            d.used.as_deref().unwrap_or("—"),
            d.free.as_deref().unwrap_or("—"),
            d.mount.unwrap_or("—"),
        ));

        let next_prefix = if last {
            format!("{prefix}    ")
        } else {
            format!("{prefix}│   ")
        };

        append_children(out, d.children, &next_prefix);
    }
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
        .args(["mkdir", "-p", mount_path])
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
        .args(["timeout", "15", "mount", dev_path, mount_path])
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
        .args(["timeout", "10", "mount", "-o", "ro", dev_path, mount_path])
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
        .status()
        .await?;

    if status.success() {
        return Ok(());
    }

    install_package(repair.package).await?;

    let status = Command::new("sh")
        .args(["-c", &format!("command -v {}", repair.tool)])
        .status()
        .await?;

    if !status.success() {
        return Err(Error::Custom(str!("Failed to install '{}'.", repair.tool)).into());
    }

    Ok(())
}

#[cfg(target_os = "linux")]
async fn install_package(package: &str) -> Result<()> {
    let managers = [
        (
            "pacman",
            vec!["pacman", "-Sy", "--needed", "--noconfirm", package],
        ),
        ("apt", vec!["apt", "install", "-y", package]),
        ("dnf", vec!["dnf", "install", "-y", package]),
        (
            "zypper",
            vec!["zypper", "--non-interactive", "install", package],
        ),
    ];

    for (manager, args) in managers {
        let exists = Command::new("sh")
            .args(["-c", &format!("command -v {manager}")])
            .status()
            .await?;

        if !exists.success() {
            continue;
        }

        if manager == "apt" {
            let _ = Command::new("sudo")
                .args(["apt", "update"])
                .status()
                .await?;
        }

        let status = Command::new("sudo").args(args).status().await?;

        if status.success() {
            return Ok(());
        }

        return Err(Error::Custom(str!("Failed to install '{}'.", package)).into());
    }

    Err(Error::Custom(str!("Unsupported package manager.")).into())
}

#[cfg(target_os = "linux")]
async fn perform_unmount(target: &str) -> Result<String> {
    let dev = find(target).await?;

    if is_system_device(&dev) {
        return Err(Error::Custom(format!(
            "Access denied: '{target}' contains current OS system partitions."
        ))
        .into());
    }

    let mountpoint = match dev.mountpoint.clone() {
        Some(mp) => mp,
        None => {
            return Err(Error::Custom(format!("Device '{}' is not mounted.", target)).into());
        }
    };

    let dev_path = match dev.path.as_deref() {
        Some(path) => path,
        None => return Err(Error::Custom(str!("Device path is missing.")).into()),
    };

    let output = Command::new("sudo")
        .args(["umount", dev_path])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        if stderr.contains("not mounted") {
            return Err(Error::Custom(format!("Device '{}' is not mounted.", target)).into());
        }

        return Err(
            Error::Custom(format!("Failed to unmount '{}': {}", target, stderr.trim())).into(),
        );
    }

    if mountpoint.starts_with("/run/media/") && Path::new(&mountpoint).exists() {
        let _ = Command::new("sudo")
            .args(["rmdir", &mountpoint])
            .status()
            .await;
    }

    Ok(format!(
        "Successfully unmounted '{dev_path}' from '{mountpoint}'."
    ))
}

#[cfg(target_os = "linux")]
async fn perform_repair(target: &str) -> Result<String> {
    let dev = find(target).await?;

    if is_system_device(&dev) {
        return Err(Error::Custom(format!(
            "Access denied: Cannot run repair on system device '{target}'."
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
                Error::Custom(str!("Automatic repair for '{}' is not supported.", fs)).into(),
            );
        }
        None => {
            return Err(Error::Custom(str!("Could not detect filesystem type.")).into());
        }
    };

    ensure_tool(&repair).await?;

    let mut cmd = Command::new("sudo");
    match repair.tool {
        "ntfsfix" => {
            cmd.args(["ntfsfix", "-b", "-d", &dev_path]);
        }
        "e2fsck" => {
            cmd.args(["e2fsck", "-p", &dev_path]);
        }
        "fsck.exfat" => {
            cmd.args(["fsck.exfat", &dev_path]);
        }
        "btrfs" => {
            cmd.args(["btrfs", "check", "--repair", &dev_path]);
        }
        "fsck.f2fs" => {
            cmd.args(["fsck.f2fs", "-a", &dev_path]);
        }
        _ => {
            return Err(
                Error::Custom(str!("Unsupported repair utility '{}'.", repair.tool)).into(),
            );
        }
    }

    let status = cmd.status().await?;
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
            Ok(_) => mount_msg = format!(" and remounted at '{target_mount}'"),
            Err(_) => mount_msg = format!(", but failed to remount at '{target_mount}'"),
        }
    }

    Ok(format!(
        "Filesystem on '{dev_path}' successfully repaired{mount_msg}."
    ))
}

// ============================================================================
// HANDLERS
// ============================================================================

#[log()]
pub async fn handle_list(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
    match list().await {
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
pub async fn handle_mount(tx: Sender<Bytes>, action: MountAction) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        osy_share::ensure_sudo_priv!()?;

        let dev = find(&action.target).await?;

        if is_system_device(&dev) {
            return Err(Error::Custom(format!(
                "Access denied: '{target}' is part of the system drive.",
                target = action.target
            ))
            .into());
        }

        let dev_path = match dev.path.as_deref() {
            Some(path) => path,
            None => return Err(Error::Custom(str!("Device path is missing.")).into()),
        };

        if let Some(mount) = dev.mountpoint.as_deref() {
            let msg = format!("Device '{dev_path}' is already mounted at '{mount}'.");
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            return Ok(());
        }

        let mount_path = build_mount_path(&dev, action.point.as_deref());

        // 1. RW attempt
        if try_mount_rw(dev_path, &mount_path).await.is_ok() {
            let msg = format!("Mounted '{dev_path}' at '{mount_path}'.");
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            return Ok(());
        }

        // 2. Automatic repair
        let _ = perform_repair(&action.target).await;

        // 3. Retry RW
        if try_mount_rw(dev_path, &mount_path).await.is_ok() {
            let msg = format!("Mounted '{dev_path}' at '{mount_path}' after repair.");
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            return Ok(());
        }

        // 4. Fallback RO
        if try_mount_ro(dev_path, &mount_path).await.is_ok() {
            let msg = format!("Mounted '{dev_path}' read-only at '{mount_path}'.");
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
        osy_share::ensure_sudo_priv!()?;

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
        osy_share::ensure_sudo_priv!()?;

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
        osy_share::ensure_sudo_priv!()?;

        // check device before requesting confirmation
        let dev = find(&action.target).await?;

        if is_system_device(&dev) {
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

        let mut args = vec![tool];
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

        let status = Command::new("sudo").args(&args).status().await?;
        if !status.success() {
            return Err(
                Error::Custom(format!("Failed to format '{dev_path}' as {}", action.fs)).into(),
            );
        }

        let msg = format!("Successfully formatted '{dev_path}' as {}.", action.fs);
        info!("{msg}");
        tx.send(Event::Answer(msg))?;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::UnsupportedOS.into())
    }
}
