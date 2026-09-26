# AoW

English | [简体中文](README.zh-CN.md)

## Overview

**AoW — Agent-oriented Workbench** is a browser-based development workbench for AI agents that runs on your own computer or server.

Manage Git repositories and worktrees, browse and edit files, use terminals, review agent conversations, and run automation tasks in one interface. AoW supports popular agents such as Codex and Claude Code, with desktop and mobile interfaces.

For details, see the [Usage guide](docs/usage.md).

Product website: [yorkart.github.io/aow](https://yorkart.github.io/aow/). The website is published separately through GitHub Pages. See [website/README.md](website/README.md) for its source, preview, and deployment instructions.

## Installation and upgrades

Supports **Linux and macOS on x86_64 and ARM64**. Runtime requirements are Bash, Node.js 20+, Git, tar, and curl. Linux requires working systemd user services; macOS requires the current user to be logged into a graphical session.

Run the following command on the machine where you want to run AoW:

```bash
curl -fsSL https://github.com/yorkart/aow/releases/latest/download/aow-install.sh | bash
```

The installer downloads the package for your platform from GitHub Releases, verifies it, installs it, and starts the web service. On the first installation, you must set a six-digit access PIN in the terminal. When prompted to start terminald, enter lowercase `y` to enable terminals. Restarting terminald during an upgrade ends any existing terminal sessions it manages.

To upgrade an existing installation:

```bash
aow update
```

Add `~/.local/bin` to your `PATH`. Use `aow pin` to change the access PIN.

See [GitHub Releases](https://github.com/yorkart/aow/releases) for versions and release packages, and the [Installation guide](docs/release-installation.md) for installing a specific version and troubleshooting.

## Accessing AoW

By default, the web service listens only on `127.0.0.1:8282`. Open `http://127.0.0.1:8282/` on the same machine and enter the PIN set during installation. For remote access, you can use an SSH tunnel.

### Direct access by IP address (explicit opt-in required)

Choose one bind address in `~/.config/aow/server.env`. Keep only one setting active, replace specific addresses with an actual IP address assigned to a network interface, and uncomment the example you want to use:

```dotenv
# Specific IPv4 address
AOW_SERVER_HOST=192.168.1.10

# All IPv4 interfaces
# AOW_SERVER_HOST=0.0.0.0

# Specific IPv6 address
# AOW_SERVER_HOST=2001:db8::10

# All IPv6 interfaces; some systems also accept IPv4 connections
# AOW_SERVER_HOST=::
```

Apply the setting as described in the [Configuration guide](docs/configuration.md#监听地址与服务配置), and configure your firewall or security group to allow only trusted sources to access TCP port 8282. Then open `http://<server-ip>:8282/` (for IPv6, use `http://[ipv6-address]:8282/`). Phones automatically use the mobile interface.

Direct IP access is intended for trusted internal networks or VPNs, with access restricted to trusted sources. AoW can access files and terminals as the user running it, and the six-digit PIN is only a lightweight access barrier. For public internet access, configure HTTPS and additional access controls.

### Access through an SSH tunnel

You can securely connect to AoW on your remote machine through an SSH tunnel. Run the following command on your local machine, replacing `user@host` with the SSH login address of the remote machine:

```bash
ssh -N -o ExitOnForwardFailure=yes -L 127.0.0.1:18282:127.0.0.1:8282 user@host
```

Keep the SSH connection open and visit `http://127.0.0.1:18282/` in your local browser. You can replace local port `18282` with another available port.

The default listen address works with this tunnel; the server only needs to expose its SSH port.

## Local development and requirements

In addition to the runtime dependencies, install Rust stable / Cargo, npm, and just. macOS requires Xcode Command Line Tools. Building Linux packages requires a C compiler, `musl-gcc`, `readelf`, and the Rust musl target for the machine's architecture. Testing the macOS installation scripts also requires Python 3.

From the repository root, run `just build` to build and `just test` to run tests. For local deployment, run `just package`, then `just install`. Local packages use the same installation process as GitHub release packages and update the services actually running under the current user. Development services also listen only on localhost by default.
