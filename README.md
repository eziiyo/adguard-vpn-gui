# AdGuard VPN GUI

A desktop GUI for [adguardvpn-cli](https://adguard-vpn.com/en/blog/adguard-vpn-linux.html) built with Tauri 2 + Vue 3 + TypeScript.

## Features

- **Connect / Disconnect** with one click
- **Location picker** — full list with country flags, ping times, and search
- **Fastest location** auto-select
- **Site exclusions** — general and selective mode
- **Account info** — plan, devices, expiry date
- **Settings** — VPN mode (TUN/SOCKS), protocol, post-quantum encryption, crash reporting, analytics
- **GUI password prompt** — no terminal needed; prompts for your sudo password when required and caches credentials for ~15 minutes

## Requirements

- [`adguardvpn-cli`](https://adguard-vpn.com/en/blog/adguard-vpn-linux.html) installed and logged in
- Rust + Cargo
- Node.js

## Setup

**Log in to AdGuard VPN** (one-time, in a terminal):
```sh
adguardvpn-cli login
```

**Install dependencies:**
```sh
npm install
```

## Development

```sh
npm run tauri dev
```

## Build

```sh
npm run tauri build
```

## How privilege escalation works

`adguardvpn-cli` uses `sudo` internally to configure network interfaces (TUN setup, routing scripts, etc.). It has no polkit integration, so running it from a GUI without a terminal would normally leave sudo with no way to prompt for a password.

This app works around that by showing its own password dialog and running:

```
sudo -S -E adguardvpn-cli <command>
```

- `-S` reads the password from stdin (no TTY needed)
- `-E` preserves your environment so the CLI finds its credentials in your home directory

Once authenticated, sudo caches your credentials for ~15 minutes (the system default), so you won't be prompted again for subsequent operations within that window.
