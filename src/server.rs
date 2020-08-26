use nom::{IResult, bytes::complete::{tag, take_until}};
use std::net::{SocketAddrV4};
use std::time::Duration;
use crate::timed_udp::TimedUdpSocket;

const A2S_INFO: &[u8] = b"\xFF\xFF\xFF\xFF\x54Source Engine Query\x00";

pub struct ServerInfo {
    pub name: String,
}

fn take_cstr(input: &[u8]) -> IResult<&[u8], &[u8]> {
    const NUL: &[u8] = &[0];
    let (input, string) = take_until(NUL)(input)?;
    let (input, _) = tag(NUL)(input)?;
    Ok((input, string))
}

fn byte_tag(input: &[u8], pattern: u8) -> IResult<&[u8], u8> {
    use nom::bits::complete::tag;
    use nom::bits::bits;

    bits(tag::<_, _, _, (_, _)>(pattern, 8usize))(input)
}

fn take_byte(input: &[u8]) -> IResult<&[u8], u8> {
    use nom::bits::complete::take;
    use nom::bits::bits;

    bits(take::<_, _, _, (_, _)>(8usize))(input)
}

fn take_short(input: &[u8]) -> IResult<&[u8], u16> {
    use nom::bits::complete::take;
    use nom::bits::bits;

    bits(take::<_, _, _, (_, _)>(16usize))(input)
}

fn parse_info(input: &[u8]) -> IResult<&[u8], ServerInfo> {
    let (input, _header_long) = tag(b"\xFF\xFF\xFF\xFF")(input)?;
    let (input, _header) = byte_tag(input, 0x49)?;
    let (input, _protocol) = take_byte(input)?;
    let (input, name) = take_cstr(input)?;
    let (input, _map) = take_cstr(input)?;
    let (input, _folder) = take_cstr(input)?;
    let (input, _game) = take_cstr(input)?;
    let (input, _app_id) = take_short(input)?;
    let (input, _num_players) = take_byte(input)?;
    let (input, _max_players) = take_byte(input)?;
    let (input, _num_bots) = take_byte(input)?;
    let (input, _server_type) = take_byte(input)?;
    let (input, _env) = take_byte(input)?;
    let (input, _vis) = take_byte(input)?;
    let (input, _vac) = take_byte(input)?;

    let name = String::from_utf8_lossy(name).into();

    Ok((input, ServerInfo {name}))
}

pub async fn query(addr: SocketAddrV4) -> std::io::Result<ServerInfo> {
    let mut socket = TimedUdpSocket::new(Duration::from_secs(5)).await?;
    let mut buf = [0u8; 2048];
    let _ = socket.send_to(A2S_INFO, addr).await?;
    let (len, _) = socket.recv_from(&mut buf).await?;
    let buf = &buf[0..len];
    let (_, info) = parse_info(&buf).unwrap();
    Ok(info)
}