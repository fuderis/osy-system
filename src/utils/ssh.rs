use crate::prelude::*;

use russh::{
    client::{self, Config, Handler},
    keys::{PrivateKeyWithHashAlg, PublicKeyOrCertificate},
};

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

pub fn parse_host_string(raw: &str) -> Result<(String, String, u16)> {
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

pub fn resolve_identity_file(override_path: Option<&str>) -> Result<PathBuf> {
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
        if rsa.exists() { Ok(rsa) } else { Ok(ed25519) }
    }
}

pub fn generate_nonce() -> u16 {
    rand::random::<u16>()
}

pub fn expand_home(path: &str) -> PathBuf {
    if path.starts_with('~') {
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            return PathBuf::from(path.replacen('~', &home, 1));
        }
    }
    PathBuf::from(path)
}

pub fn resolve_host(override_host: Option<&str>) -> Result<String> {
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

pub fn clean_host_target(raw: &str) -> String {
    let mut host = raw.trim();

    if let Some((_, h)) = host.split_once('@') {
        host = h;
    }

    if let Some((h, _)) = host.split_once(':') {
        host = h;
    }

    host.to_string()
}

pub fn resolve_host_target(target: &Option<String>, ip: &Option<String>) -> Result<String> {
    let raw = if let Some(ip_addr) = ip {
        let trimmed = ip_addr.trim();
        if !trimmed.is_empty() {
            Some(trimmed.to_string())
        } else {
            None
        }
    } else {
        None
    };

    let raw = raw.or_else(|| {
        target.as_ref().and_then(|t| {
            let trimmed = t.trim();
            if !trimmed.is_empty() {
                Some(trimmed.to_string())
            } else {
                None
            }
        })
    });

    let host_str = match raw {
        Some(h) => h,
        None => std::env::var("DEFAULT_VPS_HOST")
            .or_else(|_| std::env::var("DEFAULT_TARGET"))
            .map_err(|_| Error::Custom("Target host or IP address was not specified.".into()))?,
    };

    let cleaned = clean_host_target(&host_str);
    if cleaned.is_empty() {
        return Err(Error::Custom("Resolved target host address is empty.".into()).into());
    }

    Ok(cleaned)
}

pub async fn upload_pubkey_to_vps(
    conn: &mut SshConnection,
    user: &str,
    pubkey: &str,
) -> Result<()> {
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

pub fn get_editor_paths(editor: &str) -> Result<(&'static str, &'static str)> {
    match editor {
        "helix" => Ok((".config/helix", ".config/helix")),
        "neovim" => Ok((".config/nvim", ".config/nvim")),
        "vim" => Ok((".vimrc", ".vimrc")),
        _ => Err(Error::Custom("Unsupported editor".into()).into()),
    }
}
