use crate::{
    prelude::*,
    utils::{self, ToolPkg},
};

use anylm::{Schema, api::Tool};
use atoman::process::Command;
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
        Tool::typed::<BackupAction>(
            "backup",
            "Backs up the current working directory or a specified file/directory to a target storage device, excluding build artifacts (e.g. target, node_modules).",
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

#[derive(Debug, Deserialize, Schema)]
pub struct BackupAction {
    /// Path to file or directory to back up. Defaults to current working directory if omitted.
    pub source: Option<String>,
    /// Target block device, partition, label, or mount point. Defaults to DEFAULT_BACKUP_DISK env variable if omitted.
    pub target: Option<String>,
}

// ============================================================================
// HANDLERS
// ============================================================================

#[log()]
pub async fn handle_list(tx: Sender<Bytes>, _query: ToolQuery<JsonValue>) -> Result<()> {
    match utils::list_disks().await {
        Ok(devices) => {
            let msg = utils::format_disk_tree(&devices);
            info!("Disk list fetched successfully");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

#[log(target = %query.payload.target)]
pub async fn handle_info(tx: Sender<Bytes>, query: ToolQuery<InfoAction>) -> Result<()> {
    let ToolQuery { payload, .. } = query;

    #[cfg(target_os = "linux")]
    {
        let dev = utils::find_disk(&payload.target).await?;
        let msg = utils::format_device_info(&dev);
        info!("Disk info fetched for target {}", payload.target);
        tx.send(Event::Answer(msg))?;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::UnsupportedOS.into())
    }
}

#[log(target = %query.payload.target)]
pub async fn handle_mount(tx: Sender<Bytes>, query: ToolQuery<MountAction>) -> Result<()> {
    let ToolQuery { payload, .. } = query;

    #[cfg(target_os = "linux")]
    {
        let dev = utils::find_disk(&payload.target).await?;

        if utils::is_system_disk(&dev) {
            return Err(Error::Custom(format!(
                "Access denied: `{target}` is part of the system drive.",
                target = payload.target
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

        let mount_path = utils::build_mount_path(&dev, payload.point.as_deref());
        let fstype = dev.fstype.as_deref();

        // 1. RW attempt
        if utils::try_mount_rw(dev_path, &mount_path, fstype)
            .await
            .is_ok()
        {
            let msg = format!("Mounted `{dev_path}` at `{mount_path}`.");
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            return Ok(());
        }

        // 2. Automatic repair
        let _ = utils::perform_repair(&payload.target).await;

        // 3. Retry RW
        if utils::try_mount_rw(dev_path, &mount_path, fstype)
            .await
            .is_ok()
        {
            let msg = format!("Mounted `{dev_path}` at `{mount_path}` after repair.");
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            return Ok(());
        }

        // 4. Fallback RO
        if utils::try_mount_ro(dev_path, &mount_path, fstype)
            .await
            .is_ok()
        {
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

#[log(target = %query.payload.target)]
pub async fn handle_unmount(tx: Sender<Bytes>, query: ToolQuery<UnmountAction>) -> Result<()> {
    let ToolQuery { payload, .. } = query;

    #[cfg(target_os = "linux")]
    {
        let msg = utils::perform_unmount(&payload.target).await?;
        info!("{msg}");
        tx.send(Event::Answer(msg))?;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::UnsupportedOS.into())
    }
}

#[log(target = %query.payload.target)]
pub async fn handle_repair(tx: Sender<Bytes>, query: ToolQuery<RepairAction>) -> Result<()> {
    let ToolQuery { payload, .. } = query;

    #[cfg(target_os = "linux")]
    {
        let msg = utils::perform_repair(&payload.target).await?;
        info!("{msg}");
        tx.send(Event::Answer(msg))?;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::UnsupportedOS.into())
    }
}

#[log(fs = %query.payload.fs)]
pub async fn handle_format(tx: Sender<Bytes>, query: ToolQuery<FormatAction>) -> Result<()> {
    let ToolQuery { payload, .. } = query;

    #[cfg(target_os = "linux")]
    {
        // check device before requesting confirmation
        let dev = utils::find_disk(&payload.target).await?;

        if utils::is_system_disk(&dev) {
            return Err(Error::Custom(format!(
                "Cannot format system drive '{target}'!",
                target = payload.target
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
                fs = payload.fs,
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
            let _ = utils::perform_unmount(&payload.target).await;
        }

        let (tool, pkg) = match payload.fs.as_str() {
            "ext4" => ("mkfs.ext4", "e2fsprogs"),
            "btrfs" => ("mkfs.btrfs", "btrfs-progs"),
            "ntfs" => ("mkfs.ntfs", "ntfsprogs"),
            "vfat" => ("mkfs.vfat", "dosfstools"),
            "exfat" => ("mkfs.exfat", "exfatprogs"),
            fs => return Err(Error::Custom(format!("Unsupported filesystem format: {fs}")).into()),
        };

        utils::ensure_tool(&ToolPkg { tool, package: pkg }).await?;

        let mut args = vec!["-n", tool];
        if let Some(ref label) = payload.label {
            match payload.fs.as_str() {
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
                Error::Custom(format!("Failed to format `{dev_path}` as {}", payload.fs)).into(),
            );
        }

        let msg = format!("Formatted `{dev_path}` as {}.", payload.fs);
        info!("{msg}");
        tx.send(Event::Answer(msg))?;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::UnsupportedOS.into())
    }
}

#[log(target = %query.payload.target.as_deref().unwrap_or("default"))]
pub async fn handle_backup(tx: Sender<Bytes>, query: ToolQuery<BackupAction>) -> Result<()> {
    let ToolQuery {
        current_path,
        payload,
    } = query;

    #[cfg(target_os = "linux")]
    {
        // resolve disk name / fallback env var
        let target_disk = payload
            .target
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var("DEFAULT_BACKUP_DISK").ok())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                Error::Custom(str!(
                    "Target disk not specified and DEFAULT_BACKUP_DISK environment variable is missing."
                ))
            })?;

        // locate disk device
        let dev = utils::find_disk(&target_disk).await?;
        if utils::is_system_disk(&dev) {
            return Err(Error::Custom(format!(
                "Access denied: `{target_disk}` is part of the system drive."
            ))
            .into());
        }

        let dev_path = match dev.path.as_deref() {
            Some(path) => path,
            None => return Err(Error::Custom(str!("Device path is missing.")).into()),
        };

        // ensure disk is mounted for read-write
        let mount_path_buf = if let Some(ref mp) = dev.mountpoint {
            PathBuf::from(mp)
        } else {
            let mount_str = utils::build_mount_path(&dev, None);
            utils::try_mount_rw(dev_path, &mount_str, dev.fstype.as_deref()).await?;
            PathBuf::from(mount_str)
        };

        // resolve source path (current dir or passed path)
        let source_str = payload.source.as_deref().filter(|s| !s.trim().is_empty());
        let src_path = match source_str {
            Some(p) => PathBuf::from(p),
            None => current_path.unwrap_or(std::env::current_dir()?),
        };

        let meta = atoman::fs::metadata(&src_path).await.map_err(|e| {
            Error::Custom(format!(
                "Failed to access source path `{}`: {e}",
                src_path.display()
            ))
        })?;

        let stem = src_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "backup".into());

        let ext_suffix = src_path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();

        let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();

        // parent folder in target disk root
        let root_folder = mount_path_buf.join(&stem);
        atoman::fs::create_dir_all(&root_folder).await?;

        let total_files: u64;
        let dest_display: String;

        if meta.is_dir() {
            let backup_dir_name = format!("{stem}__{timestamp}");
            let target_dir = root_folder.join(&backup_dir_name);
            dest_display = target_dir.display().to_string();

            total_files = utils::copy_directory_with_excludes(&src_path, &target_dir, &tx).await?;
        } else if meta.is_file() {
            let backup_file_name = format!("{stem}__{timestamp}{ext_suffix}");
            let target_file = root_folder.join(&backup_file_name);
            dest_display = target_file.display().to_string();

            let file_name = src_path
                .file_name()
                .map(|s| s.to_string_lossy())
                .unwrap_or_default();

            let _ = tx.send(Event::Thinking(format!("Backing up file: {file_name}")));
            atoman::fs::copy(&src_path, &target_file).await?;
            total_files = 1;
        } else {
            return Err(
                Error::Custom(str!("Source path is neither a file nor a directory.")).into(),
            );
        }

        let (uid, gid) = utils::resolve_target_uid_gid(&mount_path_buf.to_string_lossy());

        let _ = atoman::Command::new("sudo")
            .args([
                "-n",
                "chown",
                "-R",
                &format!("{uid}:{gid}"),
                &root_folder.to_string_lossy(),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        let msg = format!(
            "Backed up {total_files} file(s) from `{}` to `{dest_display}`.",
            src_path.display()
        );
        info!("{msg}");
        tx.send(Event::Answer(msg))?;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::UnsupportedOS.into())
    }
}
