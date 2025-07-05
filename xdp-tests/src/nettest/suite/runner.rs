use std::fmt::{Debug, Display};
use crate::nettest::suite::vethpair;
use std::future::Future;
use std::io::{Error, ErrorKind, Result};
use std::net::Ipv4Addr;
use std::str::FromStr;
use xdp_socket::util::get_ipv4_address;

pub const DEV_PREFIX: &str = "xdpVeth";
pub const IP_PREFIX: &str = "192.168.77.";

#[derive(Clone,Debug)]
pub struct Host {
    pub if_dev: String,
    pub ip_str: String,
    pub ip_addr: Ipv4Addr,
    pub if_index: u32,
}

impl Default for Host {
    fn default() -> Self {
        Host {
            if_dev: String::new(),
            ip_str: String::new(),
            ip_addr: Ipv4Addr::new(0, 0, 0, 0),
            if_index: 0,
        }
    }
}

impl Display for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Host {{ ip: {}, if_dev: {}, if_index: {} }}",
            self.ip_str, self.if_dev, self.if_index
        )
    }
}

impl Host {
    pub fn new(if_dev: String, ip_str: String) -> Self {
        let ip_addr = Ipv4Addr::from_str(&ip_str).expect("Invalid IP address format");
        let if_index = get_ipv4_address(None).unwrap()
            .iter()
            .find(|(addr, _)| *addr == ip_addr)
            .ok_or_else(|| Error::other(format!("Source IP {} not found", ip_addr))).unwrap()
            .1;
        Host { if_dev, ip_str, ip_addr, if_index }
    }
}

pub struct HostPair {
    pub host0: Host,
    pub host1: Host,
}

impl HostPair {
    pub fn new(host0: Host, host1: Host) -> Self {
        HostPair { host0, host1 }
    }

    pub fn from_prefixes(dev_prefix: &str, ip_prefix: &str) -> Self {
        let host0 = Host::new(format!("{}0", dev_prefix), format!("{}100", ip_prefix));
        let host1 = Host::new(format!("{}1", dev_prefix), format!("{}101", ip_prefix));
        HostPair::new(host0, host1)
    }
}

pub async fn run_test_with_pair<F, Fut>(test: F) -> Result<()>
where
    F: FnOnce(HostPair) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    if let Err(e) = vethpair::check_pair(DEV_PREFIX) {
        if e.kind() == ErrorKind::NotFound {
            vethpair::setup_pair(DEV_PREFIX, IP_PREFIX)?;
        } else {
            return Err(e);
        }
    }
    let host_pair = HostPair::from_prefixes(DEV_PREFIX, IP_PREFIX);
    test(host_pair).await?;
    vethpair::teardown_pair(DEV_PREFIX)?;
    Ok(())
}
