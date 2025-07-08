use std::net::SocketAddrV4;
use std::{io,time};
use clap::Parser;
use etherparse::PacketBuilder;
use xdp_util::{get_ipv4_address, mac_by_ifindex, Router};
use xdp_tests::xdp;

#[derive(Parser, Debug)]
#[command(arg_required_else_help = true)]
struct Args {
    /// source ip:port
    src: String,

    /// destination ip:port
    dst: String,

    /// payload
    #[clap(short,long)]
    text: String,

    /// delay between packets like 1s or 100ms
    #[clap(short, long)]
    delay: Option<String>,

    /// Number of times to send the packet
    #[clap(short, long, default_value_t = 1)]
    count: u8,
}

pub fn main() -> io::Result<()> {

    xdp_tests::nettest::suite::command::setup(&[
        caps::Capability::CAP_NET_ADMIN,
        caps::Capability::CAP_NET_RAW,
        caps::Capability::CAP_BPF,
    ])?;

    let args = Args::parse();

    let delay = args.delay.as_ref().map_or(Ok(time::Duration::from_secs(1)), |x| humantime::parse_duration(x)
        .map_err(|_| io::Error::other("Invalid delay format")))?;

    let src: SocketAddrV4 = args.src.parse()
        .map_err(|_|io::Error::other("Invalid source address format"))?;
    let dst: SocketAddrV4 = args.dst.parse()
        .map_err(|_|io::Error::other("Invalid source address format"))?;

    let if_index = get_ipv4_address(None)?
        .iter()
        .find(|(addr, _)| addr == src.ip())
        .ok_or_else(|| io::Error::other(format!("Source IP {} not found", src.ip())))?
        .1;

    let _owned_xdp_host1 =
        xdp::attach_pass_program(if_index).map_err(|e| {
            log::error!("Failed to attach XDP pass program on {}: {}", if_index, e);
            e
        })?;

    let mut router = Router::new(if_index);
    router.refresh()?;

    let next_hop = router.route(dst.ip()).ok_or_else(||
        io::Error::other(format!("No route to destination IP {}", dst.ip())))?;

    let mut sok = xdp_socket::create_tx_socket(if_index,0, None)
        .map_err(|e| io::Error::other(format!("Failed to create XDP socket: {}", e)))?;

    let bytes = args.text.as_bytes();
    let src_mac =  mac_by_ifindex(if_index)?;

    for _ in 0 .. args.count {

        let mut bf = sok.seek_and_peek(42 + bytes.len()).map_err(|e|
            io::Error::other(format!("Failed to seek and peek: {}", e)))?;

        PacketBuilder::ethernet2(src_mac, next_hop.mac_addr.unwrap())
            .ipv4(src.ip().octets(), dst.ip().octets(), 64) // 64 is a common TTL
            .udp(src.port(), dst.port())
            .write(&mut bf, bytes)
            .map_err(|e| io::Error::other(format!("Error writing packet header: {}", e)))?;

        sok.commit().map_err(|e| io::Error::other( format!("Failed to commit buffer in RX ring: {e}")))?;
        sok.kick()?;

        println!("Sent packet to {}:{}", dst.ip(), dst.port());

        std::thread::sleep(delay);
    }

    Ok(())
}