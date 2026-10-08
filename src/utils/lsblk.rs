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

pub fn format_disk_tree(devices: &[Device]) -> String {
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

pub fn format_device_info(dev: &Device) -> String {
    let mut out = String::new();

    let name = &dev.name;
    let path = dev.path.as_deref().unwrap_or("—");
    let label = dev.label.as_deref().unwrap_or("—");
    let uuid = dev.uuid.as_deref().unwrap_or("—");
    let fstype = dev.fstype.as_deref().unwrap_or("—");
    let size = dev.size.as_deref().unwrap_or("—");
    let mountpoint = dev.mountpoint.as_deref().unwrap_or("—");
    let is_sys = if super::is_system_disk(dev) {
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
