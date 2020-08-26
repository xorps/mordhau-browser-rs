use futures::stream::{self, Stream, TryStreamExt, StreamExt, FuturesUnordered, TryStream};
use tokio::net::{UdpSocket, ToSocketAddrs};
use tokio::task::{spawn, JoinHandle};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use futures::future::join_all;
use crate::server::{self, ServerInfo};
use crate::send_server;

type IO<T> = std::io::Result<T>;

const MASTER_SERVER: &str = "hl2master.steampowered.com:27011";
const EOL: &[u8] = &[0, 0, 0, 0, 0, 0];

fn packet(addr: SocketAddrV4) -> Vec<u8> {
    let mut query = vec![];
    query.extend_from_slice(b"\x31\xFF");
    query.extend(format!("{}", addr).as_bytes());
    query.extend_from_slice(b"\x00\\appid\\629760\x00");
    query
}

fn bytes_to_addr(buf: &[u8]) -> SocketAddrV4 {
    let ip = Ipv4Addr::new(buf[0], buf[1], buf[2], buf[3]);
    let port = u16::from_be_bytes([buf[4], buf[5]]);
    SocketAddrV4::new(ip, port)
}

struct QueryParam<A: ToSocketAddrs> {
    socket: UdpSocket,
    seed: SocketAddrV4,
    master: A,
}

enum QueryResult {
    Continue { socket: UdpSocket, seed: SocketAddrV4, master: SocketAddr, data: Vec<SocketAddrV4> },
    Finished { data: Vec<SocketAddrV4> },
}

async fn query<A: ToSocketAddrs>(param: QueryParam<A>) -> IO<QueryResult> {
    let QueryParam {mut socket, seed, master} = param;
    let _ = socket.send_to(&packet(seed), master).await?;
    let mut buf = [0u8; 2048];
    let (len, master) = socket.recv_from(&mut buf).await?;
    let seed = &buf[len-6..len];
    let stop = seed == EOL;
    let data = if stop { &buf[6..len-6] } else { &buf[6..] };
    let data = data.chunks_exact(6).map(bytes_to_addr).collect();
    let seed = bytes_to_addr(seed);
    Ok(if stop { QueryResult::Finished {data} }
       else    { QueryResult::Continue {socket, seed, master, data} })
}

enum QueryState {
    New,
    Loop { socket: UdpSocket, seed: SocketAddrV4, master: SocketAddr },
    Done,
}

async fn query_machine(state: QueryState) -> IO<Option<(Vec<SocketAddrV4>, QueryState)>> {
    match state {
        QueryState::New => {
            let socket = UdpSocket::bind("0.0.0.0:0").await?;
            let seed = SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0);
            let master = MASTER_SERVER;
            match query(QueryParam {socket, seed, master}).await? {
                QueryResult::Continue {socket, seed, master, data} => Ok(Some((data, QueryState::Loop {socket, seed, master}))),
                QueryResult::Finished {data} => Ok(Some((data, QueryState::Done))),
            }
        },
        QueryState::Loop {socket, seed, master} => {
            match query(QueryParam {socket, seed, master}).await? {
                QueryResult::Continue {socket, seed, master, data} => Ok(Some((data, QueryState::Loop {socket, seed, master}))),
                QueryResult::Finished {data} => Ok(Some((data, QueryState::Done))),
            }
        },
        QueryState::Done => Ok(None),
    }
}

fn master_stream() -> impl Stream<Item = std::io::Result<Vec<SocketAddrV4>>> {
    stream::try_unfold(QueryState::New, query_machine)
}

pub async fn query_master(sink: druid::ExtEventSink) -> std::io::Result<()> {
    let mut master = master_stream().boxed();
    let mut tasks = Vec::new();
    while let Some(addr) = master.try_next().await? {
        for ip in addr.into_iter() {
            let sink = sink.clone();
            let task = spawn(async move {
                match server::query(ip).await {
                    Ok(info) => { let _ = send_server(sink, info).await; () },
                    Err(_) => (),
                }
            });
            tasks.push(task);
        }
    }
    join_all(tasks).await;
    Ok(())
}
