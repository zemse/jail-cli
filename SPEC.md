# jail

Sandboxed dev environments via containers. Clone untrusted repos safely so that malicious code can't access host secrets, keys, or files in other projects on the system.

## Commands

```
jail clone <git-url|local-path> [--name <n>]
jail list
jail enter <n>         # open the shell of the sandbox
jail remove <n>
jail code <n>          # open vscode attached to container
jail status            # runtime health check
```

Default name of the project can be taken as "githubowner/repo" or if its on local then name of the directory.

## Runtime

Support both Docker and Podman. Auto-detect, prefer Podman if available otherwise Docker. Config override via `~/.config/jail/config.toml` or `$JAIL_RUNTIME`.

On macOS both use a Linux VM - Podman requires `podman machine start` first.

## Security Boundary

Container gets:

- `/workspace` mount (the cloned repo only)
- SSH agent socket (signing only, no key files)
- Full network

Container cannot access:

- Host home, `~/.ssh`, `~/.aws`, `~/.config`, etc.
- Other projects
- Host env vars

## SSH Agent (macOS)

Docker: `-v /run/host-services/ssh-auth.sock:/run/ssh.sock:ro`
Podman: Forward `$SSH_AUTH_SOCK` through VM

## Base Image

`jail-dev:latest` - auto-built on first use:

- ubuntu:24.04, git, build-essential
- node (nvm), rust (rustup), python3
- claude-code
- non-root `dev` user with sudo

## Data

```
~/.local/share/jail/jails/<n>/
├── workspace/     # mounted as /workspace
└── jail.toml      # source url, container id, timestamps
```

## VSCode support

`jail code <n>` → start container if needed → `code --folder-uri vscode-remote://attached-container+<hex-name>/workspace`

Requires "Dev Containers" extension.

## Dependencies to use

clap, serde, toml, directories, anyhow, which, colored
