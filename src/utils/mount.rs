use super::{Device, ToolPkg};
use crate::prelude::*;

use atoman::Command;
use pearce::stream::futures::FutureExt;
use std::path::Path;
use std::process::Stdio;

/// Defines the target UID/GID of the user.
pub fn resolve_target_uid_gid(mount_path: &str) -> (u32, u32) {
    let env_sudo_uid = std::env::var("SUDO_UID").ok();
    let env_sudo_gid = std::env::var("SUDO_GID").ok();
    let env_sudo_user = std::env::var("SUDO_USER").ok();
    let env_user = std::env::var("USER").ok();

    // checking SUDO_UID / SUDO_GID
    if let (Some(uid_str), Some(gid_str)) = (&env_sudo_uid, &env_sudo_gid) {
        if let (Ok(uid), Ok(gid)) = (uid_str.parse::<u32>(), gid_str.parse::<u32>()) {
            if uid != 0 {
                return (uid, gid);
            }
        }
    }

    // extracting name from the path (/run/media/{user}/...)
    let path_parts: Vec<&str> = mount_path.split('/').collect();
    let path_user = if path_parts.len() >= 4 && path_parts[1] == "run" && path_parts[2] == "media" {
        Some(path_parts[3])
    } else {
        None
    };

    let target_username = path_user
        .map(|s| s.to_string())
        .or(env_sudo_user)
        .or(env_user)
        .filter(|u| u != "root");

    if let Some(ref user) = target_username {
        if let Ok(output) = std::process::Command::new("id").args(["-u", user]).output() {
            if let Ok(uid_str) = String::from_utf8(output.stdout) {
                if let Ok(uid) = uid_str.trim().parse::<u32>() {
                    let gid = std::process::Command::new("id")
                        .args(["-g", user])
                        .output()
                        .ok()
                        .and_then(|o| String::from_utf8(o.stdout).ok())
                        .and_then(|g| g.trim().parse::<u32>().ok())
                        .unwrap_or(uid);

                    return (uid, gid);
                }
            }
        }
    }

    // fallback to libc
    let process_uid = unsafe { libc::getuid() };
    let process_gid = unsafe { libc::getgid() };
    warn!("[UID_RESOLVE] Fallback to process libc -> uid: {process_uid}, gid: {process_gid}");

    (process_uid, process_gid)
}

pub fn build_mount_path(dev: &Device, custom_point: Option<&str>) -> String {
    if let Some(path) = custom_point {
        return path.to_string();
    }

    let mount_name = dev
        .label
        .as_deref()
        .filter(|s: &&str| !s.is_empty())
        .unwrap_or(&dev.name);

    let user = std::env::var("SUDO_USER")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "fuderis".into());

    format!("/run/media/{user}/{mount_name}")
}

#[cfg(target_os = "linux")]
async fn ensure_mount_dir(mount_path: &str) -> Result<()> {
    let (uid, gid) = resolve_target_uid_gid(mount_path);

    let status = Command::new("sudo")
        .args(["-n", "mkdir", "-p", mount_path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .status()
        .await?;

    if !status.success() {
        error!("[ENSURE_DIR] Failed to mkdir: {mount_path}");
        return Err(Error::Custom(str!("Failed to create mount directory.")).into());
    }

    // setting up rights to the parent directory
    if let Some(parent) = Path::new(mount_path).parent() {
        if let Some(parent_str) = parent.to_str() {
            let _ = Command::new("sudo")
                .args(["-n", "chown", &format!("{uid}:{gid}"), parent_str])
                .output()
                .await;

            let _ = Command::new("sudo")
                .args(["-n", "chmod", "755", parent_str])
                .output()
                .await;
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
pub async fn try_mount_rw(dev_path: &str, mount_path: &str, fstype: Option<&str>) -> Result<()> {
    ensure_mount_dir(mount_path).await?;

    let (uid, gid) = resolve_target_uid_gid(mount_path);
    let fs = fstype.unwrap_or("");
    let is_non_posix = matches!(fs, "ntfs" | "ntfs3" | "exfat" | "vfat" | "fat" | "msdos");

    let mut args = vec!["-n", "mount"];
    let options_str;

    if is_non_posix {
        options_str =
            format!("uid={uid},gid={gid},iocharset=utf8,umask=000,dmask=000,fmask=000,allow_other");
        args.extend(["-o", &options_str]);
    }

    args.push(dev_path);
    args.push(mount_path);

    let status = Command::new("sudo")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .status()
        .await?;

    if !status.success() {
        warn!(
            "[MOUNT_RW] Initial mount failed for {dev_path}. Attempting fallback mount without options..."
        );
        let fallback_output = Command::new("sudo")
            .args(["-n", "mount", dev_path, mount_path])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !fallback_output.status.success() {
            error!(
                "[MOUNT_RW] Fallback mount failed: {}",
                String::from_utf8_lossy(&fallback_output.stderr)
            );
            return Err(Error::Custom(str!("Read-write mount failed.")).into());
        }
    }

    // changing owner and rights of the mount point ONLY (without -R)
    let chown_out = Command::new("sudo")
        .args(["-n", "chown", &format!("{uid}:{gid}"), mount_path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !chown_out.status.success() {
        warn!(
            "[MOUNT_RW] Post-mount chown warning: {}",
            String::from_utf8_lossy(&chown_out.stderr)
        );
    }

    let chmod_out = Command::new("sudo")
        .args(["-n", "chmod", "777", mount_path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !chmod_out.status.success() {
        warn!(
            "[MOUNT_RW] Post-mount chmod warning: {}",
            String::from_utf8_lossy(&chmod_out.stderr)
        );
    }

    Ok(())
}

#[cfg(target_os = "linux")]
pub async fn try_mount_ro(dev_path: &str, mount_path: &str, fstype: Option<&str>) -> Result<()> {
    ensure_mount_dir(mount_path).await?;

    let (uid, gid) = resolve_target_uid_gid(mount_path);
    let is_non_posix = matches!(fstype, Some("ntfs" | "exfat" | "vfat" | "fat" | "msdos"));

    let mut args = vec!["-n", "mount"];
    let options_str;

    if is_non_posix {
        options_str = format!("ro,uid={uid},gid={gid},umask=022");
        args.extend(["-o", &options_str]);
    } else {
        args.extend(["-o", "ro"]);
    }

    args.push(dev_path);
    args.push(mount_path);

    let status = Command::new("sudo")
        .args(&args)
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
pub async fn perform_unmount(target: &str) -> Result<String> {
    let dev = super::find_disk(target).await?;

    if super::is_system_disk(&dev) {
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
pub async fn perform_repair(target: &str) -> Result<String> {
    let dev = super::find_disk(target).await?;

    if super::is_system_disk(&dev) {
        return Err(Error::Custom(format!(
            "Access denied: Cannot run repair on system device `{target}`."
        ))
        .into());
    }

    let dev_path = match dev.path.as_deref() {
        Some(path) => path.to_string(),
        None => return Err(Error::Custom(str!("Device path is missing.")).into()),
    };

    let original_mountpoint = dev.mountpoint.clone();

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

    super::ensure_tool(&repair).await?;

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

    let mut mount_msg = String::new();
    if let Some(ref target_mount) = original_mountpoint {
        let fstype = dev.fstype.as_deref();
        let remount_result = try_mount_rw(&dev_path, target_mount, fstype)
            .await
            .or_else(|_| {
                try_mount_ro(&dev_path, target_mount, fstype)
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
