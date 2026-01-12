# jail

Sandboxed dev environments via containers.

## Install

```bash
cargo install jail-cli
```

Requires [Podman](https://podman.io/) or [Docker](https://www.docker.com/).

## Usage

```bash
# Clone an untrusted repo into an isolated container
jail clone https://github.com/suspicious/malicious-repo

# Expose ports for dev servers (macOS)
jail enter -p 3000 -p 5173

# Open VSCode attached to the container
jail code myproject

# List and remove jails
jail ls
jail rm
```

## How it works

- Each jail runs in its own container with a minimal dev environment (Ubuntu + common tools)
- Only the project directory is mounted - no access to host filesystem, credentials, or other projects
- Container is stopped when you exit the shell

## License

MIT
