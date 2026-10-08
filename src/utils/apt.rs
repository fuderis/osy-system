use crate::prelude::*;

use atoman::process::Command;
use std::process::Stdio;

pub struct ToolPkg {
    pub tool: &'static str,
    pub package: &'static str,
}

#[cfg(target_os = "linux")]
pub async fn ensure_tool(repair: &ToolPkg) -> Result<()> {
    let status = Command::new("sh")
        .args(["-c", &format!("command -v {}", repair.tool)])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .status()
        .await?;

    if status.success() {
        return Ok(());
    }

    install_package(repair.package).await?;

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
pub async fn install_package(package: &str) -> Result<()> {
    let managers = [
        (
            "pacman",
            vec![
                "sudo",
                "-n",
                "pacman",
                "-Sy",
                "--needed",
                "--noconfirm",
                package,
            ],
        ),
        ("apt", vec!["sudo", "-n", "apt", "install", "-y", package]),
        ("dnf", vec!["sudo", "-n", "dnf", "install", "-y", package]),
        (
            "zypper",
            vec![
                "sudo",
                "-n",
                "zypper",
                "--non-interactive",
                "install",
                package,
            ],
        ),
    ];

    for (manager, args) in managers {
        let exists = Command::new("sh")
            .args(["-c", &format!("command -v {manager}")])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .status()
            .await?;

        if !exists.success() {
            continue;
        }

        if manager == "apt" {
            let _ = Command::new("sudo")
                .args(["-n", "apt", "update"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .status()
                .await?;
        }

        let status = Command::new(args[0])
            .args(&args[1..])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .status()
            .await?;

        if status.success() {
            return Ok(());
        }

        return Err(Error::Custom(str!("Failed to install `{}`.", package)).into());
    }

    Err(Error::Custom(str!("Unsupported package manager.")).into())
}

pub async fn install_dependency(package: &str, tx: &Sender<Bytes>) -> Result<()> {
    info!("Installing missing dependency: {package}");
    tx.send(Event::Answer(format!(
        "📦 Missing required package `{package}`. Attempting installation..."
    )))?;

    #[cfg(target_os = "linux")]
    {
        if Command::new("pacman")
            .arg("--version")
            .output()
            .await
            .is_ok()
        {
            let status = Command::new("sudo")
                .args(["pacman", "-S", "--noconfirm", package])
                .status()
                .await?;
            if !status.success() {
                return Err(
                    Error::Custom(format!("Failed to install `{package}` via pacman.")).into(),
                );
            }
        } else if Command::new("apt-get")
            .arg("--version")
            .output()
            .await
            .is_ok()
        {
            Command::new("sudo")
                .args(["apt-get", "update", "-y"])
                .status()
                .await?;
            let status = Command::new("sudo")
                .args(["apt-get", "install", "-y", package])
                .status()
                .await?;
            if !status.success() {
                return Err(
                    Error::Custom(format!("Failed to install `{package}` via apt.")).into(),
                );
            }
        } else {
            return Err(Error::Custom(format!(
                "Unsupported Linux package manager to install `{package}`."
            ))
            .into());
        }
    }

    #[cfg(target_os = "macos")]
    {
        if Command::new("brew")
            .arg("--version")
            .output()
            .await
            .is_err()
        {
            return Err(Error::Custom(
                "Homebrew is required to install missing dependencies on macOS.".into(),
            )
            .into());
        }

        let status = Command::new("brew")
            .args(["install", package])
            .status()
            .await?;
        if !status.success() {
            return Err(
                Error::Custom(format!("Failed to install `{package}` via Homebrew.")).into(),
            );
        }
    }

    tx.send(Event::Answer(format!(
        "✅ Dependency `{package}` installed successfully."
    )))?;

    Ok(())
}
