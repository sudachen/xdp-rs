use crate::nettest::suite::command::execute_sudo_command;
use std::io::{Error, ErrorKind, Result};

pub fn setup_pair(dev_prefix: &str, ip_prefix: &str) -> Result<()> {
    log::info!("creating new veth pair {dev_prefix}0 + {dev_prefix}1");
    execute_sudo_command(&format!("ip link add {dev_prefix}0 type veth peer {dev_prefix}1"))?;
    up_pair(dev_prefix, ip_prefix)?;
    Ok(())
}

pub fn teardown_pair(prefix: &str) -> Result<()> {
    log::info!("tearing down veth pair {prefix}0");
    execute_sudo_command(&format!("ip link del {prefix}0"))?;
    Ok(())
}

pub fn check_pair(prefix: &str) -> Result<()> {
    log::info!("checking for veth pair {prefix}0 + {prefix}1");
    let output = std::process::Command::new("ip")
        .arg("link")
        .arg("show")
        .arg(format!("{prefix}0"))
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Error::new(
            ErrorKind::NotFound,
            format!("Pair {prefix}0 does not exist"),
        ))
    }
}

pub fn up_if_dev(dev: &str) -> Result<()> {
    log::info!("setting interface {dev} up");
    execute_sudo_command(&format!("ip link set {dev} up"))?;
    Ok(())
}

pub fn up_pair(dev_prefix: &str, ip_prefix: &str) -> Result<()> {
    let dev = format!("{dev_prefix}0");
    set_ipv4_addr(&dev, &format!("{ip_prefix}100"))?;
    set_promisc_mode(&dev, true)?;
    up_if_dev(&dev)?;
    let dev = format!("{dev_prefix}1");
    set_ipv4_addr(&dev, &format!("{ip_prefix}101"))?;
    set_promisc_mode(&dev, true)?;
    up_if_dev(&dev)?;
    Ok(())
}

pub fn set_ipv4_addr(dev: &str, addr: &str) -> Result<()> {
    log::info!("setting IPv4 address {addr} on {dev}");
    execute_sudo_command(&format!("ip addr add {addr}/24 dev {dev}"))?;
    Ok(())
}

pub fn set_promisc_mode(dev: &str, enable: bool) -> Result<()> {
    log::info!(
        "setting promisc mode {0} on {dev}",
        if enable { "on" } else { "off" }
    );
    let mode = if enable { "on" } else { "off" };
    execute_sudo_command(&format!("ip link set {dev} promisc {mode}"))?;
    Ok(())
}
