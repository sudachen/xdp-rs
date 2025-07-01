use std::env;
use std::process::Stdio;
use std::io::{Result,Error};

pub fn execute_sudo_command(command: &str) -> Result<()> {
    use std::process::Command;
    let output = Command::new("sudo")
        .arg("sh")
        .arg("-c")
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    if !output.status.success() {
        return Err(Error::other(
            format!(
                "Command failed with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    } else {
        log::info!("sudo# {}", command);
        //log::debug!("Output: {}", String::from_utf8_lossy(&output.stdout));
    }
    Ok(())
}

pub fn restart_with_caps(my_caps: &[caps::Capability]) -> Result<()>{
    let perm_caps = caps::read(None,caps::CapSet::Permitted).map_err(|e| Error::other(e.to_string()))?;
    log::info!("Permitted Caps: {:?}", perm_caps);
    let effect_caps = caps::read(None,caps::CapSet::Effective).map_err(|e| Error::other(e.to_string()))?;
    log::info!("Effective Caps: {:?}", effect_caps);
    match env::var("RESTARTED") {
        Err(_) => {}
        Ok(val) => {
            if val == "1" {
                log::info!("Already restarted with capabilities.");
                return Ok(());
            }
        }
    }
    unsafe { env::set_var("RESTARTED", "1"); }
    if my_caps.iter().any(|c| !perm_caps.contains(c)) {
        let caps_string = my_caps.iter()
            .map(|cap| cap.to_string())
            .collect::<Vec<String>>()
            .join(",");
        let current_prog = env::current_exe()?;
        let current_prog_path = current_prog.as_path().to_str().ok_or_else(|| Error::other("Failed to get current executable path"))?;
        execute_sudo_command(&format!("setcap {caps_string}+eip {}",current_prog_path))?;
        let args: Vec<String> = env::args().collect();
        log::debug!("Re-executing: {:?}", args);
        Err(Error::other(exec::execvp(&current_prog, &args).to_string()))
    } else {
        Ok(())
    }
}

pub fn setup(my_caps: &[caps::Capability]) -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();
    if !my_caps.is_empty() {
        restart_with_caps(my_caps)
    } else {
        Ok(())
    }
}
