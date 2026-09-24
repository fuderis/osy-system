use crate::prelude::*;

use anylm::api::{Schema, Tool};
use atoman::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use russh::{
    client::{self, Config, Handler},
    keys::{PrivateKeyWithHashAlg, PublicKeyOrCertificate},
};
use russh_keys::ssh_key::rand_core::OsRng;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

// Registry to keep track of active tunnel cancellation channels by local port
static ACTIVE_TUNNELS: LazyLock<Mutex<HashMap<u16, oneshot::Sender<()>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ============================================================================
// TOOLS DEFINITION
// ============================================================================

pub fn tools_list() -> Vec<Tool> {
    vec![
        // ________________________________________
        //                  INFRA INFO
        Tool::new(
            "info",
            "Fetches diagnostics, CPU/RAM usage, active services, and OS stats from a remote VPS.",
        )
        .optional_property(
            "host",
            Schema::string("Target VPS IP/Host (e.g. '192.168.1.1' or 'user@192.168.1.1'). Omit for DEFAULT_VPS_HOST."),
        )
        .optional_property(
            "identity_file",
            Schema::string("Path to private SSH key file. Defaults to ~/.ssh/id_ed25519."),
        ),
        // ________________________________________
        //                  USER MANAGEMENT
        Tool::new(
            "user",
            "Comprehensive user and SSH key lifecycle management on target Linux VPS.",
        )
        .required_property(
            "action",
            Schema::string("Action to perform.").variants(set![
                "create".into(),
                "remove".into(),
                "list".into(),
                "add_ssh_key".into(),
                "add_ssh_key_from_file".into(),
                "generate_ssh_key".into(),
                "set_sudo".into(),
            ]),
        )
        .optional_property("username", Schema::string("Target username on the VPS."))
        .optional_property("pubkey", Schema::string("Raw SSH public key content."))
        .optional_property("key_path", Schema::string("Path to local key file to upload."))
        .optional_property("sudo", Schema::boolean("Grant (true) or revoke (false) sudo privileges."))
        .optional_property("host", Schema::string("Target VPS host. Omit for default."))
        .optional_property("identity_file", Schema::string("Path to private SSH key file. Defaults to ~/.ssh/id_ed25519.")),
        // ________________________________________
        //                  AUTOSSH TUNNEL / SOCKS5
        Tool::new(
            "tunnel",
            "Manages persistent SOCKS5 SSH proxy tunnel using pure Rust async runtime.",
        )
        .required_property(
            "action",
            Schema::string("Tunnel lifecycle action.").variants(set![
                "start".into(),
                "stop".into(),
                "status".into(),
            ]),
        )
        .optional_property("local_port", Schema::integer("Local port to bind (default: 1080)."))
        .optional_property("vps_host", Schema::string("Target VPS SSH host. Omit for default."))
        .optional_property("identity_file", Schema::string("Path to private SSH key file. Defaults to ~/.ssh/id_ed25519.")),
        // ________________________________________
        //                  FILE TRANSFER
        Tool::new(
            "transfer",
            "Uploads or downloads files/directories over SSH.",
        )
        .required_property(
            "direction",
            Schema::string("Direction of transfer.").variants(set![
                "upload".into(),
                "download".into(),
            ]),
        )
        .required_property("local_path", Schema::string("Local file path."))
        .required_property("remote_path", Schema::string("Remote file path."))
        .optional_property("host", Schema::string("Target VPS host. Omit for default."))
        .optional_property("identity_file", Schema::string("Path to private SSH key file. Defaults to ~/.ssh/id_ed25519.")),
        // ________________________________________
        //                  CONFIG SYNC
        Tool::new(
            "sync",
            "Synchronizes editor configurations (Helix, Neovim, Vim) between local machine and VPS.",
        )
        .required_property(
            "editor",
            Schema::string("Editor to sync.").variants(set![
                "helix".into(),
                "neovim".into(),
                "vim".into(),
            ]),
        )
        .required_property(
            "direction",
            Schema::string("Sync direction.").variants(set![
                "push".into(), // local -> remote
                "pull".into(), // remote -> local
            ]),
        )
        .optional_property("host", Schema::string("Target VPS host. Omit for default."))
        .optional_property("identity_file", Schema::string("Path to private SSH key file. Defaults to ~/.ssh/id_ed25519.")),
    ]
}

// ============================================================================
// DTO STRUCTS
// ============================================================================

#[derive(Deserialize)]
pub struct InfraInfoAction {
    pub host: Option<String>,
    pub identity_file: Option<String>,
}

#[derive(Deserialize)]
pub struct InfraUserAction {
    pub action: String,
    pub username: Option<String>,
    pub pubkey: Option<String>,
    pub key_path: Option<String>,
    pub sudo: Option<bool>,
    pub host: Option<String>,
    pub identity_file: Option<String>,
}

#[derive(Deserialize)]
pub struct InfraTunnelAction {
    pub action: String,
    #[serde(default = "default_port")]
    pub local_port: u16,
    pub vps_host: Option<String>,
    pub identity_file: Option<String>,
}

fn default_port() -> u16 {
    1080
}

#[derive(Deserialize)]
pub struct InfraTransferAction {
    pub direction: String,
    pub local_path: String,
    pub remote_path: String,
    pub host: Option<String>,
    pub identity_file: Option<String>,
}

#[derive(Deserialize)]
pub struct InfraSyncConfigAction {
    pub editor: String,
    pub direction: String,
    pub host: Option<String>,
    pub identity_file: Option<String>,
}

// ============================================================================
// RUSSH CLIENT HANDLER & HELPERS
// ============================================================================

struct SshClientHandler;

impl Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKeyOrCertificate,
    ) -> StdResult<bool, Self::Error> {
        Ok(true)
    }
}

pub struct SshConnection {
    session: client::Handle<SshClientHandler>,
}

impl SshConnection {
    pub async fn connect(host_str: &str, identity_file: Option<&str>) -> Result<Self> {
        let (user, host, port) = parse_host_string(host_str)?;
        let key_path = resolve_identity_file(identity_file)?;

        let key_pair = russh::keys::load_secret_key(&key_path, None).map_err(|e| {
            Error::Custom(format!(
                "Failed to load SSH private key from {}: {e}",
                key_path.display()
            ))
        })?;

        let config = Arc::new(Config {
            inactivity_timeout: Some(Duration::from_secs(30)),
            ..Default::default()
        });

        let addr = format!("{host}:{port}");
        let mut session = client::connect(config, addr, SshClientHandler)
            .await
            .map_err(|e| Error::Custom(format!("SSH connection to {host}:{port} failed: {e}")))?;

        let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(key_pair), None);
        let auth_res = session.authenticate_publickey(user, key_with_alg).await?;

        if !auth_res.success() {
            return Err(
                Error::Custom("SSH authentication rejected by remote server".into()).into(),
            );
        }

        Ok(Self { session })
    }

    pub async fn exec(&mut self, command: &str) -> Result<String> {
        let mut channel = self
            .session
            .channel_open_session()
            .await
            .map_err(|e| Error::Custom(format!("Failed to open SSH channel: {e}")))?;

        channel
            .exec(true, command)
            .await
            .map_err(|e| Error::Custom(format!("Failed to exec command over SSH: {e}")))?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        while let Some(msg) = channel.wait().await {
            match msg {
                russh::ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                russh::ChannelMsg::ExtendedData { data, ext: 1 } => stderr.extend_from_slice(&data),
                _ => {}
            }
        }

        let stdout_str = String::from_utf8_lossy(&stdout).to_string();
        let stderr_str = String::from_utf8_lossy(&stderr).to_string();

        if !stderr_str.is_empty() && stdout_str.is_empty() {
            return Err(Error::Custom(format!("SSH Command failed: {}", stderr_str.trim())).into());
        }

        Ok(stdout_str)
    }
}

fn parse_host_string(raw: &str) -> Result<(String, String, u16)> {
    let mut user = "root".to_string();
    let mut host_port = raw.trim();

    if let Some((u, hp)) = host_port.split_once('@') {
        user = u.to_string();
        host_port = hp;
    }

    let (host, port) = if let Some((h, p)) = host_port.split_once(':') {
        let port_num = p
            .parse::<u16>()
            .map_err(|_| Error::Custom(format!("Invalid port in host string: {p}")))?;
        (h.to_string(), port_num)
    } else {
        (host_port.to_string(), 22)
    };

    if host.is_empty() {
        return Err(Error::Custom("Host address cannot be empty".into()).into());
    }

    Ok((user, host, port))
}

fn resolve_identity_file(override_path: Option<&str>) -> Result<PathBuf> {
    if let Some(p) = override_path {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            return Ok(expand_home(trimmed));
        }
    }

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| Error::Custom("Could not resolve home directory".into()))?;

    let ssh_dir = PathBuf::from(home).join(".ssh");
    let ed25519 = ssh_dir.join("id_ed25519");

    if ed25519.exists() {
        Ok(ed25519)
    } else {
        let rsa = ssh_dir.join("id_rsa");
        if rsa.exists() {
            Ok(rsa)
        } else {
            // Default to ed25519 to produce a clear error message on key load failure
            Ok(ed25519)
        }
    }
}

fn generate_nonce() -> u16 {
    rand::random::<u16>()
}

fn expand_home(path: &str) -> PathBuf {
    if path.starts_with('~') {
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            return PathBuf::from(path.replacen('~', &home, 1));
        }
    }
    PathBuf::from(path)
}

fn resolve_host(override_host: Option<&str>) -> Result<String> {
    if let Some(h) = override_host {
        let trimmed = h.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    std::env::var("DEFAULT_VPS_HOST").map_err(|_| {
        Error::Custom("Expected host name or DEFAULT_VPS_HOST env variable.".into()).into()
    })
}

fn get_editor_paths(editor: &str) -> Result<(&'static str, &'static str)> {
    match editor {
        "helix" => Ok((".config/helix", ".config/helix")),
        "neovim" => Ok((".config/nvim", ".config/nvim")),
        "vim" => Ok((".vimrc", ".vimrc")),
        _ => Err(Error::Custom("Unsupported editor".into()).into()),
    }
}

async fn upload_pubkey_to_vps(conn: &mut SshConnection, user: &str, pubkey: &str) -> Result<()> {
    let clean_pubkey = pubkey.trim();
    let target_dir = if user == "root" {
        "/root/.ssh".to_string()
    } else {
        format!("/home/{user}/.ssh")
    };

    let cmd = format!(
        "sudo mkdir -p {target_dir} && \
         echo '{clean_pubkey}' | sudo tee -a {target_dir}/authorized_keys > /dev/null && \
         (id -u {user} >/dev/null 2>&1 && sudo chown -R {user}:{user} {target_dir} || true) && \
         sudo chmod 700 {target_dir} && \
         sudo chmod 600 {target_dir}/authorized_keys"
    );
    conn.exec(&cmd).await?;
    Ok(())
}

// ============================================================================
// HANDLERS
// ============================================================================

#[log()]
pub async fn handle_info(tx: Sender<Bytes>, action: InfraInfoAction) -> Result<()> {
    let host = resolve_host(action.host.as_deref())?;

    let mut conn = SshConnection::connect(&host, action.identity_file.as_deref()).await?;

    let cmd = "echo '=== SYSTEM INFO ===' && (lsb_release -d 2>/dev/null || cat /etc/os-release | grep PRETTY_NAME) && uptime && \
               echo '\n=== CPU & RAM ===' && free -h && \
               echo '\n=== DISK USAGE ===' && df -h / && \
               echo '\n=== FAILED SERVICES ===' && (systemctl --failed --plain --no-legend 2>/dev/null || echo 'N/A')";
    let output = conn.exec(cmd).await?;

    info!("Received system info for host: `{host}`.");
    tx.send(Event::Answer(output))?;

    Ok(())
}

#[log()]
pub async fn handle_user(tx: Sender<Bytes>, action: InfraUserAction) -> Result<()> {
    let host = resolve_host(action.host.as_deref())?;
    let identity = action.identity_file.as_deref();

    let mut conn = SshConnection::connect(&host, identity).await?;

    match action.action.as_str() {
        "list" => {
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

            upload_pubkey_to_vps(&mut conn, user, &pubkey).await?;
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
            let path = expand_home(&raw_path);

            let pubkey = fs::read_to_string(&path).await.map_err(|e| {
                Error::Custom(format!(
                    "Failed to read public key file `{}`: {e}",
                    path.display()
                ))
            })?;

            upload_pubkey_to_vps(&mut conn, user, &pubkey).await?;
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

            // Generate ED25519 Private Key via ssh_key / russh_keys
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

            let nonce = generate_nonce();
            let key_name = format!("{user}@{host}_{nonce}");
            let priv_key_path = ssh_dir.join(&key_name);
            let pub_key_path = ssh_dir.join(format!("{key_name}.pub"));

            // Write private key (PEM encoded PKCS#8)
            let mut priv_file = std::fs::File::create(&priv_key_path)
                .map_err(|e| Error::Custom(format!("Failed to create private key file: {e}")))?;
            russh_keys::encode_pkcs8_pem(&private_key, &mut priv_file)
                .map_err(|e| Error::Custom(format!("Failed to write private key to file: {e}")))?;

            // Format OpenSSH public key line
            let public_key = private_key.public_key();
            let pubkey_str = public_key
                .to_openssh()
                .map_err(|e| Error::Custom(format!("Failed to format public key: {e}")))?;

            fs::write(&pub_key_path, format!("{pubkey_str} {user}@{host}\n")).await?;

            upload_pubkey_to_vps(&mut conn, user, &pubkey_str).await?;

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

            let cmd = if grant {
                format!("sudo usermod -aG sudo '{user}'")
            } else {
                format!("sudo gpasswd -d '{user}' sudo")
            };

            conn.exec(&cmd).await?;
            let status_str = if grant { "granted to" } else { "revoked from" };
            let msg = format!("Sudo privileges {status_str} user `{user}`.");

            info!("{msg}");
            tx.send(Event::Answer(msg))?;
        }

        _ => return Err(Error::Custom("Invalid user action.".into()).into()),
    }

    Ok(())
}

#[log()]
pub async fn handle_tunnel(tx: Sender<Bytes>, action: InfraTunnelAction) -> Result<()> {
    let port = action.local_port;

    let vps = resolve_host(action.vps_host.as_deref())?;

    match action.action.as_str() {
        "start" => {
            let addr = format!("127.0.0.1:{port}");
            let listener = TcpListener::bind(&addr).await.map_err(|e| {
                Error::Custom(format!("Port {port} is already in use or bind failed: {e}"))
            })?;

            let conn = SshConnection::connect(&vps, action.identity_file.as_deref()).await?;
            let session = Arc::new(conn.session);

            let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
            {
                let mut tunnels = ACTIVE_TUNNELS.lock().unwrap();
                if let Some(old_stop_tx) = tunnels.insert(port, stop_tx) {
                    let _ = old_stop_tx.send(());
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
            let mut tunnels = ACTIVE_TUNNELS.lock().unwrap();

            let msg = if let Some(stop_tx) = tunnels.remove(&port) {
                let _ = stop_tx.send(());
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
            let is_registered = ACTIVE_TUNNELS.lock().unwrap().contains_key(&port);
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

#[log()]
pub async fn handle_transfer(tx: Sender<Bytes>, action: InfraTransferAction) -> Result<()> {
    let host = resolve_host(action.host.as_deref())?;

    let mut conn = SshConnection::connect(&host, action.identity_file.as_deref()).await?;

    let local_path = expand_home(&action.local_path);
    let remote_path = action.remote_path.as_str();

    match action.direction.as_str() {
        "upload" => {
            let content = fs::read(&local_path).await.map_err(|e| {
                Error::Custom(format!(
                    "Failed to read local file `{}`: {e}",
                    local_path.display()
                ))
            })?;

            // write file directly via SSH using base64 stream (safe for binary files)
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
            let cmd = format!("base64 '{remote_path}'");
            let output = conn.exec(&cmd).await?;
            let clean_b64 = output.replace(['\r', '\n'], "");

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

    Ok(())
}

#[log()]
pub async fn handle_sync(tx: Sender<Bytes>, action: InfraSyncConfigAction) -> Result<()> {
    let host = resolve_host(action.host.as_deref())?;

    let mut conn = SshConnection::connect(&host, action.identity_file.as_deref()).await?;

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());

    let (local_rel, remote_rel) = get_editor_paths(&action.editor)?;
    let local_full = PathBuf::from(home).join(local_rel);

    match action.direction.as_str() {
        "push" => {
            if local_full.is_file() {
                let content = fs::read(&local_full).await?;
                let encoded = BASE64.encode(content);
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
            let cmd = format!("base64 '~/{remote_rel}'");
            let output = conn.exec(&cmd).await?;
            let clean_b64 = output.replace(['\r', '\n'], "");

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
    info!("{msg}");
    tx.send(Event::Answer(msg))?;

    Ok(())
}
