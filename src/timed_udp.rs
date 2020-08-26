use tokio::net::{UdpSocket, ToSocketAddrs};
use tokio::time::timeout;
use std::time::Duration;
use std::net::SocketAddr;

pub struct TimedUdpSocket {
    socket: UdpSocket,
    duration: Duration,
}

impl TimedUdpSocket {
    pub async fn new(duration: Duration) -> std::io::Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        Ok(Self {socket, duration})
    }

    pub async fn send_to(&mut self, buf: &[u8], target: impl ToSocketAddrs) -> std::io::Result<usize> {
        timeout(self.duration, self.socket.send_to(buf, target)).await?
    }

    pub async fn recv_from(&mut self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        timeout(self.duration, self.socket.recv_from(buf)).await?
    }
}