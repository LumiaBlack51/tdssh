use iroh::{Endpoint, RelayMode, SecretKey, endpoint::presets};
use std::{sync::Arc, time::Duration};
use tdssh::{state::State, tunnel};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

async fn local() -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")
        .unwrap()
        .alpns(vec![tunnel::ALPN.to_vec()])
        .bind()
        .await
        .unwrap()
}
async fn bounded<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(15), f)
        .await
        .expect("test timed out")
}

#[tokio::test]
async fn forward_exits_after_initial_idle_timeout() {
    let (server, client) = (local().await, local().await);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    bounded(tunnel::forward_with_idle_timeout(
        listener,
        client,
        server.addr(),
        Some(Duration::from_millis(50)),
    ))
    .await
    .unwrap();
    server.close().await;
}

#[tokio::test]
async fn forward_idle_timeout_preserves_live_sessions_and_rearms_after_last_close() {
    let dir = tempfile::tempdir().unwrap();
    let state = State::new(dir.path().join("state"));
    state.init().unwrap();
    let (server, client) = (local().await, local().await);
    state.authorize(client.id(), true).unwrap();
    let backend_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = backend_listener.local_addr().unwrap();
    let backend = tokio::spawn(async move {
        let mut jobs = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = backend_listener.accept() => {
                    let (tcp, _) = accepted.unwrap();
                    jobs.spawn(async move {
                        let (mut read, mut write) = tcp.into_split();
                        tokio::io::copy(&mut read, &mut write).await.unwrap();
                    });
                }
                _ = jobs.join_next(), if !jobs.is_empty() => {},
            }
        }
    });
    let service = tokio::spawn(tunnel::serve(server.clone(), state, target));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let forward = tokio::spawn(tunnel::forward_with_idle_timeout(
        listener,
        client.clone(),
        server.addr(),
        Some(Duration::from_millis(200)),
    ));
    let mut sessions = Vec::new();
    for _ in 0..2 {
        let mut tcp = TcpStream::connect(addr).await.unwrap();
        tcp.write_all(b"x").await.unwrap();
        assert_eq!(bounded(tcp.read_u8()).await.unwrap(), b'x');
        sessions.push(tcp);
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !forward.is_finished(),
        "quiet live sessions must remain open"
    );
    drop(sessions.pop());
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !forward.is_finished(),
        "one remaining session prevents exit"
    );
    drop(sessions);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !forward.is_finished(),
        "last close must get a full idle interval"
    );
    let mut renewed = TcpStream::connect(addr).await.unwrap();
    renewed.write_all(b"y").await.unwrap();
    assert_eq!(bounded(renewed.read_u8()).await.unwrap(), b'y');
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !forward.is_finished(),
        "new session cancels the idle countdown"
    );
    drop(renewed);
    bounded(forward).await.unwrap().unwrap();
    backend.abort();
    service.abort();
    client.close().await;
    server.close().await;
}

#[tokio::test]
async fn forward_without_idle_timeout_remains_available() {
    let (server, client) = (local().await, local().await);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let forward = tokio::spawn(tunnel::forward(listener, client.clone(), server.addr()));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!forward.is_finished());
    forward.abort();
    client.close().await;
    server.close().await;
}

#[tokio::test]
async fn authenticated_quic_transfers_large_payload_and_half_close() {
    let dir = tempfile::tempdir().unwrap();
    let state = State::new(dir.path().join("state"));
    state.init().unwrap();
    let (server, client) = (local().await, local().await);
    state.authorize(client.id(), true).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = listener.local_addr().unwrap();
    let payload = Arc::new(
        (0..(4 * 1024 * 1024))
            .map(|i| (i % 251) as u8)
            .collect::<Vec<_>>(),
    );
    let expected = payload.clone();
    let backend = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        let mut received = Vec::new();
        tcp.read_to_end(&mut received).await.unwrap();
        assert_eq!(received, *expected); // Requires client FIN to reach TCP.
        tcp.write_all(&received).await.unwrap();
        tcp.shutdown().await.unwrap();
    });
    let server_endpoint = server.clone();
    let task = tokio::spawn(tunnel::serve(server_endpoint, state, target));
    let (connection, send, recv) = bounded(tunnel::connect(&client, server.addr()))
        .await
        .unwrap();
    let mut input = &payload[..];
    let mut output = Vec::new();
    bounded(tunnel::copy(&mut input, &mut output, send, recv))
        .await
        .unwrap();
    assert_eq!(output, *payload);
    bounded(backend).await.unwrap();
    drop(connection);
    task.abort();
    client.close().await;
    server.close().await;
}

#[tokio::test]
async fn unauthorized_identity_cannot_open_target_and_revocation_closes_live_connection() {
    let dir = tempfile::tempdir().unwrap();
    let state = State::new(dir.path().join("state"));
    state.init().unwrap();
    let (server, client) = (local().await, local().await);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let task = tokio::spawn(tunnel::serve(
        server.clone(),
        state.clone(),
        listener.local_addr().unwrap(),
    ));
    assert!(
        bounded(tunnel::connect(&client, server.addr()))
            .await
            .is_err()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
    state.authorize(client.id(), true).unwrap();
    let (connection, _send, _recv) = bounded(tunnel::connect(&client, server.addr()))
        .await
        .unwrap();
    let (_target, _) = bounded(listener.accept()).await.unwrap();
    state.authorize(client.id(), false).unwrap();
    tokio::time::timeout(Duration::from_secs(3), connection.closed())
        .await
        .expect("revocation must close active connection");
    assert!(
        bounded(tunnel::connect(&client, server.addr()))
            .await
            .is_err()
    );
    task.abort();
    client.close().await;
    server.close().await;
}

#[tokio::test]
async fn pinned_identity_rejects_ticket_substitution_and_unreachable_ssh_returns_error() {
    let (server, client) = (local().await, local().await);
    let ticket = tunnel::ticket(&server).unwrap();
    assert!(tunnel::peer(client.id(), Some(&ticket)).is_err());
    assert_eq!(
        tunnel::peer(server.id(), Some(&ticket)).unwrap().id,
        server.id()
    );
    let dir = tempfile::tempdir().unwrap();
    let state = State::new(dir.path().join("state"));
    state.init().unwrap();
    state.authorize(client.id(), true).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = listener.local_addr().unwrap();
    drop(listener);
    assert!(TcpStream::connect(target).await.is_err());
    let task = tokio::spawn(tunnel::serve(server.clone(), state, target));
    let error = bounded(tunnel::connect(&client, server.addr()))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("local SSH service"));
    task.abort();
    client.close().await;
    server.close().await;
}

#[test]
fn identity_initialization_is_serialized_across_concurrent_writers() {
    let dir = tempfile::tempdir().unwrap();
    let state = State::new(dir.path().join("state"));
    let jobs: Vec<_> = (0..8)
        .map(|_| {
            let state = state.clone();
            std::thread::spawn(move || state.init().unwrap().public())
        })
        .collect();
    let ids: Vec<_> = jobs.into_iter().map(|job| job.join().unwrap()).collect();
    assert!(ids.iter().all(|id| *id == ids[0]));
    let peer = SecretKey::from_bytes(&rand::random()).public();
    assert!(!state.allowed(peer));
}

#[test]
fn one_endpoint_per_identity_prevents_relay_takeover_but_acl_remains_editable() {
    let dir = tempfile::tempdir().unwrap();
    let state = State::new(dir.path().join("state"));
    state.init().unwrap();
    let lock = state.network_lock().unwrap();
    assert!(state.network_lock().is_err());
    state
        .authorize(SecretKey::from_bytes(&rand::random()).public(), true)
        .unwrap();
    drop(lock);
    assert!(state.network_lock().is_ok());
}
