use anyhow::{bail, Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Select};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

use crate::config::jails_dir;
use crate::image::{self, IMAGE_NAME};
use crate::runtime::{self, Runtime};

#[derive(Debug, Serialize, Deserialize)]
pub struct JailMetadata {
    /// Source URL or path that was cloned
    pub source: String,
    /// Container ID (if running)
    pub container_id: Option<String>,
    /// Runtime used to create this jail
    pub runtime: Runtime,
    /// Creation timestamp
    pub created_at: String,
    /// Ports to expose (for macOS)
    #[serde(default)]
    pub ports: Vec<u16>,
    /// Workspace directory name (defaults to "workspace" for backward compatibility)
    #[serde(default = "default_workspace_dir")]
    pub workspace_dir: String,
}

fn default_workspace_dir() -> String {
    "workspace".to_string()
}

impl JailMetadata {
    fn new(source: &str, runtime: Runtime, ports: Vec<u16>, workspace_dir: String) -> Self {
        Self {
            source: source.to_string(),
            container_id: None,
            runtime,
            created_at: chrono_now(),
            ports,
            workspace_dir,
        }
    }

    fn load(jail_path: &PathBuf) -> Result<Self> {
        let meta_path = jail_path.join("jail.toml");
        let content = std::fs::read_to_string(&meta_path)
            .with_context(|| format!("Failed to read jail metadata: {}", meta_path.display()))?;
        toml::from_str(&content).context("Failed to parse jail metadata")
    }

    fn save(&self, jail_path: &PathBuf) -> Result<()> {
        let meta_path = jail_path.join("jail.toml");
        let content = toml::to_string_pretty(self).context("Failed to serialize jail metadata")?;
        std::fs::write(&meta_path, content)
            .with_context(|| format!("Failed to write jail metadata: {}", meta_path.display()))
    }
}

fn chrono_now() -> String {
    // Simple ISO 8601 timestamp without chrono dependency
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", duration.as_secs())
}

/// Derive a jail name from source
fn derive_name(source: &str) -> String {
    // Handle git URLs
    if source.contains("github.com") || source.contains("gitlab.com") || source.ends_with(".git") {
        // Extract owner/repo from URL
        let cleaned = source.trim_end_matches(".git").trim_end_matches('/');

        // Try to extract the last two path components (owner/repo)
        let parts: Vec<&str> = cleaned.split('/').collect();
        if parts.len() >= 2 {
            let owner = parts[parts.len() - 2];
            let repo = parts[parts.len() - 1];
            // Clean owner in case it has @ prefix (ssh urls)
            let owner = owner.split(':').last().unwrap_or(owner);
            return format!("{}/{}", owner, repo);
        }
    }

    // Handle local paths - use directory name
    let path = std::path::Path::new(source);
    if let Some(name) = path.file_name() {
        return name.to_string_lossy().to_string();
    }

    // Fallback
    source.replace(['/', ':', '@'], "-")
}

/// Sanitize name for use as container name
fn sanitize_container_name(name: &str) -> String {
    name.replace('/', "-").replace([':', '@', ' '], "_")
}

/// Extract repo name from jail name (e.g., "KMPARDS/timeally-react" -> "timeally-react")
fn extract_repo_name(jail_name: &str) -> String {
    jail_name.split('/').last().unwrap_or(jail_name).to_string()
}

/// Get the path to a specific jail
fn jail_path(name: &str) -> Result<PathBuf> {
    Ok(jails_dir()?.join(name.replace('/', "_")))
}

/// Clone a repository into a new jail
pub fn clone(source: &str, name: Option<&str>, ports: Vec<u16>) -> Result<()> {
    let runtime = runtime::detect()?;
    let jail_name = name
        .map(String::from)
        .unwrap_or_else(|| derive_name(source));
    let jail_dir = jail_path(&jail_name)?;

    // Check if jail already exists
    if jail_dir.exists() {
        bail!("Jail '{}' already exists", jail_name);
    }

    println!(
        "{} Creating jail '{}' from {}",
        "→".blue().bold(),
        jail_name.cyan(),
        source
    );

    // Ensure base image exists
    image::ensure(runtime)?;

    // Create jail directory structure using repo name
    let workspace_name = extract_repo_name(&jail_name);
    let workspace_dir = jail_dir.join(&workspace_name);
    std::fs::create_dir_all(&workspace_dir)
        .with_context(|| format!("Failed to create directory: {}", workspace_dir.display()))?;

    // Clone the source
    println!("{} Cloning repository...", "→".blue().bold());

    let clone_status = if std::path::Path::new(source).exists() {
        // Local path - copy
        copy_dir_recursive(source, &workspace_dir)?;
        true
    } else {
        // Git URL - clone
        Command::new("git")
            .args(["clone", source, "."])
            .current_dir(&workspace_dir)
            .status()
            .context("Failed to run git clone")?
            .success()
    };

    if !clone_status {
        // Clean up on failure
        let _ = std::fs::remove_dir_all(&jail_dir);
        bail!("Failed to clone repository");
    }

    // Save metadata
    let metadata = JailMetadata::new(source, runtime, ports, workspace_name);
    metadata.save(&jail_dir)?;

    println!(
        "{} Jail '{}' created successfully",
        "✓".green().bold(),
        jail_name.cyan()
    );

    // Auto-enter the jail
    enter_jail(&jail_name, vec![])
}

/// Create an empty jail
pub fn create(name: &str, ports: Vec<u16>) -> Result<()> {
    let runtime = runtime::detect()?;
    let jail_dir = jail_path(name)?;

    // Check if jail already exists
    if jail_dir.exists() {
        bail!("Jail '{}' already exists", name);
    }

    println!("{} Creating jail '{}'", "→".blue().bold(), name.cyan());

    // Ensure base image exists
    image::ensure(runtime)?;

    // Create jail directory structure using jail name
    let workspace_name = name.to_string();
    let workspace_dir = jail_dir.join(&workspace_name);
    std::fs::create_dir_all(&workspace_dir)
        .with_context(|| format!("Failed to create directory: {}", workspace_dir.display()))?;

    // Save metadata
    let metadata = JailMetadata::new("(empty)", runtime, ports, workspace_name);
    metadata.save(&jail_dir)?;

    println!(
        "{} Jail '{}' created successfully",
        "✓".green().bold(),
        name.cyan()
    );

    // Auto-enter the jail
    enter_jail(name, vec![])
}

/// Copy directory recursively
fn copy_dir_recursive(src: &str, dst: &PathBuf) -> Result<bool> {
    let status = Command::new("cp")
        .args(["-r", &format!("{}/..", src), "."])
        .current_dir(dst)
        .status()
        .context("Failed to copy directory")?;

    // Alternative: copy contents
    if !status.success() {
        let src_path = std::path::Path::new(src);
        for entry in std::fs::read_dir(src_path)? {
            let entry = entry?;
            let dest = dst.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                std::fs::create_dir_all(&dest)?;
                copy_dir_recursive(entry.path().to_str().unwrap(), &dest)?;
            } else {
                std::fs::copy(entry.path(), dest)?;
            }
        }
    }

    Ok(true)
}

/// List all jails
pub fn list() -> Result<()> {
    let jails = jails_dir()?;

    if !jails.exists() {
        println!("No jails found.");
        return Ok(());
    }

    let mut found_any = false;
    for entry in std::fs::read_dir(&jails)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let jail_dir = entry.path();
        let meta_path = jail_dir.join("jail.toml");

        if !meta_path.exists() {
            continue;
        }

        found_any = true;
        let name = entry.file_name().to_string_lossy().replace('_', "/");

        if let Ok(metadata) = JailMetadata::load(&jail_dir) {
            let status = if is_container_running(&name, metadata.runtime)? {
                "running".green()
            } else {
                "stopped".yellow()
            };

            println!(
                "  {} {} [{}]",
                name.cyan(),
                format!("({})", metadata.source).dimmed(),
                status
            );
        } else {
            println!("  {}", name.cyan());
        }
    }

    if !found_any {
        println!("No jails found.");
    }

    Ok(())
}

/// Check if a container is running
fn is_container_running(name: &str, runtime: Runtime) -> Result<bool> {
    let container_name = format!("jail-{}", sanitize_container_name(name));
    let output = Command::new(runtime.command())
        .args(["ps", "-q", "-f", &format!("name={}", container_name)])
        .output()
        .context("Failed to check container status")?;

    Ok(!output.stdout.is_empty())
}

/// Get all jail names
fn get_jail_names() -> Result<Vec<String>> {
    let jails = jails_dir()?;
    let mut names = Vec::new();

    if !jails.exists() {
        return Ok(names);
    }

    for entry in std::fs::read_dir(&jails)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let jail_dir = entry.path();
        let meta_path = jail_dir.join("jail.toml");

        if meta_path.exists() {
            let name = entry.file_name().to_string_lossy().replace('_', "/");
            names.push(name);
        }
    }

    Ok(names)
}

/// Filter jail names by a pattern (matches owner or repo name prefix)
fn filter_jails(names: &[String], filter: &str) -> Vec<String> {
    let filter_lower = filter.to_lowercase();
    names
        .iter()
        .filter(|name| {
            let name_lower = name.to_lowercase();
            // Match if the full name starts with filter
            if name_lower.starts_with(&filter_lower) {
                return true;
            }
            // Match if owner or repo part starts with filter
            if let Some((owner, repo)) = name_lower.split_once('/') {
                return owner.starts_with(&filter_lower) || repo.starts_with(&filter_lower);
            }
            false
        })
        .cloned()
        .collect()
}

/// Select a jail interactively, optionally filtered by a pattern
fn select_jail(filter: Option<&str>) -> Result<String> {
    let all_names = get_jail_names()?;

    if all_names.is_empty() {
        bail!("No jails found. Create one with: jail clone <url>");
    }

    let candidates = match filter {
        Some(f) if !f.is_empty() => {
            let filtered = filter_jails(&all_names, f);
            if filtered.is_empty() {
                bail!("No jails match filter '{}'", f);
            }
            // If exact match exists, return it directly (user typed full name)
            if let Some(exact) = filtered.iter().find(|n| n.eq_ignore_ascii_case(f)) {
                return Ok(exact.clone());
            }
            filtered
        }
        _ => all_names,
    };

    // Interactive selection (always show, even for single item)
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select a jail")
        .items(&candidates)
        .default(0)
        .interact()?;

    Ok(candidates[selection].clone())
}

/// Get or create a container for a jail
fn get_or_create_container(
    name: &str,
    jail_dir: &PathBuf,
    metadata: &JailMetadata,
    force_recreate: bool,
) -> Result<String> {
    let runtime = metadata.runtime;
    let container_name = format!("jail-{}", sanitize_container_name(name));
    let workspace_dir = jail_dir.join(&metadata.workspace_dir);

    // Check if container already exists
    let output = Command::new(runtime.command())
        .args(["ps", "-aq", "-f", &format!("name=^{}$", container_name)])
        .output()
        .context("Failed to check for existing container")?;

    if !output.stdout.is_empty() {
        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();

        if force_recreate {
            // Need to recreate container with new ports - preserve state using docker commit
            println!("{} Updating container with new ports...", "→".blue().bold());

            // Stop container first
            let _ = Command::new(runtime.command())
                .args(["stop", &container_id])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();

            // Commit container to preserve installed packages etc.
            let temp_image = format!("jail-temp-{}", sanitize_container_name(name));
            let commit_output = Command::new(runtime.command())
                .args(["commit", &container_id, &temp_image])
                .output()
                .context("Failed to commit container")?;

            if !commit_output.status.success() {
                bail!(
                    "Failed to preserve container state: {}",
                    String::from_utf8_lossy(&commit_output.stderr)
                );
            }

            // Remove old container
            let _ = Command::new(runtime.command())
                .args(["rm", &container_id])
                .output();

            // Create new container from committed image with new ports
            let new_id =
                create_container(name, &workspace_dir, metadata, runtime, Some(&temp_image))?;

            // Remove temporary image
            let _ = Command::new(runtime.command())
                .args(["rmi", &temp_image])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output();

            return Ok(new_id);
        }

        // Start container if not running
        let running = Command::new(runtime.command())
            .args(["ps", "-q", "-f", &format!("name=^{}$", container_name)])
            .output()?;

        if running.stdout.is_empty() {
            Command::new(runtime.command())
                .args(["start", &container_id])
                .status()
                .context("Failed to start container")?;
        }

        return Ok(container_id);
    }

    // Create new container
    create_container(name, &workspace_dir, metadata, runtime, None)
}

/// Create a new container with the given configuration
fn create_container(
    name: &str,
    workspace_dir: &PathBuf,
    metadata: &JailMetadata,
    runtime: Runtime,
    base_image: Option<&str>,
) -> Result<String> {
    let container_name = format!("jail-{}", sanitize_container_name(name));

    let mut args = vec![
        "run".to_string(),
        "-d".to_string(),
        "-it".to_string(),
        "--name".to_string(),
        container_name.clone(),
    ];

    // Port mapping
    if cfg!(target_os = "macos") {
        // On macOS, use explicit port mapping (--network=host doesn't work in VM)
        for port in &metadata.ports {
            args.push("-p".to_string());
            args.push(format!("{}:{}", port, port));
        }
    } else {
        // On Linux, --network=host works directly
        args.push("--network=host".to_string());
    }

    let container_workdir = format!("/{}", metadata.workspace_dir);
    args.extend([
        "-v".to_string(),
        format!("{}:{}", workspace_dir.display(), container_workdir),
        "-w".to_string(),
        container_workdir,
        "--user".to_string(),
        "dev".to_string(),
    ]);

    // Add SSH agent socket mount
    if let Some(ssh_args) = runtime.ssh_agent_mount() {
        args.extend(ssh_args);
    }

    // Use custom base image if provided (from docker commit), otherwise use default
    args.push(base_image.unwrap_or(IMAGE_NAME).to_string());
    args.push("/bin/bash".to_string());

    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let output = Command::new(runtime.command())
        .args(&args_ref)
        .output()
        .context("Failed to create container")?;

    if !output.status.success() {
        bail!(
            "Failed to create container: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(container_id)
}

/// Enter a jail's shell
pub fn enter(filter: Option<&str>, new_ports: Vec<u16>) -> Result<()> {
    let name = select_jail(filter)?;
    enter_jail(&name, new_ports)
}

/// Internal function to enter a jail by name
fn enter_jail(name: &str, new_ports: Vec<u16>) -> Result<()> {
    let jail_dir = jail_path(name)?;

    if !jail_dir.exists() {
        bail!("Jail '{}' not found", name);
    }

    let mut metadata = JailMetadata::load(&jail_dir)?;

    // Check if we need to add new ports
    let ports_changed = if !new_ports.is_empty() {
        let mut changed = false;
        for port in &new_ports {
            if !metadata.ports.contains(port) {
                metadata.ports.push(*port);
                changed = true;
            }
        }
        if changed {
            metadata.save(&jail_dir)?;
        }
        changed
    } else {
        false
    };

    // Ensure image exists
    image::ensure(metadata.runtime)?;

    let container_id = get_or_create_container(name, &jail_dir, &metadata, ports_changed)?;

    println!("{} Entering jail '{}'...", "→".blue().bold(), name.cyan());
    println!("  Type '{}' to leave the jail", "exit".yellow());

    // Exec into container
    let status = Command::new(metadata.runtime.command())
        .args(["exec", "-it", &container_id, "/bin/bash"])
        .status()
        .context("Failed to enter container")?;

    // Stop container after exiting shell to free resources
    println!("{} Stopping container...", "→".blue().bold());
    let _ = Command::new(metadata.runtime.command())
        .args(["stop", &container_id])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if !status.success() {
        bail!("Shell exited with error");
    }

    Ok(())
}

/// Remove a jail
pub fn remove(filter: Option<&str>) -> Result<()> {
    let name = select_jail(filter)?;
    let jail_dir = jail_path(&name)?;

    if !jail_dir.exists() {
        bail!("Jail '{}' not found", name);
    }

    println!("{} Removing jail '{}'...", "→".blue().bold(), name.cyan());

    // Try to stop and remove container
    if let Ok(metadata) = JailMetadata::load(&jail_dir) {
        let container_name = format!("jail-{}", sanitize_container_name(&name));

        // Stop container (ignore errors)
        let _ = Command::new(metadata.runtime.command())
            .args(["stop", &container_name])
            .output();

        // Remove container (ignore errors)
        let _ = Command::new(metadata.runtime.command())
            .args(["rm", &container_name])
            .output();
    }

    // Remove jail directory
    std::fs::remove_dir_all(&jail_dir)
        .with_context(|| format!("Failed to remove jail directory: {}", jail_dir.display()))?;

    println!("{} Jail '{}' removed", "✓".green().bold(), name.cyan());

    Ok(())
}

/// Open VSCode attached to a jail's container
pub fn code(name: &str) -> Result<()> {
    let jail_dir = jail_path(name)?;

    if !jail_dir.exists() {
        bail!("Jail '{}' not found", name);
    }

    let metadata = JailMetadata::load(&jail_dir)?;

    // Ensure image exists
    image::ensure(metadata.runtime)?;

    let container_id = get_or_create_container(name, &jail_dir, &metadata, false)?;

    println!(
        "{} Opening VSCode for jail '{}'...",
        "→".blue().bold(),
        name.cyan()
    );

    // Convert container ID to hex for VSCode URI
    let hex_id = hex_encode(&container_id);
    let uri = format!(
        "vscode-remote://attached-container+{}/{}",
        hex_id, metadata.workspace_dir
    );

    // Open VSCode
    let status = Command::new("code")
        .args(["--folder-uri", &uri])
        .status()
        .context("Failed to open VSCode. Make sure 'code' command is available.")?;

    if !status.success() {
        bail!("Failed to open VSCode");
    }

    println!(
        "{} VSCode opened. Make sure you have the 'Dev Containers' extension installed.",
        "✓".green().bold()
    );

    Ok(())
}

/// Encode string as hex
fn hex_encode(s: &str) -> String {
    s.bytes().map(|b| format!("{:02x}", b)).collect()
}

/// Show runtime status
pub fn status() -> Result<()> {
    println!("{}", "Runtime Status".bold());
    println!();

    // Check Podman
    print!("  Podman: ");
    if Runtime::Podman.is_available() {
        println!("{}", "available ✓".green());
    } else if which::which("podman").is_ok() {
        println!("{}", "installed but not running".yellow());
        if cfg!(target_os = "macos") {
            println!("         Run '{}' to start", "podman machine start".cyan());
        }
    } else {
        println!("{}", "not installed".dimmed());
    }

    // Check Docker
    print!("  Docker: ");
    if Runtime::Docker.is_available() {
        println!("{}", "available ✓".green());
    } else if which::which("docker").is_ok() {
        println!("{}", "installed but not running".yellow());
    } else {
        println!("{}", "not installed".dimmed());
    }

    println!();

    // Show active runtime
    match runtime::detect() {
        Ok(rt) => println!("  Active runtime: {}", rt.to_string().green().bold()),
        Err(_) => println!("  {}", "No container runtime available!".red().bold()),
    }

    println!();

    // Check base image
    if let Ok(rt) = runtime::detect() {
        print!("  Base image ({}): ", IMAGE_NAME);
        if image::exists(rt)? {
            println!("{}", "exists ✓".green());
        } else {
            println!("{}", "not built (will build on first use)".yellow());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_name_github_https() {
        assert_eq!(
            derive_name("https://github.com/owner/repo.git"),
            "owner/repo"
        );
        assert_eq!(derive_name("https://github.com/owner/repo"), "owner/repo");
    }

    #[test]
    fn test_derive_name_github_ssh() {
        assert_eq!(derive_name("git@github.com:owner/repo.git"), "owner/repo");
    }

    #[test]
    fn test_derive_name_local_path() {
        assert_eq!(derive_name("/home/user/projects/myproject"), "myproject");
        assert_eq!(derive_name("./myproject"), "myproject");
    }

    #[test]
    fn test_sanitize_container_name() {
        assert_eq!(sanitize_container_name("owner/repo"), "owner-repo");
        assert_eq!(sanitize_container_name("my project"), "my_project");
    }

    #[test]
    fn test_hex_encode() {
        assert_eq!(hex_encode("abc"), "616263");
    }
}
