use crate::{
    prelude::*,
    utils::{self, SshConnection},
};

use anylm::{Schema, api::Tool};
use atoman::{
    Command, fs,
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use russh_keys::ssh_key::rand_core::OsRng;
use std::process::Stdio;

// Registry to keep track of active tunnel cancellation channels by local port
static ACTIVE_TUNNELS: SharedMap<u16, Sender<()>> = SharedMap::new();

pub fn tools_list() -> Vec<Tool> {
    vec![
        Tool::typed::<ConnectAction>(
            "connect",
            "Opens an interactive SSH session to a remote VPS in a new terminal window.",
        ),
        Tool::typed::<InfoAction>(
            "info",
            "Fetches diagnostics, CPU/RAM usage, active services, and OS stats from a remote VPS.",
        ),
        Tool::typed::<UserAction>(
            "user",
            "Comprehensive user and SSH key lifecycle management on target Linux VPS.",
        ),
        Tool::typed::<TransferAction>(
            "transfer",
            "Uploads or downloads files/directories over SSH.",
        ),
        Tool::typed::<SyncConfigAction>(
            "sync",
            "Synchronizes editor configurations (Helix, Neovim, Vim) between local machine and VPS.",
        ),
        Tool::typed::<PingAction>(
            "ping",
            "Measures round-trip latency to a target host using ICMP echo requests.",
        ),
        Tool::typed::<TraceAction>(
            "trace",
            "Traces the layer-3 network path/hops to a remote host.",
        ),
        Tool::typed::<RouteAction>(
            "route",
            "Performs continuous network route quality analysis using MTR.",
        ),
        Tool::typed::<TunnelAction>(
            "tunnel",
            "Manages persistent SOCKS5 SSH proxy tunnel using pure Rust async runtime.",
        ),
    ]
}

// ============================================================================
// DTO STRUCTS
// ============================================================================

#[derive(Debug, Deserialize, Schema)]
pub struct ConnectAction {
    /// Target VPS host (e.g. 'user@192.168.1.1'). Omit for default host.
    pub host: Option<String>,
    /// Path to private SSH key file. Defaults to ~/.ssh/id_ed25519.
    pub ssh_file: Option<String>,
}

#[derive(Debug, Deserialize, Schema)]
pub struct InfoAction {
    /// Target VPS IP/Host (e.g. 'user@192.168.1.1'). Omit for DEFAULT_VPS_HOST.
    pub host: Option<String>,
    /// Path to private SSH key file. Defaults to ~/.ssh/id_ed25519.
    pub ssh_file: Option<String>,
}

#[derive(Debug, Deserialize, Schema)]
pub struct UserAction {
    /// Action to perform.
    #[schema(variants = ["create", "remove", "list", "add_ssh_key", "add_ssh_key_from_file", "generate_ssh_key", "set_sudo"])]
    pub action: String,
    /// Target username on the VPS.
    pub username: Option<String>,
    /// Raw SSH public key content.
    pub pubkey: Option<String>,
    /// Path to local key file to upload.
    pub key_path: Option<String>,
    /// Grant (true) or revoke (false) sudo privileges.
    pub sudo: Option<bool>,
    /// Target VPS host. Omit for default.
    pub host: Option<String>,
    /// Path to private SSH key file. Defaults to ~/.ssh/id_ed25519.
    pub ssh_file: Option<String>,
}

#[derive(Debug, Deserialize, Schema)]
pub struct TransferAction {
    /// Direction of transfer.
    #[schema(variants = ["upload", "download"])]
    pub direction: String,
    /// Local file path.
    pub local_path: String,
    /// Remote file path.
    pub remote_path: String,
    /// Target VPS host. Omit for default.
    pub host: Option<String>,
    /// Path to private SSH key file. Defaults to ~/.ssh/id_ed25519.
    pub ssh_file: Option<String>,
}

#[derive(Debug, Deserialize, Schema)]
pub struct SyncConfigAction {
    /// Editor to sync.
    #[schema(variants = ["helix", "neovim", "vim"])]
    pub editor: String,
    /// Sync direction.
    #[schema(variants = ["push", "pull"])]
    pub direction: String,
    /// Target VPS host. Omit for default.
    pub host: Option<String>,
    /// Path to private SSH key file. Defaults to ~/.ssh/id_ed25519.
    pub ssh_file: Option<String>,
}

#[derive(Debug, Deserialize, Schema)]
pub struct PingAction {
    /// Target domain or IP address (e.g. 'example.com' or '8.8.8.8').
    pub target: Option<String>,
    /// Optional IP override if target is specified as domain name.
    pub ip: Option<String>,
    /// Number of ICMP echo requests to send (default: 4).
    pub count: Option<usize>,
}

#[derive(Debug, Deserialize, Schema)]
pub struct TraceAction {
    /// Target domain or IP address.
    pub target: Option<String>,
    /// Optional IP override.
    pub ip: Option<String>,
}

#[derive(Debug, Deserialize, Schema)]
pub struct RouteAction {
    /// Target domain or IP address.
    pub target: Option<String>,
    /// Optional IP override.
    pub ip: Option<String>,
    /// Number of MTR cycles (default: 10).
    pub count: Option<usize>,
}

#[derive(Debug, Deserialize, Schema)]
pub struct TunnelAction {
    /// Tunnel lifecycle action.
    #[schema(variants = ["start", "stop", "status", "restart"])]
    action: String,
    /// Local port to bind (default: 1080).
    port: Option<u16>,
    /// Target VPS SSH host. Omit for default.
    host: Option<String>,
    /// Path to private SSH key file. Defaults to ~/.ssh/id_ed25519.
    ssh_file: Option<String>,
}

// ============================================================================
// HANDLERS
// ============================================================================

#[log()]
pub async fn handle_connect(tx: Sender<Bytes>, action: ConnectAction) -> Result<()> {
    let host = utils::resolve_host(action.host.as_deref())?;

    tx.send(Event::Thinking(format!(
        "Opening SSH session to `{host}` in a new terminal window..."
    )))?;

    let mut ssh_args = vec![host.clone()];
    if let Some(ref identity) = action.ssh_file {
        let path = utils::expand_home(identity);
        ssh_args.push("-i".to_string());
        ssh_args.push(path.to_string_lossy().to_string());
    }

    let ssh_cmd = format!("ssh {}", ssh_args.join(" "));
    let bash_cmd = format!("{ssh_cmd}; exec bash");

    #[allow(unused)]
    let mut child_res = Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "No suitable terminal emulator found",
    ));

    #[cfg(target_os = "macos")]
    {
        child_res = Command::new("osascript")
            .arg("-e")
            .arg(format!(
                "tell application \"Terminal\" to do script \"{ssh_cmd}\""
            ))
            .spawn();
    }

    #[cfg(target_os = "linux")]
    {
        child_res = Command::new("kgx")
            .args(["-e", "bash", "-c", &bash_cmd])
            .spawn();

        if child_res.is_err() {
            child_res = Command::new("xdg-terminal-exec")
                .args(["bash", "-c", &bash_cmd])
                .spawn();
        }

        if child_res.is_err() {
            if let Ok(term) = std::env::var("TERMINAL") {
                child_res = Command::new(&term)
                    .args(["-e", "bash", "-c", &bash_cmd])
                    .spawn();
            }
        }

        if child_res.is_err() {
            let terms: &[(&str, Vec<&str>)] = &[
                ("x-terminal-emulator", vec!["-e", &ssh_cmd]),
                ("ptyxis", vec!["--", "bash", "-c", &bash_cmd]),
                ("gnome-terminal", vec!["--", "bash", "-c", &bash_cmd]),
                ("konsole", vec!["-e", "bash", "-c", &bash_cmd]),
                ("xfce4-terminal", vec!["-e", &ssh_cmd]),
                ("alacritty", vec!["-e", "bash", "-c", &bash_cmd]),
                ("kitty", vec!["bash", "-c", &bash_cmd]),
                ("xterm", vec!["-e", &ssh_cmd]),
            ];

            for (binary, args) in terms {
                let res = Command::new(binary).args(args).spawn();
                if res.is_ok() {
                    child_res = res;
                    break;
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        child_res = Command::new("cmd")
            .args(["/C", "start", "cmd", "/K", &ssh_cmd])
            .spawn();
    }

    match child_res {
        Ok(_) => {
            tx.send(Event::Answer(format!(
                "Opened new window with SSH connection to `{host}`."
            )))?;
            info!("Spawned new terminal SSH session for host '{host}'.");
            Ok(())
        }
        Err(e) => Err(Error::Custom(format!("Failed to launch new terminal process: {e}")).into()),
    }
}

#[log()]
pub async fn handle_info(tx: Sender<Bytes>, action: InfoAction) -> Result<()> {
    let host = utils::resolve_host(action.host.as_deref())?;

    tx.send(Event::Thinking(format!(
        "Connecting to `{host}` via SSH..."
    )))?;
    let mut conn = SshConnection::connect(&host, action.ssh_file.as_deref()).await?;

    tx.send(Event::Thinking(
        "Fetching OS info, uptime, and load average...".into(),
    ))?;
    let os = conn
        .exec("(lsb_release -d 2>/dev/null | cut -f2- || grep PRETTY_NAME /etc/os-release | cut -d= -f2 | tr -d '\"')")
        .await?;

    let uptime = conn
        .exec("uptime -p 2>/dev/null | sed 's/^up //' || echo unknown")
        .await?;

    let load_avg = conn
        .exec("uptime | awk -F'load average:' '{print $2}'")
        .await?;

    tx.send(Event::Thinking("Fetching memory and disk usage...".into()))?;
    let ram = conn
        .exec("free -h | awk '/^Mem:/ {printf \"%s / %s\", $3, $2}'")
        .await?;

    let ram_avail = conn.exec("free -h | awk '/^Mem:/ {print $7}'").await?;

    let swap = conn
        .exec("free -h | awk '/^Swap:/ {printf \"%s / %s\", $3, $2}'")
        .await?;

    let disk = conn
        .exec("df -h / | awk 'NR==2 {printf \"%s / %s (%s)\", $3, $2, $5}'")
        .await?;

    tx.send(Event::Thinking(
        "Checking failed systemd services...".into(),
    ))?;
    let failed_raw = conn
        .exec("systemctl --failed --plain --no-legend 2>/dev/null || true")
        .await?;

    let failed = if failed_raw.trim().is_empty() {
        "None".to_string()
    } else {
        failed_raw
            .lines()
            .map(|l| l.split_whitespace().next().unwrap_or(l))
            .collect::<Vec<_>>()
            .join(", ")
    };

    tx.send(Event::Thinking("Composing system info table...".into()))?;
    let table = format!(
        "## System Info for `{host}`\n\n\
         | Parameter | Value |\n\
         |:----------|:------|\n\
         | OS | {} |\n\
         | Uptime | {} |\n\
         | Load average | {} |\n\
         | RAM (used / total) | {} |\n\
         | RAM available | {} |\n\
         | Swap (used / total) | {} |\n\
         | Disk (used / total) | {} |\n\
         | Failed services | {} |\n",
        os.trim(),
        uptime.trim(),
        load_avg.trim(),
        ram.trim(),
        ram_avail.trim(),
        swap.trim(),
        disk.trim(),
        failed,
    );

    tx.send(Event::Answer(table))?;
    info!("Fetched system info for host '{host}'.");

    Ok(())
}

#[log()]
pub async fn handle_user(tx: Sender<Bytes>, action: UserAction) -> Result<()> {
    let host = utils::resolve_host(action.host.as_deref())?;
    let identity = action.ssh_file.as_deref();

    tx.send(Event::Thinking(format!(
        "Connecting to `{host}` via SSH for user management..."
    )))?;
    let mut conn = SshConnection::connect(&host, identity).await?;

    match action.action.as_str() {
        "list" => {
            tx.send(Event::Thinking(
                "Fetching user list from /etc/passwd...".into(),
            ))?;
            let cmd = "awk -F: '$3 >= 1000 && $3 < 60000 {print $1}' /etc/passwd";
            let output = conn.exec(cmd).await?;
            tx.send(Event::Answer(format!(
                "Users on `{host}`:\n{}",
                if output.trim().is_empty() {
                    "No regular users found.".to_string()
                } else {
                    output
                }
            )))?;
        }

        "create" => {
            let user = action
                .username
                .as_deref()
                .ok_or_else(|| Error::Custom("Missing 'username' parameter".into()))?;

            tx.send(Event::Thinking(format!(
                "Creating user `{user}` on `{host}`..."
            )))?;
            let mut cmd = format!("sudo useradd -m -s /bin/bash '{user}'");
            if action.sudo.unwrap_or(false) {
                cmd.push_str(&format!(" && sudo usermod -aG sudo '{user}'"));
            }
            conn.exec(&cmd).await?;
            tx.send(Event::Answer(format!(
                "User `{user}` created successfully on `{host}`."
            )))?;
        }

        "remove" => {
            let user = action
                .username
                .as_deref()
                .ok_or_else(|| Error::Custom("Missing 'username' parameter".into()))?;

            let d_event_id = Id::new().to_string();
            let d_event = DialogEvent::Confirm {
                id: d_event_id.clone(),
                prompt: format!("Are you sure you want to delete user `{user}` from `{host}`?"),
                default: Some(Confirmation::No),
            };

            let mut callback = Callback::register(&d_event_id).await;
            tx.send(Event::Dialog(d_event))?;

            let confirmation = atoman::select! {
                _ = tx.closed() => return Err(Error::ConnectionClosed.into()),
                res = callback.recv::<Confirmation>(Duration::from_secs(120)) => res?,
            };

            if matches!(confirmation, Some(Confirmation::Yes)) {
                tx.send(Event::Thinking(format!(
                    "Killing active sessions and removing user `{user}` from `{host}`..."
                )))?;
                let cleanup_cmd = format!(
                    "sudo pkill -u '{user}' || true; \
                     sudo systemctl stop user@{user}.service || true; \
                     sudo userdel -r -f '{user}'"
                );

                conn.exec(&cleanup_cmd).await?;
                tx.send(Event::Answer(format!(
                    "Active sessions killed and user `{user}` removed successfully from `{host}`."
                )))?;
            } else {
                tx.send(Event::Answer("User removal cancelled.".to_string()))?;
            }
        }

        "add_ssh_key" => {
            let user = action
                .username
                .as_deref()
                .ok_or_else(|| Error::Custom("Missing `username` parameter".into()))?;
            let pubkey = action
                .pubkey
                .ok_or_else(|| Error::Custom("Missing pubkey parameter".into()))?;

            tx.send(Event::Thinking(format!(
                "Uploading SSH public key for user `{user}` on `{host}`..."
            )))?;
            utils::upload_pubkey_to_vps(&mut conn, user, &pubkey).await?;
            tx.send(Event::Answer(format!("SSH key added for user `{user}`.")))?;
        }

        "add_ssh_key_from_file" => {
            let user = action
                .username
                .as_deref()
                .ok_or_else(|| Error::Custom("Missing 'username' parameter".into()))?;
            let raw_path = action
                .key_path
                .ok_or_else(|| Error::Custom("Missing key_path parameter".into()))?;
            let path = utils::expand_home(&raw_path);

            tx.send(Event::Thinking(format!(
                "Reading public key from `{}`...",
                path.display()
            )))?;
            let pubkey = fs::read_to_string(&path).await.map_err(|e| {
                Error::Custom(format!(
                    "Failed to read public key file `{}`: {e}",
                    path.display()
                ))
            })?;

            tx.send(Event::Thinking(format!(
                "Uploading SSH key from `{}` for user `{user}` on `{host}`...",
                path.display()
            )))?;
            utils::upload_pubkey_to_vps(&mut conn, user, &pubkey).await?;
            tx.send(Event::Answer(format!(
                "SSH key from `{}` uploaded for user `{user}`.",
                path.display()
            )))?;
        }

        "generate_ssh_key" => {
            let user = action
                .username
                .as_deref()
                .ok_or_else(|| Error::Custom("Missing 'username' parameter".into()))?;

            tx.send(Event::Thinking(
                "Generating ED25519 keypair via russh...".into(),
            ))?;
            let private_key =
                russh_keys::PrivateKey::random(&mut OsRng, russh_keys::Algorithm::Ed25519)
                    .map_err(|e| {
                        Error::Custom(format!("Failed to generate ED25519 keypair: {e}"))
                    })?;

            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".into());
            let ssh_dir = PathBuf::from(home).join(".ssh");
            fs::create_dir_all(&ssh_dir).await?;

            let nonce = utils::generate_nonce();
            let key_name = format!("{user}@{host}_{nonce}");
            let priv_key_path = ssh_dir.join(&key_name);
            let pub_key_path = ssh_dir.join(format!("{key_name}.pub"));

            tx.send(Event::Thinking(format!(
                "Saving keypair to `{}`...",
                ssh_dir.display()
            )))?;
            let mut priv_file = std::fs::File::create(&priv_key_path)
                .map_err(|e| Error::Custom(format!("Failed to create private key file: {e}")))?;
            russh_keys::encode_pkcs8_pem(&private_key, &mut priv_file)
                .map_err(|e| Error::Custom(format!("Failed to write private key to file: {e}")))?;

            let public_key = private_key.public_key();
            let pubkey_str = public_key
                .to_openssh()
                .map_err(|e| Error::Custom(format!("Failed to format public key: {e}")))?;

            fs::write(&pub_key_path, format!("{pubkey_str} {user}@{host}\n")).await?;

            tx.send(Event::Thinking(format!(
                "Authorizing public key for user `{user}` on `{host}`..."
            )))?;
            utils::upload_pubkey_to_vps(&mut conn, user, &pubkey_str).await?;

            tx.send(Event::Answer(format!(
                "Generated ED25519 keypair via russh:\n\
                 - Private key: {}\n\
                 - Public key: {}\n\
                 Successfully authorized key for user `{user}` on `{host}`.",
                priv_key_path.display(),
                pub_key_path.display()
            )))?;
        }

        "set_sudo" => {
            let user = action
                .username
                .as_deref()
                .ok_or_else(|| Error::Custom("Missing 'username' parameter".into()))?;
            let grant = action
                .sudo
                .ok_or_else(|| Error::Custom("Missing 'sudo' boolean parameter".into()))?;

            let action_str = if grant { "Granting" } else { "Revoking" };
            tx.send(Event::Thinking(format!(
                "{action_str} sudo privileges for user `{user}` on `{host}`..."
            )))?;

            let cmd = if grant {
                format!("sudo usermod -aG sudo '{user}'")
            } else {
                format!("sudo gpasswd -d '{user}' sudo")
            };

            conn.exec(&cmd).await?;
            let status_str = if grant { "granted to" } else { "revoked from" };
            let msg = format!("Sudo privileges {status_str} user `{user}`.");

            tx.send(Event::Answer(msg))?;
        }

        _ => return Err(Error::Custom("Invalid user action.".into()).into()),
    }

    info!(
        "Completed user action '{}' for host '{host}'.",
        action.action
    );
    Ok(())
}

#[log()]
pub async fn handle_transfer(tx: Sender<Bytes>, action: TransferAction) -> Result<()> {
    let host = utils::resolve_host(action.host.as_deref())?;

    tx.send(Event::Thinking(format!(
        "Connecting to `{host}` via SSH for file transfer..."
    )))?;
    let mut conn = SshConnection::connect(&host, action.ssh_file.as_deref()).await?;

    let local_path = utils::expand_home(&action.local_path);
    let remote_path = action.remote_path.as_str();

    match action.direction.as_str() {
        "upload" => {
            tx.send(Event::Thinking(format!(
                "Reading local file `{}`...",
                local_path.display()
            )))?;
            let content = fs::read(&local_path).await.map_err(|e| {
                Error::Custom(format!(
                    "Failed to read local file `{}`: {e}",
                    local_path.display()
                ))
            })?;

            tx.send(Event::Thinking(format!(
                "Encoding and uploading `{}` to `{remote_path}` on `{host}`...",
                local_path.display()
            )))?;
            let encoded = BASE64.encode(content);
            let cmd = format!(
                "mkdir -p $(dirname '{remote_path}') && echo '{encoded}' | base64 -d > '{remote_path}'"
            );
            conn.exec(&cmd).await?;

            tx.send(Event::Answer(format!(
                "Uploaded `{}` to `{remote_path}` on {host}",
                local_path.display()
            )))?;
        }

        "download" => {
            tx.send(Event::Thinking(format!(
                "Downloading `{remote_path}` from `{host}`..."
            )))?;
            let cmd = format!("base64 '{remote_path}'");
            let output = conn.exec(&cmd).await?;
            let clean_b64 = output.replace(['\r', '\n'], "");

            tx.send(Event::Thinking(format!(
                "Decoding and saving to `{}`...",
                local_path.display()
            )))?;
            let decoded = BASE64.decode(clean_b64).map_err(|e| {
                Error::Custom(format!(
                    "Failed to decode base64 file data from remote: {e}"
                ))
            })?;

            if let Some(parent) = local_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::write(&local_path, decoded).await?;

            tx.send(Event::Answer(format!(
                "Downloaded `{remote_path}` from {host} to `{}`",
                local_path.display()
            )))?;
        }
        _ => return Err(Error::Custom("Invalid direction. Use upload/download".into()).into()),
    }

    info!(
        "Completed file transfer ({}) for host '{host}'.",
        action.direction
    );
    Ok(())
}

#[log()]
pub async fn handle_sync(tx: Sender<Bytes>, action: SyncConfigAction) -> Result<()> {
    let host = utils::resolve_host(action.host.as_deref())?;

    tx.send(Event::Thinking(format!(
        "Connecting to `{host}` via SSH for config sync..."
    )))?;
    let mut conn = SshConnection::connect(&host, action.ssh_file.as_deref()).await?;

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());

    let (local_rel, remote_rel) = utils::get_editor_paths(&action.editor)?;
    let local_full = PathBuf::from(home).join(local_rel);

    match action.direction.as_str() {
        "push" => {
            tx.send(Event::Thinking(format!(
                "Reading local `{}` config from `{}`...",
                action.editor,
                local_full.display()
            )))?;
            if local_full.is_file() {
                let content = fs::read(&local_full).await?;
                let encoded = BASE64.encode(content);

                tx.send(Event::Thinking(format!(
                    "Pushing `{}` config to `{host}`...",
                    action.editor
                )))?;
                let cmd = format!(
                    "mkdir -p $(dirname '~/{remote_rel}') && echo '{encoded}' | base64 -d > '~/{remote_rel}'"
                );
                conn.exec(&cmd).await?;
            } else {
                return Err(Error::Custom(format!(
                    "Local config file or path `{}` not found.",
                    local_full.display()
                ))
                .into());
            }
        }

        "pull" => {
            tx.send(Event::Thinking(format!(
                "Pulling `{}` config from `{host}`...",
                action.editor,
            )))?;
            let cmd = format!("base64 '~/{remote_rel}'");
            let output = conn.exec(&cmd).await?;
            let clean_b64 = output.replace(['\r', '\n'], "");

            tx.send(Event::Thinking(format!(
                "Decoding and saving `{}` config to `{}`...",
                action.editor,
                local_full.display()
            )))?;
            let decoded = BASE64
                .decode(clean_b64)
                .map_err(|e| Error::Custom(format!("Failed to decode remote config file: {e}")))?;

            if let Some(parent) = local_full.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::write(&local_full, decoded).await?;
        }

        _ => return Err(Error::Custom("Invalid direction. Use push/pull".into()).into()),
    }

    let msg = format!(
        "Successfully synchronized `{}` config (`{}`) with `{host}`.",
        action.editor, action.direction
    );
    tx.send(Event::Answer(msg))?;

    info!(
        "Completed config sync for editor '{}' ({}) with host '{host}'.",
        action.editor, action.direction
    );
    Ok(())
}

#[log()]
pub async fn handle_ping(tx: Sender<Bytes>, action: PingAction) -> Result<()> {
    let target = utils::resolve_host_target(&action.target, &action.ip)?;
    let count = action.count.unwrap_or(4);

    tx.send(Event::Thinking(format!(
        "Executing ICMP ping to `{target}` ({count} packets)..."
    )))?;

    let mut child = Command::new("ping")
        .args(["-c", &count.to_string(), &target])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Custom(format!("Failed to execute ping command: {e}")))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Custom("Failed to capture stdout for ping".into()))?;

    let mut reader = BufReader::new(stdout).lines();
    let mut row_count = 0;
    let mut stats = Vec::new();
    let mut raw_lines = Vec::new();

    tx.send(Event::Answer(format!(
        "## ICMP Ping Results for `{target}`\n\n| Seq | TTL | Time | Status |\n|:---:|:---:|:----:|:------:|\n"
    )))?;

    while let Ok(Some(line)) = reader.next_line().await {
        let trimmed = line.trim();
        if trimmed.contains("bytes from") {
            let seq = trimmed
                .split("icmp_seq=")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or("-");
            let ttl = trimmed
                .split("ttl=")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or("-");
            let time = trimmed
                .split("time=")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or("-");

            row_count += 1;
            tx.send(Event::Answer(format!(
                "| `{seq}` | `{ttl}` | `{time} ms` | OK |\n"
            )))?;
        } else if trimmed.contains("packet loss")
            || trimmed.contains("rtt")
            || trimmed.contains("round-trip")
        {
            stats.push(trimmed.to_string());
        } else if !trimmed.is_empty() && !trimmed.starts_with("PING ") {
            raw_lines.push(trimmed.to_string());
        }
    }

    if row_count == 0 && !raw_lines.is_empty() {
        let raw_md = format!("\n```text\n{}\n```\n", raw_lines.join("\n"));
        tx.send(Event::Answer(raw_md))?;
    }

    if !stats.is_empty() {
        let mut stats_md = String::from("\n");
        for stat in stats {
            stats_md.push_str(&format!("`{stat}`\n"));
        }
        tx.send(Event::Answer(stats_md))?;
    }

    info!("Executed ping to '{target}'.");
    Ok(())
}

#[log()]
pub async fn handle_trace(tx: Sender<Bytes>, action: TraceAction) -> Result<()> {
    let target = utils::resolve_host_target(&action.target, &action.ip)?;

    tx.send(Event::Thinking(format!(
        "Checking for `traceroute` availability..."
    )))?;
    if Command::new("traceroute").arg("-V").output().await.is_err() {
        utils::install_dependency("traceroute", &tx).await?;
    }

    tx.send(Event::Thinking(format!(
        "Tracing layer-3 network route to `{target}`..."
    )))?;

    let mut child = Command::new("traceroute")
        .arg(&target)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Custom(format!("Failed to execute traceroute: {e}")))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Custom("Failed to capture stdout for traceroute".into()))?;

    let mut reader = BufReader::new(stdout).lines();
    let mut row_count = 0;
    let mut raw_lines = Vec::new();

    tx.send(Event::Answer(format!(
        "## Traceroute for `{target}`\n\n| Hop | Node & Latency | Status |\n|:---:|:---------------|:------:|\n"
    )))?;

    while let Ok(Some(line)) = reader.next_line().await {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("traceroute to") {
            continue;
        }

        let mut parts = trimmed.split_whitespace();
        if let Some(hop_str) = parts.next() {
            if hop_str.parse::<u32>().is_ok() {
                let rest: Vec<&str> = parts.collect();
                let row = if rest.iter().all(|&s| s == "*") {
                    format!("| `{hop_str}` | `* * *` | Timeout |\n")
                } else {
                    let details = rest.join(" ");
                    format!("| `{hop_str}` | `{details}` | OK |\n")
                };
                row_count += 1;
                tx.send(Event::Answer(row))?;
                continue;
            }
        }
        raw_lines.push(trimmed.to_string());
    }

    if row_count == 0 && !raw_lines.is_empty() {
        let raw_md = format!("\n```text\n{}\n```\n", raw_lines.join("\n"));
        tx.send(Event::Answer(raw_md))?;
    }

    info!("Executed traceroute to '{target}'.");
    Ok(())
}

#[log()]
pub async fn handle_route(tx: Sender<Bytes>, action: RouteAction) -> Result<()> {
    let target = utils::resolve_host_target(&action.target, &action.ip)?;
    let count = action.count.unwrap_or(10);

    tx.send(Event::Thinking(format!(
        "Checking for `mtr` availability..."
    )))?;
    if Command::new("mtr").arg("--version").output().await.is_err() {
        utils::install_dependency("mtr", &tx).await?;
    }

    tx.send(Event::Thinking(format!(
        "Running MTR network quality analysis for `{target}` ({count} cycles)..."
    )))?;

    let mut mtr_cmd = Command::new("mtr");

    #[cfg(target_os = "linux")]
    {
        mtr_cmd.args(["-rwzc", &count.to_string(), &target]);
    }

    #[cfg(target_os = "macos")]
    {
        mtr_cmd.args(["-rc", &count.to_string(), &target]);
    }

    let mut child = mtr_cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Custom(format!("Failed to execute mtr: {e}")))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Custom("Failed to capture stdout for mtr".into()))?;

    let mut reader = BufReader::new(stdout).lines();
    let mut row_count = 0;
    let mut raw_lines = Vec::new();

    tx.send(Event::Answer(format!(
        "## MTR Route Analysis for `{target}`\n\n| Host / Hop | Loss% | Sent | Last | Avg | Best | Worst | StDev |\n|:-----------|:-----:|:----:|:----:|:---:|:----:|:-----:|:-----:|\n"
    )))?;

    while let Ok(Some(line)) = reader.next_line().await {
        let trimmed = line.trim();
        if trimmed.contains("HOST:") || trimmed.starts_with("Start:") {
            continue;
        }

        if trimmed.contains("|--") || trimmed.contains("|  ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 8 {
                let host_name = parts[0..parts.len() - 7]
                    .join(" ")
                    .replace("|--", "")
                    .replace("|", "")
                    .trim()
                    .to_string();
                let loss = parts[parts.len() - 7];
                let snt = parts[parts.len() - 6];
                let last = parts[parts.len() - 5];
                let avg = parts[parts.len() - 4];
                let best = parts[parts.len() - 3];
                let wrst = parts[parts.len() - 2];
                let stdev = parts[parts.len() - 1];

                let row = format!(
                    "| `{host_name}` | `{loss}` | `{snt}` | `{last}` | `{avg}` | `{best}` | `{wrst}` | `{stdev}` |\n"
                );
                row_count += 1;
                tx.send(Event::Answer(row))?;
                continue;
            }
        }
        if !trimmed.is_empty() {
            raw_lines.push(trimmed.to_string());
        }
    }

    if row_count == 0 && !raw_lines.is_empty() {
        let raw_md = format!("\n```text\n{}\n```\n", raw_lines.join("\n"));
        tx.send(Event::Answer(raw_md))?;
    }

    info!("Executed MTR route analysis to '{target}'.");
    Ok(())
}

#[log()]
pub async fn handle_tunnel(tx: Sender<Bytes>, action: TunnelAction) -> Result<()> {
    let port = action.port.unwrap_or(1080);
    let vps = utils::resolve_host(action.host.as_deref())?;

    match action.action.as_str() {
        "start" => {
            let addr = format!("127.0.0.1:{port}");
            let listener = TcpListener::bind(&addr).await.map_err(|e| {
                Error::Custom(format!("Port {port} is already in use or bind failed: {e}"))
            })?;

            let conn = utils::SshConnection::connect(&vps, action.ssh_file.as_deref()).await?;
            let session = Arc::new(conn.session);

            let (stop_tx, mut stop_rx) = atoman::oneshot_channel::<()>();
            {
                if let Some(old_stop_tx) = ACTIVE_TUNNELS.insert(port, stop_tx).await {
                    old_stop_tx.write().await.send(()).ok();
                }
            }

            // spawn async SOCKS5 proxy server task with graceful cancellation support
            atoman::spawn(async move {
                loop {
                    atoman::select! {
                        _ = &mut stop_rx => {
                            info!("Shutdown signal received. Stopping SOCKS5 proxy on port `{port}`...");
                            break;
                        }
                        accept_res = listener.accept() => {
                            let (mut socket, _) = match accept_res {
                                Ok(res) => res,
                                Err(e) => {
                                    error!("Failed to accept TCP connection on port `{port}`: {e}");
                                    break;
                                }
                            };

                            let session_clone = Arc::clone(&session);
                            atoman::spawn(async move {
                                // SOCKS5 proxy handshaking
                                let mut buf = [0u8; 256];
                                if socket.read_exact(&mut buf[..2]).await.is_err() {
                                    return;
                                }
                                let nmethods = buf[1] as usize;
                                if socket.read_exact(&mut buf[..nmethods]).await.is_err() {
                                    return;
                                }
                                // NO AUTH response
                                if socket.write_all(&[0x05, 0x00]).await.is_err() {
                                    return;
                                }

                                // SOCKS Request
                                if socket.read_exact(&mut buf[..4]).await.is_err() {
                                    return;
                                }
                                if buf[1] != 0x01 {
                                    return; // support only CONNECT
                                }

                                let target_host = match buf[3] {
                                    0x01 => {
                                        // IPv4
                                        let mut ip = [0u8; 4];
                                        if socket.read_exact(&mut ip).await.is_err() {
                                            return;
                                        }
                                        std::net::Ipv4Addr::from(ip).to_string()
                                    }
                                    0x03 => {
                                        // Domain
                                        let mut len = [0u8; 1];
                                        if socket.read_exact(&mut len).await.is_err() {
                                            return;
                                        }
                                        let mut domain = vec![0u8; len[0] as usize];
                                        if socket.read_exact(&mut domain).await.is_err() {
                                            return;
                                        }
                                        String::from_utf8_lossy(&domain).to_string()
                                    }
                                    _ => return,
                                };

                                let mut port_buf = [0u8; 2];
                                if socket.read_exact(&mut port_buf).await.is_err() {
                                    return;
                                }
                                let target_port = u16::from_be_bytes(port_buf);

                                // open SSH Direct TCP/IP channel to target host
                                if let Ok(channel) = session_clone
                                    .channel_open_direct_tcpip(
                                        &target_host,
                                        target_port as u32,
                                        "127.0.0.1",
                                        0,
                                    )
                                    .await
                                {
                                    // send success response for SOCKS5
                                    let _ = socket
                                        .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                                        .await;

                                    let (mut reader, mut writer) = socket.split();
                                    let mut channel_stream = channel.into_stream();

                                    let _ = atoman::io::copy_bidirectional(
                                        &mut channel_stream,
                                        &mut atoman::io::join(&mut reader, &mut writer),
                                    )
                                    .await;
                                }
                            });
                        }
                    }
                }
                info!("SOCKS5 Proxy loop exited for port `{port}`.");
            });

            tx.send(Event::Answer(format!(
                "SOCKS5 Proxy listening locally on `127.0.0.1:{port}` via `{vps}`."
            )))?;
        }
        "stop" => {
            let msg = if let Some(stop_tx) = ACTIVE_TUNNELS.remove(&port).await {
                let _ = stop_tx.write().await.send(());
                info!("Signal sent to stop proxy listener on port `{port}`.");
                format!("Successfully stopped SOCKS5 proxy tunnel on port `{port}`.")
            } else {
                warn!("Stop requested, but no active tunnel found registered on port `{port}`");
                format!("No active proxy tunnel found running on port `{port}`.")
            };

            info!("{msg}");
            tx.send(Event::Answer(msg))?;
        }
        "status" => {
            let is_registered = ACTIVE_TUNNELS.get(&port).await.is_some();
            let addr = format!("127.0.0.1:{port}");
            let is_port_bound = TcpListener::bind(&addr).await.is_err();

            let msg = if is_registered || is_port_bound {
                format!(
                    "Tunnel status for port `{port}`: **ACTIVE** (Port occupied by active SOCKS5 worker)"
                )
            } else {
                format!("No active proxy tunnel found listening on port `{port}`.")
            };

            info!("{msg}");
            tx.send(Event::Answer(msg))?;
        }
        _ => return Err(Error::Custom("Invalid tunnel action".into()).into()),
    }

    Ok(())
}
