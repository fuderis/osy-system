use crate::prelude::*;

use atoman::process::Command;
use std::process::Stdio;

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

pub async fn list_disks() -> Result<Vec<Device>> {
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("lsblk")
            .args(["--json", "-O"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
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

pub async fn find_disk(name: &str) -> Result<Device> {
    let devices = list_disks().await?;

    find_disk_recursive(&devices, name)
        .cloned()
        .ok_or(str!("Device '{name}' not found").into())
}

pub fn find_disk_recursive<'a>(devices: &'a [Device], target: &str) -> Option<&'a Device> {
    for device in devices {
        if device.path.as_deref() == Some(target)
            || device.label.as_deref() == Some(target)
            || device.uuid.as_deref() == Some(target)
            || device.mountpoint.as_deref() == Some(target)
            || device.name == target
        {
            return Some(device);
        }

        if let Some(found) = find_disk_recursive(&device.children, target) {
            return Some(found);
        }
    }
    None
}

/// Checks if the device or any of its child partitions are root/system mounts (`/` or `/boot`).
pub fn is_system_disk(dev: &Device) -> bool {
    if let Some(ref mp) = dev.mountpoint {
        if mp == "/" || mp.starts_with("/boot") {
            return true;
        }
    }

    for child in &dev.children {
        if is_system_disk(child) {
            return true;
        }
    }

    false
}
