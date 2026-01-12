use anyhow::{Context, Result};
use colored::Colorize;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::runtime::Runtime;

pub const IMAGE_NAME: &str = "jail-dev:latest";

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
        "{} Building {} image (this may take a few minutes)...",
        "→".blue().bold(),
        IMAGE_NAME.cyan()
    );

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
    if !exists(runtime)? {
        build(runtime)?;
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
}
