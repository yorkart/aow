# AoW

English | [简体中文](README.zh-CN.md)

## Overview

**AoW — Agent-oriented Workbench** is a browser-based development workbench for AI agents that runs on your own computer or server.

Manage Git repositories and worktrees, browse and edit files, use terminals, review agent conversations, and run automation tasks in one interface. AoW supports popular agents such as Codex and Claude Code, with desktop and mobile interfaces.

## Installation and upgrades

Supports **Linux and macOS on x86_64 and ARM64**. Runtime requirements are Bash, Node.js 20+, Git, tar, and curl. Linux requires working systemd user services. On first installation, macOS selects LaunchAgent if the installing account has a graphical session, otherwise LaunchDaemon; updates retain that choice. Existing LaunchAgent installations keep their mode.

Run the following command on the machine where you want to run AoW:

```bash
curl -fsSL https://github.com/yorkart/aow/releases/latest/download/aow-install.sh | bash
```

The installer downloads the package for your platform from GitHub Releases, verifies it, installs it, and starts the web service. On the first installation, you must enter a local login username and password in the terminal and confirm the password before the service starts. When prompted to start terminald, enter lowercase `y` to enable terminals. Restarting terminald during an upgrade ends any existing terminal sessions it manages.

LaunchDaemon installations pause for registration by a local administrator before starting services. They run as the installing account and do not require its desktop login. Subsequent code updates need no administrator privileges; changes to the system service configuration require registration again.

To upgrade an existing installation:

```bash
aow update
```

Add `~/.local/bin` to your `PATH`. Use `aow account` to change the login username or password. Upgrades preserve the account; installations that only have an old PIN must set up an account once.

See [GitHub Releases](https://github.com/yorkart/aow/releases) for versions and release packages, and the [Installation guide](docs/release-installation.md) for installing a specific version and troubleshooting.

## Accessing AoW

By default, the web service listens only on `127.0.0.1:8282`. Open `http://127.0.0.1:8282/` on the same machine and log in with the username and password set during installation. For remote access, you can use an SSH tunnel.

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

Direct IP access is intended for trusted internal networks or VPNs, with access restricted to trusted sources. AoW can access files and terminals as the user running it. For public internet access, configure HTTPS and additional access controls.

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
