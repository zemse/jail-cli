use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    Podman,
    Docker,
    Tart,
}

impl Runtime {
    pub fn command(&self) -> &'static str {
        match self {
            Runtime::Podman => "podman",
            Runtime::Docker => "docker",
            Runtime::Tart => "tart",
        }
    }

    /// Whether this runtime uses containers (Docker/Podman) vs VMs (Tart)
    pub fn is_container_runtime(&self) -> bool {
        matches!(self, Runtime::Podman | Runtime::Docker)
    }

    /// Check if this runtime is available and working
    pub fn is_available(&self) -> bool {
        let cmd = self.command();
        if which::which(cmd).is_err() {
            return false;
        }

        match self {
            Runtime::Tart => {
                // Tart is macOS-only (Apple Silicon)
                if !cfg!(target_os = "macos") {
                    return false;
                }
                // Quick check: tart list should work if tart is installed
                Command::new(cmd)
                    .args(["list"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            }
            _ => Command::new(cmd)
                .args(["info"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false),
        }
    }

    /// Get SSH agent socket mount arguments for this runtime
    pub fn ssh_agent_mount(&self) -> Option<Vec<String>> {
        match self {
            Runtime::Tart => {
                // Tart VMs are accessed via SSH, which handles agent forwarding natively
                None
            }
            Runtime::Docker => {
                // Docker Desktop on macOS uses a special path
                if cfg!(target_os = "macos") {
                    Some(vec![
                        "-v".to_string(),
                        "/run/host-services/ssh-auth.sock:/run/ssh.sock:ro".to_string(),
                        "-e".to_string(),
                        "SSH_AUTH_SOCK=/run/ssh.sock".to_string(),
                    ])
                } else if let Ok(sock) = std::env::var("SSH_AUTH_SOCK") {
                    Some(vec![
                        "-v".to_string(),
                        format!("{}:/run/ssh.sock:ro", sock),
                        "-e".to_string(),
                        "SSH_AUTH_SOCK=/run/ssh.sock".to_string(),
                    ])
                } else {
                    None
                }
            }
            Runtime::Podman => {
                // On macOS, Podman runs in a VM and can't directly mount host Unix sockets
                // SSH agent forwarding requires special Podman machine configuration
                if cfg!(target_os = "macos") {
                    None
                } else if let Ok(sock) = std::env::var("SSH_AUTH_SOCK") {
                    // On Linux, Podman can mount the SSH socket directly
                    Some(vec![
                        "-v".to_string(),
                        format!("{}:/run/ssh.sock:ro", sock),
                        "-e".to_string(),
                        "SSH_AUTH_SOCK=/run/ssh.sock".to_string(),
                    ])
                } else {
                    None
                }
            }
        }
    }
}

impl std::fmt::Display for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.command())
    }
}

/// Get platform-specific installation instructions
fn install_instructions() -> &'static str {
    match std::env::consts::OS {
        "macos" => {
            "Install a container or VM runtime:\n\n\
             Podman (recommended):\n  \
             brew install podman\n  \
             podman machine init\n  \
             podman machine start\n\n\
             Docker Desktop:\n  \
             brew install --cask docker\n  \
             # Then launch Docker.app\n\n\
             Tart (macOS VMs on Apple Silicon):\n  \
             brew install cirruslabs/cli/tart"
        }
        "linux" => {
            "Install a container runtime:\n\n\
             Podman (recommended):\n  \
             sudo apt install podman      # Ubuntu/Debian\n  \
             sudo dnf install podman      # Fedora\n  \
             sudo pacman -S podman        # Arch\n\n\
             Docker:\n  \
             See https://docs.docker.com/engine/install/"
        }
        _ => "Please install Docker, Podman, or Tart for your platform.",
    }
}

/// Detect the best available runtime, preferring Podman
pub fn detect() -> Result<Runtime> {
    // Check for config override first
    if let Some(runtime) = crate::config::get_runtime_override()? {
        if runtime.is_available() {
            return Ok(runtime);
        }
        bail!(
            "Configured runtime '{}' is not available or not working",
            runtime
        );
    }

    // Prefer container runtimes (Podman > Docker), fall back to Tart
    if Runtime::Podman.is_available() {
        return Ok(Runtime::Podman);
    }

    if Runtime::Docker.is_available() {
        return Ok(Runtime::Docker);
    }

    if Runtime::Tart.is_available() {
        return Ok(Runtime::Tart);
    }

    bail!("No container runtime found.\n\n{}", install_instructions())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_command() {
        assert_eq!(Runtime::Docker.command(), "docker");
        assert_eq!(Runtime::Podman.command(), "podman");
        assert_eq!(Runtime::Tart.command(), "tart");
    }

    #[test]
    fn test_is_container_runtime() {
        assert!(Runtime::Docker.is_container_runtime());
        assert!(Runtime::Podman.is_container_runtime());
        assert!(!Runtime::Tart.is_container_runtime());
    }
}
