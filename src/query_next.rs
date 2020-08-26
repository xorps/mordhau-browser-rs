use tokio::time::timeout;
use tokio::net::UdpSocket;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;
use tokio::sync::mpsc;
use futures::try_join;

const A2S_INFO: &[u8] = b"\xFF\xFF\xFF\xFF\x54Source Engine Query\x00";
const MASTER_SERVER: &str = "hl2master.steampowered.com:27011";

async fn query_server(addr: SocketAddrV4, sender: mpsc::UnboundedSender<String>) {
    async fn exec(addr: SocketAddrV4) -> std::io::Result<SocketAddrV4> {
        let mut socket = UdpSocket::bind("0.0.0.0:0").await?;
        let _ = socket.send_to(A2S_INFO, addr).await?;
        let mut buf = vec![0u8; 2048];
        let _ = socket.recv_from(&mut buf).await?;
        Ok(addr)
    }

    let res = timeout(Duration::from_secs(5), exec(addr)).await;
    let _ = sender.send(match res {
        Err(err) => format!("{}", err),
        Ok(Ok(addr)) => format!("{}", addr),
        Ok(Err(err)) => format!("{}", err),
    });
}

fn query(addr: SocketAddrV4) -> Vec<u8> {
    let mut query = vec![];
    query.extend_from_slice(b"\x31\xFF");
    query.extend(format!("{}", addr).as_bytes());
    query.extend_from_slice(b"\x00\\appid\\629760\x00");
    query
}

struct BytesAddr<'a>(&'a [u8]);

impl<'a> From<BytesAddr<'a>> for SocketAddrV4 {
    fn from(addr: BytesAddr<'a>) -> Self {
        let BytesAddr(buf) = addr;
        let ip = Ipv4Addr::new(buf[0], buf[1], buf[2], buf[3]);
        let port = u16::from_be_bytes([buf[4], buf[5]]);
        SocketAddrV4::new(ip, port)
    }
}

async fn exec_query(socket: &mut UdpSocket, buf: &mut [u8], seed: SocketAddrV4, host: &mut SocketAddr, len: &mut usize) -> std::io::Result<()> {
    let _ = socket.send_to(&query(seed), *host).await?;
    let (new_len, new_host) = socket.recv_from(buf).await?;
    *len = new_len;
    *host = new_host;
    Ok(())
}

async fn query_iter(socket: &mut UdpSocket, host: &mut SocketAddr, sender: mpsc::UnboundedSender<String>, buf: &mut [u8], mut len: usize) -> std::io::Result<()> {
    const EOL: &[u8] = &[0, 0, 0, 0, 0, 0];
    loop {
        let seed = &buf[len-6..len];
        let stop = seed == EOL;
        let data = (if stop { &buf[6..len-6] } else { &buf[6..] }).to_vec();
        let sender = sender.clone();
        tokio::spawn(async move {
            let iter = data.chunks_exact(6);
            iter.map(|bytes| BytesAddr(bytes).into()).for_each(|addr| {
                tokio::spawn(query_server(addr, sender.clone()));
            });
        });
        if stop { return Ok(()); }
        let seed = BytesAddr(seed).into();
        exec_query(socket, buf, seed, host, &mut len).await?;
    }
}

async fn start_query(sender: mpsc::UnboundedSender<String>) -> std::io::Result<()> {
    let mut socket: UdpSocket = UdpSocket::bind("0.0.0.0:0").await?;
    let mut buf = [0u8; 2048];
    let seed = SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0);
    let _ = socket.send_to(&query(seed), MASTER_SERVER).await?;
    let (len, mut host) = socket.recv_from(&mut buf).await?;
    query_iter(&mut socket, &mut host, sender, &mut buf, len).await
}

async fn wait_loop(mut recv: mpsc::UnboundedReceiver<String>) -> std::io::Result<()> {
    loop {
        match recv.recv().await {
            Some(msg) => println!("{}", msg),
            None => { println!("wait_loop closed"); return Ok(()) }
        }
    }
}

async fn main_task() -> std::io::Result<()> {
    let (send, recv) = mpsc::unbounded_channel::<String>();
    let _ = try_join!(start_query(send), wait_loop(recv))?;
    Ok(())
}

pub fn get_servers() -> std::io::Result<()> {
    let mut rt = tokio::runtime::Builder::new().threaded_scheduler().enable_all().build()?;
    rt.block_on(main_task())
}