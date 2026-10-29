use anyhow::{Context, ensure};
use clap::{Args, Parser, Subcommand};
use iroh::EndpointId;
use std::{net::SocketAddr, path::PathBuf};
use tdssh::{
    state::{State, loopback, write_public},
    tunnel::{self, Network},
};
use tokio::net::TcpListener;

#[derive(Parser)]
#[command(
    version,
    about = "Cross-NAT SSH over encrypted QUIC; no VPN or shared LAN required"
)]
struct Cli {
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    #[command(flatten)]
    network: NetworkArgs,
    #[command(subcommand)]
    command: Command,
}
#[derive(Args, Default)]
struct NetworkArgs {
    /// Bind DNS, relay TCP and QUIC UDP to a physical adapter; never fall back.
    #[arg(long, global = true)]
    force_direct: bool,
    /// Physical interface name, friendly name, or numeric index (requires --force-direct).
    #[arg(long, global = true, requires = "force_direct")]
    interface: Option<String>,
    /// Custom HTTPS iroh relay; repeatable. Exchange --ticket-file on both sides.
    #[arg(long, global = true, conflicts_with = "no_relay")]
    relay: Vec<String>,
    /// Disable relays (diagnostics; connectivity behind restrictive NAT is not guaranteed).
    #[arg(long, global = true, conflicts_with = "relay_only")]
    no_relay: bool,
    /// Disable peer UDP and carry encrypted QUIC only through a relay.
    #[arg(long, global = true)]
    relay_only: bool,
}
impl From<NetworkArgs> for Network {
    fn from(n: NetworkArgs) -> Self {
        Self {
            force_direct: n.force_direct,
            interface: n.interface,
            relays: n.relay,
            no_relay: n.no_relay,
            relay_only: n.relay_only,
        }
    }
}
#[derive(Args)]
struct Peer {
    /// Server's full public device ID, verified through a trusted channel.
    #[arg(long)]
    peer: EndpointId,
    /// Public address ticket saved by `serve --ticket-out`; its ID must match --peer.
    #[arg(long)]
    ticket_file: Option<PathBuf>,
}
impl Peer {
    fn address(&self) -> anyhow::Result<iroh::EndpointAddr> {
        let ticket = self
            .ticket_file
            .as_ref()
            .map(std::fs::read_to_string)
            .transpose()?;
        tunnel::peer(self.peer, ticket.as_deref())
    }
}
#[derive(Subcommand)]
enum Command {
    /// Create a private device identity; print only its public ID.
    Init,
    /// Print this device's public ID.
    Id,
    /// Authorize a verified client device ID to reach the local SSH server.
    Allow { id: EndpointId },
    /// Revoke a client; active sessions close within one second.
    Revoke { id: EndpointId },
    /// List authorized public device IDs.
    Peers,
    /// Run the server, bridging authorized peers to the local OpenSSH service.
    Serve {
        #[arg(long, default_value = "127.0.0.1:22")]
        target: SocketAddr,
        #[arg(long)]
        ticket_out: Option<PathBuf>,
    },
    /// OpenSSH ProxyCommand: stdin/stdout contain only SSH bytes.
    Proxy {
        #[command(flatten)]
        peer: Peer,
    },
    /// Bind a loopback TCP forward suitable for ssh, scp, sftp and IDEs.
    Forward {
        #[command(flatten)]
        peer: Peer,
        #[arg(long, default_value = "127.0.0.1:2222")]
        listen: SocketAddr,
        #[arg(long)]
        ready_file: Option<PathBuf>,
        /// Exit after this many seconds with no active local sessions (1..86400).
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..=86400))]
        idle_timeout: Option<u64>,
    },
    /// Start the tunnel and native OpenSSH together, preserving terminal support.
    Ssh {
        #[command(flatten)]
        peer: Peer,
        #[arg(long)]
        user: Option<String>,
        /// Repeat for each native OpenSSH option argument, e.g. --ssh-option=-i --ssh-option=key.
        #[arg(long, allow_hyphen_values = true)]
        ssh_option: Vec<String>,
        /// Remote command and its arguments, after --.
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// List interface names and indexes for --interface.
    Interfaces,
}

fn main() -> std::process::ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("tdssh: cannot start async runtime: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(run());
    // Tokio stdin uses an uncancellable blocking OS read. A peer disconnect
    // must not wait for the caller to close its stdin before this process exits.
    // Network operations explicitly close their endpoint before returning.
    runtime.shutdown_background();
    match result {
        Ok(code) => std::process::ExitCode::from(code),
        Err(error) => {
            eprintln!("tdssh: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
async fn run() -> anyhow::Result<u8> {
    let cli = Cli::parse();
    if matches!(cli.command, Command::Interfaces) {
        for iface in netdev::get_interfaces() {
            if iface.is_up() && iface.is_physical() && !iface.is_tun() && !iface.is_loopback() {
                println!(
                    "{}\t{}\t{}",
                    iface.index,
                    iface.name,
                    iface.friendly_name.unwrap_or_default()
                );
            }
        }
        return Ok(0);
    }
    let root = match cli.state_dir {
        Some(path) => path,
        None => State::default_root()?,
    };
    let state = State::new(root);
    match cli.command {
        Command::Init => {
            println!("{}", state.init()?.public());
            return Ok(0);
        }
        Command::Id => {
            println!("{}", state.identity()?.public());
            return Ok(0);
        }
        Command::Allow { id } => {
            state.authorize(id, true)?;
            println!("authorized {id}");
            return Ok(0);
        }
        Command::Revoke { id } => {
            state.authorize(id, false)?;
            println!("revoked {id}");
            return Ok(0);
        }
        Command::Peers => {
            for id in state.peers()? {
                println!("{id}");
            }
            return Ok(0);
        }
        _ => {}
    }
    // Validate local addresses and tickets before starting any networking.
    let address = match &cli.command {
        Command::Proxy { peer } | Command::Forward { peer, .. } | Command::Ssh { peer, .. } => {
            Some(peer.address()?)
        }
        Command::Serve { target, .. } => {
            loopback(*target)?;
            None
        }
        _ => None,
    };
    if let Command::Forward { listen, .. } = &cli.command {
        loopback(*listen)?;
    }
    let _network_lock = state.network_lock()?;
    let endpoint = tunnel::endpoint(state.identity()?, &cli.network.into()).await?;
    match cli.command {
        Command::Serve { target, ticket_out } => {
            let ticket = tunnel::ticket(&endpoint)?;
            if let Some(path) = ticket_out {
                write_public(&path, ticket.as_bytes())?;
            }
            eprintln!("serving device={} target={target}", endpoint.id());
            tunnel::serve(endpoint, state, target).await?;
        }
        Command::Proxy { .. } => tunnel::proxy(endpoint, address.unwrap()).await?,
        Command::Forward {
            listen,
            ready_file,
            idle_timeout,
            ..
        } => {
            let listener = TcpListener::bind(listen).await?;
            eprintln!("listening {}", listener.local_addr()?);
            if let Some(path) = ready_file {
                write_public(&path, listener.local_addr()?.to_string().as_bytes())?;
            }
            tunnel::forward_with_idle_timeout(
                listener,
                endpoint,
                address.unwrap(),
                idle_timeout.map(std::time::Duration::from_secs),
            )
            .await?;
        }
        Command::Ssh {
            peer,
            user,
            ssh_option,
            args,
        } => {
            if let Some(user) = &user {
                ensure!(
                    !user.starts_with('-') && !user.is_empty(),
                    "invalid SSH username"
                );
            }
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let port = listener.local_addr()?.port();
            let bridge = tokio::spawn(tunnel::forward(
                listener,
                endpoint.clone(),
                address.unwrap(),
            ));
            let mut ssh = tokio::process::Command::new("ssh");
            ssh.kill_on_drop(true)
                .arg("-o")
                .arg("ProxyCommand=none")
                .arg("-o")
                .arg("ProxyJump=none")
                .arg("-o")
                .arg(format!("HostKeyAlias=tdssh-{}", peer.peer))
                .arg("-p")
                .arg(port.to_string());
            if let Some(user) = user {
                ssh.arg("-l").arg(user);
            }
            // Options are passed as argv; no shell interpolation is involved.
            ssh.args(ssh_option).arg("127.0.0.1").args(args);
            let result = async {
                let mut child = ssh
                    .spawn()
                    .context("cannot start OpenSSH; install the native ssh client")?;
                tokio::select! {
                    status = child.wait() => Ok(status?.code().unwrap_or(255).clamp(0, 255) as u8),
                    _ = tunnel::shutdown() => { let _ = child.kill().await; Ok(130) },
                }
            }
            .await;
            bridge.abort();
            endpoint.close().await;
            return result;
        }
        _ => unreachable!(),
    }
    Ok(0)
}
