
use std::io::{Result,Error,ErrorKind};
use crate::suite::command::execute_sudo_command;

pub fn setup_pair(dev_prefix: &str, ip_prefix: &str) -> Result<()> {
    log::info!("creating new veth pair {0}0 + {0}1", dev_prefix);
    execute_sudo_command(&format!("ip link add {0}0 type veth peer {0}1",dev_prefix))?;
    up_pair(dev_prefix, ip_prefix)?;
    Ok(())
}

pub fn teardown_pair(prefix: &str) -> Result<()> {
    log::info!("tearing down veth pair {0}0", prefix);
    execute_sudo_command(&format!("ip link del {0}0",prefix))?;
    Ok(())
}

pub fn check_pair(prefix: &str) -> Result<()> {

    log::info!("checking for veth pair {0}0 + {0}1", prefix);
    let output = std::process::Command::new("ip")
        .arg("link")
        .arg("show")
        .arg(format!("{}0", prefix))
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Error::new(
            ErrorKind::NotFound,
            format!("Pair {}0 does not exist", prefix),
        ))
    }
}

pub fn up_if_dev(dev: &str) -> Result<()> {
    log::info!("setting interface {0} up", dev);
    execute_sudo_command(&format!("ip link set {0} up",dev))?;
    Ok(())
}

pub fn up_pair(dev_prefix: &str, ip_prefix: &str) -> Result<()> {
    let dev= format!("{}0", dev_prefix); 
    set_ipv4_addr(&dev,&format!("{}100", ip_prefix))?;
    up_if_dev(&dev)?;
    let dev= format!("{}1", dev_prefix);
    set_ipv4_addr(&dev,&format!("{}101", ip_prefix))?;
    up_if_dev(&dev)?;
    Ok(())
}

pub fn set_ipv4_addr(dev: &str, addr: &str) -> Result<()> {
    log::info!("setting IPv4 address {0} on {1}", addr, dev);
    execute_sudo_command(&format!("ip addr add {0} dev {1}", addr, dev))?;
    Ok(())
}

