//! A fake X-Plane 11: reads RREF subscriptions and answers with fixed values.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::time::timeout;

pub const T: Duration = Duration::from_secs(15);

pub fn parse_request(b: &[u8]) -> (i32, i32, String) {
    assert_eq!(b.len(), 413);
    assert_eq!(&b[..5], b"RREF\0");
    let freq = i32::from_le_bytes(b[5..9].try_into().unwrap());
    let idx = i32::from_le_bytes(b[9..13].try_into().unwrap());
    let end = b[13..].iter().position(|&x| x == 0).unwrap();
    (
        freq,
        idx,
        String::from_utf8(b[13..13 + end].to_vec()).unwrap(),
    )
}

/// Reads one full round of subscription requests; returns the client address and name by index.
pub async fn read_subscriptions(fake: &UdpSocket) -> (SocketAddr, HashMap<i32, String>) {
    let mut names = HashMap::new();
    let mut buf = [0u8; 1024];
    loop {
        let (n, from) = timeout(T, fake.recv_from(&mut buf)).await.unwrap().unwrap();
        let (freq, idx, name) = parse_request(&buf[..n]);
        assert!(freq > 0);
        names.insert(idx, name);
        if names.len() == atc_client::datarefs::SUBSCRIPTION_COUNT {
            return (from, names);
        }
    }
}

/// One RREF packet with `value(name)` for every subscribed dataref.
pub async fn answer(
    fake: &UdpSocket,
    to: SocketAddr,
    names: &HashMap<i32, String>,
    value: impl Fn(&str) -> f32,
) {
    let mut p = b"RREF,".to_vec();
    for (i, n) in names {
        p.extend(i.to_le_bytes());
        p.extend(value(n).to_le_bytes());
    }
    fake.send_to(&p, to).await.unwrap();
}
