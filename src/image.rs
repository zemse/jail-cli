use anyhow::{Context, Result};
use colored::Colorize;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::runtime::Runtime;

pub const IMAGE_NAME: &str = "jail-dev:latest";

/// OCI image used as the base for Tart VMs
pub const TART_OCI_IMAGE: &str = "ghcr.io/cirruslabs/ubuntu:latest";

/// Name of the provisioned Tart base VM (cloned for each jail)
pub const TART_BASE_VM: &str = "jail-dev-base";

const DOCKERFILE: &str = r#"FROM ubuntu:24.04

# Avoid interactive prompts
ENV DEBIAN_FRONTEND=noninteractive

# Install base packages and VSCode Server dependencies
RUN apt-get update && apt-get install -y \
    git \
    build-essential \
    curl \
    wget \
    sudo \
    vim \
    openssh-client \
    ca-certificates \
    # VSCode Server dependencies
    libxkbfile1 \
    libsecret-1-0 \
    libnss3 \
    libatk1.0-0 \
    libatk-bridge2.0-0 \
    libdrm2 \
    libgtk-3-0 \
    libgbm1 \
    libasound2t64 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user with sudo access
RUN useradd -m -s /bin/bash dev && \
    echo "dev ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers

# Switch to dev user for tool installations
USER dev
WORKDIR /home/dev

# Install nvm and Node.js
ENV NVM_DIR=/home/dev/.nvm
RUN curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash && \
    . "$NVM_DIR/nvm.sh" && \
    nvm install --lts && \
    nvm use --lts

# Install Rust via rustup
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/home/dev/.cargo/bin:${PATH}"

# Install Python3 (already in ubuntu, just ensure pip)
USER root
RUN apt-get update && apt-get install -y python3-pip python3-venv && rm -rf /var/lib/apt/lists/*
USER dev

# Install claude-code globally via npm
RUN . "$NVM_DIR/nvm.sh" && npm install -g @anthropic-ai/claude-code

# Setup bash profile to load nvm
RUN echo 'export NVM_DIR="$HOME/.nvm"' >> ~/.bashrc && \
    echo '[ -s "$NVM_DIR/nvm.sh" ] && \. "$NVM_DIR/nvm.sh"' >> ~/.bashrc && \
    echo '[ -s "$NVM_DIR/bash_completion" ] && \. "$NVM_DIR/bash_completion"' >> ~/.bashrc

# Set working directory
WORKDIR /workspace

# Default command
CMD ["/bin/bash"]
"#;

/// Check if the jail-dev image exists
pub fn exists(runtime: Runtime) -> Result<bool> {
    let output = Command::new(runtime.command())
        .args(["image", "inspect", IMAGE_NAME])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("Failed to check for image")?;

    Ok(output.success())
}

/// Build the jail-dev image
pub fn build(runtime: Runtime) -> Result<()> {
    println!(
        "{} Building {} image (one-time setup, may take a few minutes)...",
        "→".blue().bold(),
        IMAGE_NAME.cyan()
    );
    println!("  This only happens once. Future jails will start instantly.");

    let mut child = Command::new(runtime.command())
        .args(["build", "-t", IMAGE_NAME, "-f", "-", "."])
        .stdin(Stdio::piped())
        .spawn()
        .context("Failed to start image build")?;

    // Write Dockerfile to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(DOCKERFILE.as_bytes())
            .context("Failed to write Dockerfile")?;
    }

    let status = child.wait().context("Failed to wait for build")?;

    if !status.success() {
        anyhow::bail!("Image build failed");
    }

    println!(
        "{} Image {} built successfully",
        "✓".green().bold(),
        IMAGE_NAME.cyan()
    );

    Ok(())
}

/// Ensure the jail-dev image exists, building if necessary
pub fn ensure(runtime: Runtime) -> Result<()> {
    match runtime {
        Runtime::Tart => tart_ensure(),
        _ => {
            if !exists(runtime)? {
                build(runtime)?;
            }
            Ok(())
        }
    }
}

/// Shell script to provision a Tart VM with dev tools (mirrors Dockerfile)
const TART_PROVISION_SCRIPT: &str = r#"#!/bin/bash
set -e

export DEBIAN_FRONTEND=noninteractive

# Install base packages
sudo apt-get update && sudo apt-get install -y \
    git build-essential curl wget sudo vim openssh-client ca-certificates \
    libxkbfile1 libsecret-1-0 libnss3 libatk1.0-0 libatk-bridge2.0-0 \
    libdrm2 libgtk-3-0 libgbm1 libasound2t64 python3-pip python3-venv

# Create dev user with sudo
if ! id dev &>/dev/null; then
    sudo useradd -m -s /bin/bash dev
    echo "dev ALL=(ALL) NOPASSWD:ALL" | sudo tee -a /etc/sudoers
fi

# Enable SSH login for dev user (copy authorized keys from admin)
sudo mkdir -p /home/dev/.ssh
sudo cp ~/.ssh/authorized_keys /home/dev/.ssh/ 2>/dev/null || true
sudo chown -R dev:dev /home/dev/.ssh
sudo chmod 700 /home/dev/.ssh

# Install nvm and Node.js for dev user
sudo -u dev bash -c 'curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash && \
    export NVM_DIR="$HOME/.nvm" && . "$NVM_DIR/nvm.sh" && nvm install --lts'

# Install Rust for dev user
sudo -u dev bash -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y'

# Install claude-code globally via npm for dev user
sudo -u dev bash -c 'export NVM_DIR="$HOME/.nvm" && . "$NVM_DIR/nvm.sh" && npm install -g @anthropic-ai/claude-code'

# Setup bash profile for dev user
sudo -u dev bash -c 'cat >> ~/.bashrc << "PROFILE"
export NVM_DIR="$HOME/.nvm"
[ -s "$NVM_DIR/nvm.sh" ] && \. "$NVM_DIR/nvm.sh"
[ -s "$NVM_DIR/bash_completion" ] && \. "$NVM_DIR/bash_completion"
PROFILE'

echo "Provisioning complete!"
"#;

/// Check if the Tart base VM exists
pub fn tart_base_exists() -> Result<bool> {
    let output = Command::new("tart")
        .args(["get", TART_BASE_VM])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("Failed to check for Tart base VM")?;

    Ok(output.success())
}

/// Wait for a Tart VM to get an IP address (with timeout)
pub fn tart_wait_for_ip(vm_name: &str, timeout_secs: u64) -> Result<String> {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);

    loop {
        if start.elapsed() > timeout {
            anyhow::bail!(
                "Timed out waiting for VM '{}' to get an IP address",
                vm_name
            );
        }

        let output = Command::new("tart")
            .args(["ip", vm_name])
            .output()
            .context("Failed to get VM IP")?;

        if output.status.success() {
            let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !ip.is_empty() {
                return Ok(ip);
            }
        }

        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

/// Build the Tart base VM by cloning an OCI image and provisioning it
fn tart_build() -> Result<()> {
    println!(
        "{} Building Tart base VM '{}' (one-time setup, may take several minutes)...",
        "→".blue().bold(),
        TART_BASE_VM.cyan()
    );
    println!("  Pulling base image: {}", TART_OCI_IMAGE);

    // Clone from OCI image
    let status = Command::new("tart")
        .args(["clone", TART_OCI_IMAGE, TART_BASE_VM])
        .status()
        .context("Failed to clone Tart base image")?;

    if !status.success() {
        anyhow::bail!("Failed to clone Tart base image '{}'", TART_OCI_IMAGE);
    }

    println!("{} Provisioning VM with dev tools...", "→".blue().bold());

    // Start VM in background for provisioning
    let mut vm_process = Command::new("tart")
        .args(["run", "--no-graphics", TART_BASE_VM])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to start Tart VM for provisioning")?;

    // Wait for VM to get an IP
    let ip = match tart_wait_for_ip(TART_BASE_VM, 120) {
        Ok(ip) => ip,
        Err(e) => {
            let _ = Command::new("tart").args(["stop", TART_BASE_VM]).status();
            let _ = vm_process.wait();
            return Err(e);
        }
    };

    println!("  VM IP: {}", ip);

    // Run provisioning script via SSH (admin user with default Cirrus Labs config)
    let ssh_result = Command::new("ssh")
        .args([
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "ConnectTimeout=10",
            &format!("admin@{}", ip),
            "bash -s",
        ])
        .stdin(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(TART_PROVISION_SCRIPT.as_bytes())?;
            }
            child.wait()
        });

    // Stop the VM
    let _ = Command::new("tart").args(["stop", TART_BASE_VM]).status();
    let _ = vm_process.wait();

    match ssh_result {
        Ok(status) if status.success() => {
            println!(
                "{} Tart base VM '{}' built successfully",
                "✓".green().bold(),
                TART_BASE_VM.cyan()
            );
            Ok(())
        }
        Ok(_) => {
            // Clean up on failure
            let _ = Command::new("tart").args(["delete", TART_BASE_VM]).status();
            anyhow::bail!("Provisioning script failed")
        }
        Err(e) => {
            let _ = Command::new("tart").args(["delete", TART_BASE_VM]).status();
            anyhow::bail!("Failed to run provisioning script: {}", e)
        }
    }
}

/// Ensure the Tart base VM exists, building if necessary
fn tart_ensure() -> Result<()> {
    if !tart_base_exists()? {
        tart_build()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_name() {
        assert_eq!(IMAGE_NAME, "jail-dev:latest");
    }

    #[test]
    fn test_dockerfile_not_empty() {
        assert!(!DOCKERFILE.is_empty());
        assert!(DOCKERFILE.contains("ubuntu:24.04"));
        assert!(DOCKERFILE.contains("dev"));
    }

    #[test]
    fn test_tart_constants() {
        assert_eq!(TART_BASE_VM, "jail-dev-base");
        assert!(TART_OCI_IMAGE.contains("ubuntu"));
    }

    #[test]
    fn test_tart_provision_script_not_empty() {
        assert!(!TART_PROVISION_SCRIPT.is_empty());
        assert!(TART_PROVISION_SCRIPT.contains("git"));
        assert!(TART_PROVISION_SCRIPT.contains("nvm"));
        assert!(TART_PROVISION_SCRIPT.contains("rustup"));
        assert!(TART_PROVISION_SCRIPT.contains("claude-code"));
    }
}
