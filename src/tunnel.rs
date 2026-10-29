use crate::{
    network::{self, RouteMode},
    state::{State, loopback},
};
use anyhow::{Context, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use iroh::{
    Endpoint, EndpointAddr, EndpointId, RelayMode, SecretKey,
    endpoint::{Connection, RecvStream, SendStream, presets},
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::JoinSet,
};

pub const ALPN: &[u8] = b"tdssh/ssh/1";
const SETUP: Duration = Duration::from_secs(20);

#[derive(Clone, Default)]
pub struct Network {
    pub force_direct: bool,
    pub interface: Option<String>,
    pub relays: Vec<String>,
    pub no_relay: bool,
    pub relay_only: bool,
}

pub async fn endpoint(secret: SecretKey, options: &Network) -> anyhow::Result<Endpoint> {
    ensure!(
        !(options.no_relay && (options.relay_only || !options.relays.is_empty())),
        "--no-relay conflicts with relay options"
    );
    ensure!(
        options.force_direct || options.interface.is_none(),
        "--interface requires --force-direct"
    );
    let mut builder = if options.no_relay || !options.relays.is_empty() {
        Endpoint::builder(presets::Minimal)
    } else {
        Endpoint::builder(presets::N0)
    };
    if options.no_relay {
        builder = builder.relay_mode(RelayMode::Disabled);
    }
    if !options.relays.is_empty() {
        let mut urls = Vec::new();
        for relay in &options.relays {
            let url: url::Url = relay.parse()?;
            ensure!(
                url.scheme() == "https"
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none(),
                "relays require HTTPS without credentials, query or fragment"
            );
            urls.push(relay.parse()?);
        }
        builder = builder.relay_mode(RelayMode::custom(urls));
    }
    if options.relay_only {
        builder = builder.clear_ip_transports();
    }
    builder = network::configure(
        builder,
        if options.force_direct {
            RouteMode::Direct
        } else {
            RouteMode::System
        },
        options.interface.as_deref(),
        !options.no_relay && options.relays.is_empty(),
        options.relay_only,
        &options.relays,
    )
    .await?;
    let transport = iroh::endpoint::QuicTransportConfig::builder()
        .max_concurrent_bidi_streams(32u32.into())
        .max_concurrent_uni_streams(0u32.into())
        .keep_alive_interval(Duration::from_secs(15))
        .max_idle_timeout(Some(Duration::from_secs(90).try_into()?))
        .build();
    let endpoint = builder
        .secret_key(secret)
        .alpns(vec![ALPN.to_vec()])
        .portmapper_config(iroh::endpoint::PortmapperConfig::Disabled)
        .transport_config(transport)
        .bind()
        .await
        .context("binding QUIC endpoint")?;
    if !options.no_relay
        && tokio::time::timeout(Duration::from_secs(40), endpoint.online())
            .await
            .is_err()
    {
        endpoint.close().await;
        anyhow::bail!(
            "relay registration timed out; check physical connectivity, DNS or custom relay settings"
        );
    }
    Ok(endpoint)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ticket {
    version: u32,
    address: EndpointAddr,
}
pub fn ticket(endpoint: &Endpoint) -> anyhow::Result<String> {
    Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&Ticket {
        version: 1,
        address: endpoint.addr(),
    })?))
}
pub fn peer(id: EndpointId, ticket: Option<&str>) -> anyhow::Result<EndpointAddr> {
    if let Some(ticket) = ticket {
        ensure!(ticket.len() <= 16384, "ticket too large");
        let ticket: Ticket = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(ticket.trim())?)?;
        ensure!(
            ticket.version == 1 && ticket.address.id == id,
            "ticket identity differs from the pinned --peer ID"
        );
        Ok(ticket.address)
    } else {
        Ok(EndpointAddr::new(id))
    }
}

pub async fn serve(endpoint: Endpoint, state: State, target: SocketAddr) -> anyhow::Result<()> {
    loopback(target)?;
    let slots = Arc::new(Semaphore::new(32));
    let mut jobs = JoinSet::new();
    loop {
        tokio::select! {
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else { break; };
                let Ok(permit) = slots.clone().try_acquire_owned() else { incoming.refuse(); continue; };
                let state = state.clone();
                jobs.spawn(async move {
                    let _permit = permit;
                    let result = async {
                        let connection = tokio::time::timeout(SETUP, incoming).await??;
                        if !state.allowed(connection.remote_id()) {
                            connection.close(1u32.into(), b"device not authorized");
                            anyhow::bail!("device not authorized");
                        }
                        session(connection, state, target).await
                    }.await;
                    if result.is_err() { eprintln!("session closed or rejected"); }
                });
            }
            _ = jobs.join_next(), if !jobs.is_empty() => {},
            _ = shutdown() => break,
        }
    }
    jobs.abort_all();
    endpoint.close().await;
    Ok(())
}
async fn session(connection: Connection, state: State, target: SocketAddr) -> anyhow::Result<()> {
    let mut revoke = tokio::time::interval(Duration::from_secs(1));
    let mut jobs = JoinSet::new();
    let slots = Arc::new(Semaphore::new(32));
    loop {
        tokio::select! {
            _ = revoke.tick() => {
                if !state.allowed(connection.remote_id()) {
                    connection.close(1u32.into(), b"device authorization revoked");
                    break;
                }
            }
            stream = connection.accept_bi() => {
                let Ok((send, recv)) = stream else { break; };
                if !state.allowed(connection.remote_id()) { connection.close(1u32.into(), b"device not authorized"); break; }
                let Ok(permit) = slots.clone().try_acquire_owned() else { drop((send, recv)); continue; };
                jobs.spawn(async move {
                    let _permit = permit;
                    let _ = bridge_target(send, recv, target).await;
                });
            }
            _ = jobs.join_next(), if !jobs.is_empty() => {},
        }
    }
    jobs.abort_all();
    Ok(())
}
async fn bridge_target(
    mut send: SendStream,
    mut recv: RecvStream,
    target: SocketAddr,
) -> anyhow::Result<()> {
    let tcp = tokio::time::timeout(SETUP, async {
        let mut version = [0];
        recv.read_exact(&mut version).await?;
        ensure!(version == [1], "unsupported stream protocol");
        Ok::<_, anyhow::Error>(TcpStream::connect(target).await?)
    })
    .await?;
    let tcp = match tcp {
        Ok(tcp) => tcp,
        Err(error) => {
            send.write_all(&[1]).await?;
            send.finish()?;
            return Err(error);
        }
    };
    tcp.set_nodelay(true)?;
    send.write_all(&[0]).await?;
    let (mut read, mut write) = tcp.into_split();
    copy(&mut read, &mut write, send, recv).await
}

pub async fn connect(
    endpoint: &Endpoint,
    peer: EndpointAddr,
) -> anyhow::Result<(Connection, SendStream, RecvStream)> {
    tokio::time::timeout(SETUP, async {
        let connection = endpoint.connect(peer, ALPN).await?;
        let (mut send, mut recv) = connection.open_bi().await?;
        send.write_all(&[1]).await?;
        let mut response = [255];
        recv.read_exact(&mut response)
            .await
            .context("peer rejected the session; verify tdssh allow on the server")?;
        ensure!(
            response == [0],
            "server could not connect to its local SSH service"
        );
        eprintln!(
            "connected peer={} paths={:?}",
            connection.remote_id(),
            connection.paths()
        );
        Ok((connection, send, recv))
    })
    .await
    .context("peer connection timed out")?
}

pub async fn copy<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    input: &mut R,
    output: &mut W,
    mut send: SendStream,
    mut recv: RecvStream,
) -> anyhow::Result<()> {
    let outbound = async {
        tokio::io::copy(input, &mut send).await?;
        send.finish()?;
        // Wait until the FIN and all bytes are acknowledged before the caller
        // can close its endpoint. This preserves uploads and half-close.
        send.stopped().await?;
        Ok::<_, anyhow::Error>(())
    };
    let inbound = async {
        tokio::io::copy(&mut recv, output).await?;
        output.shutdown().await?;
        Ok::<_, anyhow::Error>(())
    };
    tokio::try_join!(outbound, inbound)?;
    Ok(())
}

pub async fn proxy(endpoint: Endpoint, peer: EndpointAddr) -> anyhow::Result<()> {
    let result = async {
        let (_connection, send, recv) = connect(&endpoint, peer).await?;
        copy(
            &mut tokio::io::stdin(),
            &mut tokio::io::stdout(),
            send,
            recv,
        )
        .await
    }
    .await;
    endpoint.close().await;
    result
}

pub async fn forward(
    listener: TcpListener,
    endpoint: Endpoint,
    peer: EndpointAddr,
) -> anyhow::Result<()> {
    forward_with_idle_timeout(listener, endpoint, peer, None).await
}

pub async fn forward_with_idle_timeout(
    listener: TcpListener,
    endpoint: Endpoint,
    peer: EndpointAddr,
    idle_timeout: Option<Duration>,
) -> anyhow::Result<()> {
    loopback(listener.local_addr()?)?;
    let slots = Arc::new(Semaphore::new(32));
    let mut jobs = JoinSet::new();
    let mut idle_deadline = idle_timeout.map(|timeout| tokio::time::Instant::now() + timeout);
    loop {
        tokio::select! {
            accept = listener.accept() => {
                let (tcp, _) = accept?;
                let Ok(permit) = slots.clone().try_acquire_owned() else { continue; };
                idle_deadline = None;
                let endpoint = endpoint.clone();
                let peer = peer.clone();
                jobs.spawn(async move {
                    let _permit = permit;
                    let result = async {
                        tcp.set_nodelay(true)?;
                        let (_connection, send, recv) = connect(&endpoint, peer).await?;
                        let (mut read, mut write) = tcp.into_split();
                        copy(&mut read, &mut write, send, recv).await
                    }.await;
                    if result.is_err() { eprintln!("forwarded session closed or failed"); }
                });
            }
            _ = jobs.join_next(), if !jobs.is_empty() => {
                if jobs.is_empty() {
                    idle_deadline = idle_timeout.map(|timeout| tokio::time::Instant::now() + timeout);
                }
            },
            _ = tokio::time::sleep_until(idle_deadline.unwrap_or_else(tokio::time::Instant::now)),
                if idle_deadline.is_some() && jobs.is_empty() => {
                eprintln!("forward idle timeout reached; exiting");
                break;
            },
            _ = shutdown() => break,
        }
    }
    jobs.abort_all();
    endpoint.close().await;
    Ok(())
}

pub async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
