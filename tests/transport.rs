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
