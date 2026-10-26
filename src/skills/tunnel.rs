use crate::{prelude::*, utils};

use anylm::Schema;
use atoman::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

// Registry for active SSH tunnels.
static ACTIVE_TUNNELS: SharedMap<u16, Sender<()>> = SharedMap::new();

#[derive(Debug, Clone, Deserialize, Schema)]
pub struct TunnelAction {
    /// Local port to bind (default: 1080).
    port: Option<u16>,
    /// Target VPS SSH host. Omit for default.
    host: Option<String>,
    /// Path to private SSH key file. Defaults to ~/.ssh/id_ed25519.
    ssh_file: Option<String>,
}

#[log()]
pub async fn handle_start(tx: Sender<Bytes>, query: ToolQuery<TunnelAction>) -> Result<()> {
    let ToolQuery { payload, .. } = query;
    let port = payload.port.unwrap_or(1080);
    let vps = utils::resolve_host(payload.host.as_deref())?;

    // stopping existing tunnel
    if let Some(stop_tx) = ACTIVE_TUNNELS.remove(&port).await {
        let _ = stop_tx.write().await.send(());
        info!("Stopped existing tunnel on port `{port}` before starting a new one.");
    }

    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| Error::Custom(format!("Port {port} is already in use or bind failed: {e}")))?;

    let conn = utils::SshConnection::connect(&vps, payload.ssh_file.as_deref()).await?;
    let session = Arc::new(conn.session);

    let (stop_tx, mut stop_rx) = atoman::oneshot_channel::<()>();
    {
        if let Some(old_stop_tx) = ACTIVE_TUNNELS.insert(port, stop_tx).await {
            old_stop_tx.write().await.send(()).ok();
        }
    }

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
                        let mut buf = [0u8; 256];
                        if socket.read_exact(&mut buf[..2]).await.is_err() {
                            return;
                        }
                        let nmethods = buf[1] as usize;
                        if socket.read_exact(&mut buf[..nmethods]).await.is_err() {
                            return;
                        }
                        if socket.write_all(&[0x05, 0x00]).await.is_err() {
                            return;
                        }

                        if socket.read_exact(&mut buf[..4]).await.is_err() {
                            return;
                        }
                        if buf[1] != 0x01 {
                            return;
                        }

                        let target_host = match buf[3] {
                            0x01 => {
                                let mut ip = [0u8; 4];
                                if socket.read_exact(&mut ip).await.is_err() {
                                    return;
                                }
                                std::net::Ipv4Addr::from(ip).to_string()
                            }
                            0x03 => {
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

                        if let Ok(channel) = session_clone
                            .channel_open_direct_tcpip(
                                &target_host,
                                target_port as u32,
                                "127.0.0.1",
                                0,
                            )
                            .await
                        {
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

        ACTIVE_TUNNELS.remove(&port).await;
        info!("SOCKS5 Proxy loop exited for port `{port}`.");
    });

    tx.send(Event::Answer(format!(
        "SOCKS5 Proxy listening locally on `127.0.0.1:{port}` via `{vps}`."
    )))?;
    Ok(())
}

#[log()]
pub async fn handle_stop(tx: Sender<Bytes>, query: ToolQuery<TunnelAction>) -> Result<()> {
    let ToolQuery { payload, .. } = query;
    let port = payload.port.unwrap_or(1080);

    let msg = if let Some(stop_tx) = ACTIVE_TUNNELS.remove(&port).await {
        let _ = stop_tx.write().await.send(());
        info!("Signal sent to stop proxy listener on port `{port}`.");
        format!("Successfully stopped SOCKS5 proxy tunnel on port `{port}`.")
    } else {
        format!("No active proxy tunnel found running on port `{port}`.")
    };

    info!("{msg}");
    tx.send(Event::Answer(msg))?;
    Ok(())
}

#[log()]
pub async fn handle_restart(tx: Sender<Bytes>, query: ToolQuery<TunnelAction>) -> Result<()> {
    handle_stop(tx.clone(), query.clone()).await?;
    tx.send(Event::Answer("\n".into()))?;
    handle_start(tx.clone(), query.clone()).await?;
    Ok(())
}

#[log()]
pub async fn handle_status(tx: Sender<Bytes>, query: ToolQuery<TunnelAction>) -> Result<()> {
    let ToolQuery { payload, .. } = query;
    let port = payload.port.unwrap_or(1080);

    let addr = format!("127.0.0.1:{port}");
    let is_port_occupied = TcpListener::bind(&addr).await.is_err();
    let is_registered = ACTIVE_TUNNELS.get(&port).await.is_some();

    let msg = if is_registered && is_port_occupied {
        format!("Tunnel status for port `{port}`: **ACTIVE** (Port occupied and worker registered)")
    } else {
        if is_registered && !is_port_occupied {
            ACTIVE_TUNNELS.remove(&port).await;
        }
        format!("No active proxy tunnel found listening on port `{port}`.")
    };

    info!("{msg}");
    tx.send(Event::Answer(msg))?;
    Ok(())
}
