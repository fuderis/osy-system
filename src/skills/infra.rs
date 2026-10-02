use crate::prelude::*;

use anylm::{Schema, api::Tool};
use atoman::fs;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use russh::{
    client::{self, Config, Handler},
    keys::{PrivateKeyWithHashAlg, PublicKeyOrCertificate},
};
use russh_keys::ssh_key::rand_core::OsRng;

pub fn tools_list() -> Vec<Tool> {
    vec![
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
    ]
}

// ============================================================================
// DTO STRUCTS
// ============================================================================

#[derive(Debug, Deserialize, Schema)]
pub struct InfoAction {
    /// Target VPS IP/Host (e.g. '192.168.1.1' or 'user@192.168.1.1'). Omit for DEFAULT_VPS_HOST.
    pub host: Option<String>,
    /// Path to private SSH key file. Defaults to ~/.ssh/id_ed25519.
    pub identity_file: Option<String>,
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
    pub identity_file: Option<String>,
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
    pub identity_file: Option<String>,
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
pub async fn handle_info(tx: Sender<Bytes>, action: InfoAction) -> Result<()> {
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
pub async fn handle_user(tx: Sender<Bytes>, action: UserAction) -> Result<()> {
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
pub async fn handle_transfer(tx: Sender<Bytes>, action: TransferAction) -> Result<()> {
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
pub async fn handle_sync(tx: Sender<Bytes>, action: SyncConfigAction) -> Result<()> {
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
