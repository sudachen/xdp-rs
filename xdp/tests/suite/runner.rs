use crate::suite::veth;
use std::io::{Result, ErrorKind};
use std::future::Future;

pub const DEV_PREFIX: &str = "xdpVeth";
pub const IP_PREFIX: &str = "192.168.77.";

pub struct Host {
    pub if_dev: String,
    pub ip: String,
}

impl Host {
    pub fn new(if_dev: String, ip: String) -> Self {
        Host {
            if_dev,
            ip,
        }
    }
}

pub struct HostPair {
    pub host0: Host,
    pub host1: Host,
}

impl HostPair {
    pub fn new(host0: Host, host1: Host) -> Self {
        HostPair {
            host0,
            host1,
        }
    }
    
    pub fn from_prefixes(dev_prefix: &str, ip_prefix: &str) -> Self {
        let host0 = Host::new(format!("{}0", dev_prefix), format!("{}100", ip_prefix));
        let host1 = Host::new(format!("{}1", dev_prefix), format!("{}101", ip_prefix));
        HostPair::new(host0, host1)
    }
}

pub async fn run_test_with_pair<F,Fut>(test: F) -> Result<()>
where 
    F: FnOnce(HostPair) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    if let Err(e) = veth::check_pair(DEV_PREFIX) {
        if e.kind() == ErrorKind::NotFound {
            veth::setup_pair(DEV_PREFIX,IP_PREFIX)?;
        } else {
            return Err(e);
        }
    }
    let host_pair = HostPair::from_prefixes(DEV_PREFIX, IP_PREFIX);
    test(host_pair).await?;
    veth::teardown_pair(DEV_PREFIX)?;
    Ok(())
}
