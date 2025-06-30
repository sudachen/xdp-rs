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
        log::info!("Command succeeded: {}", command);
        log::debug!("Output: {}", String::from_utf8_lossy(&output.stdout));
    }
    Ok(())
}