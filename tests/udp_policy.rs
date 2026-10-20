// A separate test executable keeps the immutable process policy isolated.
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

#[tokio::test]
async fn quic_socket_protection_runs_on_initial_bind_and_rebind_and_never_falls_back() {
    let deny = Arc::new(AtomicBool::new(false));
    let count = Arc::new(AtomicUsize::new(0));
    let (policy, calls) = (deny.clone(), count.clone());
    netwatch::set_socket_protector(Arc::new(move |_, _| {
        calls.fetch_add(1, Ordering::SeqCst);
        if policy.load(Ordering::SeqCst) {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fixture: interface denied",
            ))
        } else {
            Ok(())
        }
    }))
    .unwrap();
    let socket =
        netwatch::UdpSocket::bind_full("127.0.0.1:0".parse::<std::net::SocketAddr>().unwrap())
            .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    socket.rebind().unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
    deny.store(true, Ordering::SeqCst);
    assert!(socket.rebind().is_err());
    assert!(netwatch::UdpSocket::bind_v4(0).is_err());
    assert!(netwatch::set_socket_protector(Arc::new(|_, _| Ok(()))).is_err());
}
