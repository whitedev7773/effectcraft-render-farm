use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const DISCOVERY_PORT: u16 = 9779;
const PROBE: &[u8] = b"effectcraft-farm-discover-v1";
const MARKER: &str = "effectcraft-farm-host-v1";
const MAX_PACKET: usize = 512;
const MAX_HOSTS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    pub name: String,
    pub address: SocketAddr,
}

#[derive(Serialize, Deserialize)]
struct Reply {
    marker: String,
    name: String,
    port: u16,
    bind_ip: IpAddr,
}

fn host_name() -> String {
    let name = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "EffectCraft Host".into());
    let name: String = name.chars().filter(|c| !c.is_control()).take(48).collect();
    if name.is_empty() { "EffectCraft Host".into() } else { name }
}

fn reply_bytes(port: u16, bind_ip: IpAddr, name: String) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(&Reply { marker: MARKER.into(), name, port, bind_ip }).map_err(|e| format!("검색 응답 인코딩: {e}"))?;
    if bytes.len() > MAX_PACKET {
        return Err("검색 응답이 너무 깁니다".into());
    }
    Ok(bytes)
}

fn parse_reply(bytes: &[u8], source: SocketAddr) -> Option<Host> {
    if bytes.len() > MAX_PACKET {
        return None;
    }
    let reply: Reply = serde_json::from_slice(bytes).ok()?;
    if reply.marker != MARKER || reply.port == 0 || reply.name.is_empty() || reply.name.chars().count() > 48 || reply.name.chars().any(char::is_control) {
        return None;
    }
    if reply.bind_ip.is_loopback() && !source.ip().is_loopback() {
        return None;
    }
    let ip = if reply.bind_ip.is_unspecified() { source.ip() } else { reply.bind_ip };
    if ip.is_unspecified() || ip.is_multicast() {
        return None;
    }
    Some(Host { name: reply.name, address: SocketAddr::new(ip, reply.port) })
}

fn respond(socket: &UdpSocket, response: &[u8]) -> Result<(), String> {
    let mut packet = [0u8; MAX_PACKET + 1];
    match socket.recv_from(&mut packet) {
        Ok((size, peer)) if packet.get(..size) == Some(PROBE) => {
            socket.send_to(response, peer).map_err(|e| format!("검색 응답 보내기: {e}"))?;
            Ok(())
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => Ok(()),
        Err(e) => Err(format!("검색 요청 받기: {e}")),
    }
}

/// Start the LAN responder after the TCP coordinator is ready. Discovery is optional;
/// an unavailable UDP port must not prevent rendering through a manually entered IP.
pub fn start_responder(tcp_address: SocketAddr, stop: Arc<AtomicBool>) -> Result<(), String> {
    let socket = UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)).map_err(|e| format!("LAN 검색 포트 {DISCOVERY_PORT}: {e}"))?;
    socket.set_read_timeout(Some(Duration::from_millis(250))).map_err(|e| format!("LAN 검색 수신 설정: {e}"))?;
    let response = reply_bytes(tcp_address.port(), tcp_address.ip(), host_name())?;
    thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            if respond(&socket, &response).is_err() {
                thread::sleep(Duration::from_millis(250));
            }
        }
    });
    Ok(())
}

fn scan_targets(targets: &[SocketAddr], timeout: Duration) -> Result<Vec<Host>, String> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("LAN 검색 소켓: {e}"))?;
    socket.set_broadcast(true).map_err(|e| format!("LAN 검색 브로드캐스트: {e}"))?;
    socket.set_read_timeout(Some(Duration::from_millis(150))).map_err(|e| format!("LAN 검색 수신 설정: {e}"))?;
    let mut sent = false;
    for target in targets {
        sent |= socket.send_to(PROBE, target).is_ok();
    }
    if !sent {
        return Err("LAN 검색 요청을 보내지 못했습니다".into());
    }
    let deadline = Instant::now() + timeout.min(Duration::from_secs(3));
    let mut hosts = Vec::new();
    let mut packet = [0u8; MAX_PACKET + 1];
    while Instant::now() < deadline && hosts.len() < MAX_HOSTS {
        match socket.recv_from(&mut packet) {
            Ok((size, source)) => {
                if let Some(host) = packet.get(..size).and_then(|bytes| parse_reply(bytes, source))
                    && !hosts.iter().any(|known: &Host| known.address == host.address)
                {
                    hosts.push(host);
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::ConnectionReset => {}
            Err(e) => return Err(format!("LAN 검색 응답 받기: {e}")),
        }
    }
    hosts.sort_by_key(|host| host.address);
    Ok(hosts)
}

pub fn scan() -> Result<Vec<Host>, String> {
    scan_targets(&[SocketAddr::from(([255, 255, 255, 255], DISCOVERY_PORT)), SocketAddr::from(([127, 0, 0, 1], DISCOVERY_PORT))], Duration::from_millis(900))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unrelated_and_malformed_advertisements() {
        let source = SocketAddr::from(([192, 168, 1, 10], DISCOVERY_PORT));
        assert!(parse_reply(b"not a reply", source).is_none());
        assert!(parse_reply(&reply_bytes(0, source.ip(), "host".into()).unwrap(), source).is_none());
        assert!(parse_reply(&reply_bytes(9778, source.ip(), "bad\nname".into()).unwrap(), source).is_none());
        assert!(parse_reply(&reply_bytes(9778, IpAddr::from([127, 0, 0, 1]), "local-only".into()).unwrap(), source).is_none());
        let valid = parse_reply(&reply_bytes(9778, IpAddr::from([0, 0, 0, 0]), "render-pc".into()).unwrap(), source).unwrap();
        assert_eq!(valid.address, SocketAddr::from(([192, 168, 1, 10], 9778)));
    }

    #[test]
    fn finds_responder_on_loopback() {
        let listener = UdpSocket::bind("127.0.0.1:0").unwrap();
        listener.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let target = listener.local_addr().unwrap();
        let responder = thread::spawn(move || respond(&listener, &reply_bytes(9778, IpAddr::from([0, 0, 0, 0]), "test-host".into()).unwrap()));
        let found = scan_targets(&[target], Duration::from_millis(500)).unwrap();
        assert!(responder.join().unwrap().is_ok());
        assert_eq!(found, vec![Host { name: "test-host".into(), address: SocketAddr::from(([127, 0, 0, 1], 9778)) }]);
    }
}
