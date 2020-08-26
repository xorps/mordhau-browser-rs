use tokio::{net::UdpSocket, time::timeout, task::spawn, task::JoinHandle, sync::mpsc};
use std::net::Ipv4Addr;
use std::time::Duration;

const ALL_SERVERS: &[u8] = b"\x31\xFF0.0.0.0:0\x00\\appid\\629760\x00";
const RESPONSE_HEADER: &[u8] = b"\xFF\xFF\xFF\xFF\x66\x0A";
const A2S_INFO: &[u8] = b"\xFF\xFF\xFF\xFF\x54Source Engine Query\x00";

#[derive(Copy, Clone)]
struct Port(u16);

impl From<Port> for u16 {
    fn from(port: Port) -> Self {
        port.0
    }
}

#[derive(Clone)]
pub struct Server(Ipv4Addr, Port);

impl From<(u8, u8, u8, u8, u8, u8)> for Server {
    fn from((a, b, c, d, e, f): (u8, u8, u8, u8, u8, u8)) -> Self {
        Server(Ipv4Addr::new(a, b, c, d), Port(u16::from_be_bytes([e, f])))
    }
}

impl From<Server> for (Ipv4Addr, u16) {
    fn from(server: Server) -> Self {
        (server.0, server.1.into())
    }
}

impl std::fmt::Display for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Port(port) = self.1;
        write!(f, "{}:{}", self.0, port)
    }
}

struct Seed(Server);

impl Seed {
    fn query(&self) -> Vec<u8> {
        let mut query = vec![];
        query.extend_from_slice(b"\x31\xFF");
        query.extend(self.0.to_string().as_bytes());
        query.extend_from_slice(b"\x00\\appid\\629760\x00");
        query
    }
}

enum ParseStatus {
    Continue(Seed),
    Stop,
}

enum ParseError<'a> {
    InvalidHeader(&'a [u8]),
    UnexpectedEOL(&'a [u8]),
}

pub trait ServerStream where Self: Clone + Send {
    fn push(&self, server: Server);
}

type TaskQueue = Vec<JoinHandle<std::io::Result<()>>>;

fn parse_iter<'a>(queue: &mut TaskQueue, servers: &impl ServerStream, buf: &'a [u8]) -> Result<ParseStatus, ParseError<'a>> {
    match buf {
        &[0, 0, 0, 0, 0, 0] => Ok(ParseStatus::Stop),
        c @ &[0, 0, 0, 0, 0, 0, ..] => Err(ParseError::UnexpectedEOL(c)),
        &[a, b, c, d, e, f] => { 
            queue.push(spawn(query_server(servers.clone(), (a, b, c, d, e, f).into())));
            Ok(ParseStatus::Continue(Seed((a, b, c, d, e, f).into())))
        },
        &[a, b, c, d, e, f, ..] => { 
            servers.push((a, b, c, d, e, f).into()); 
            parse_iter(servers, &buf[6..]) 
        },
        other => Err(ParseError::UnexpectedEOL(other)),
    }
}

fn parse_response<'a>(queue: &mut , servers: &impl ServerStream, buf: &'a [u8]) -> Result<ParseStatus, ParseError<'a>> {
    // validate header
    let _ = match buf.get(0..6) {
        Some(RESPONSE_HEADER) => Ok(()),
        Some(other) => Err(ParseError::InvalidHeader(other)),
        None => Err(ParseError::InvalidHeader(buf)),
    }?;
    parse_iter(servers, &buf[6..])
}

async fn query_server(servers: impl ServerStream, server: Server) -> std::io::Result<()> {
    timeout(Duration::from_secs(5), async move {
        let (ip, port) = server.clone().into();
        let mut socket = UdpSocket::bind("0.0.0.0:0").await?;
        let _ = socket.send_to(A2S_INFO, (ip, port)).await?;
        let mut buf = [0u8; 2048];
        let (len, socketaddr) = socket.recv_from(&mut buf).await?;
        println!("{:?}", buf.to_vec());
        servers.push(server);
        Ok(())
    }).await?
}

pub async fn query_master(servers: &impl ServerStream) -> std::io::Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let _ = socket.send_to(ALL_SERVERS, "hl2master.steampowered.com:27011").await?;
    let mut buf = [0u8; 2048];
    // using the new sockaddr gets better results from server
    let (mut len, mut sockaddr) = socket.recv_from(&mut buf).await?;
    let mut queue = vec![];
    loop {
        match parse_response(&mut queue, servers, &buf[..len]) {
            Ok(ParseStatus::Continue(seed)) => {
                let _ = socket.send_to(&seed.query(), sockaddr).await?;
                let r = socket.recv_from(&mut buf).await?;
                len = r.0;
                sockaddr = r.1;
            },
            Ok(ParseStatus::Stop) => return Ok(()),
            Err(ParseError::InvalidHeader(header)) => return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("invalid header: {:?}", header))),
            Err(ParseError::UnexpectedEOL(eol)) => return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("invalid eol: {:?}", eol))),
        }
    }
}