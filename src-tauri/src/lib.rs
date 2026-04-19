use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::process::{Command, Stdio};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VpnStatus {
    pub connected: bool,
    pub location: Option<String>,
    pub ip: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Location {
    pub iso: String,
    pub country: String,
    pub city: String,
    pub ping: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LicenseInfo {
    pub logged_in: bool,
    pub email: Option<String>,
    pub plan: Option<String>,
    pub devices: Option<String>,
    pub valid_until: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConfigInfo {
    pub mode: String,
    pub dns: String,
    pub protocol: String,
    pub tunnel_routing_mode: String,
    pub update_channel: String,
    pub change_system_dns: bool,
    pub post_quantum: bool,
    pub crash_reporting: bool,
    pub telemetry: bool,
    pub debug_logging: bool,
    pub show_notifications: bool,
}

fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for c2 in chars.by_ref() {
                if c2 == 'm' {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Split a line into columns using 2+ consecutive spaces as delimiter.
/// Preserves single-space words like "United Kingdom" or "São Paulo".
fn split_columns(line: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut col_start = 0;
    let mut space_run_start: Option<usize> = None;

    let bytes = line.as_bytes();
    let len = line.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b' ' {
            if space_run_start.is_none() {
                space_run_start = Some(i);
            }
            i += 1;
        } else {
            if let Some(run_start) = space_run_start {
                if i - run_start >= 2 {
                    let col = line[col_start..run_start].trim();
                    if !col.is_empty() {
                        result.push(col.to_string());
                    }
                    col_start = i;
                }
                space_run_start = None;
            }
            i += 1;
        }
    }

    let col = line[col_start..].trim();
    if !col.is_empty() {
        result.push(col.to_string());
    }

    result
}

/// Run adguardvpn-cli as the current user (read-only / unprivileged operations).
fn run_cli(args: &[&str]) -> Result<String, String> {
    let out = Command::new("adguardvpn-cli")
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run adguardvpn-cli: {e}"))?;

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();

    if out.status.success() {
        Ok(stdout)
    } else {
        Err(if !stderr.is_empty() { stderr } else { stdout })
    }
}

/// Run adguardvpn-cli with elevated privileges via sudo.
///
/// The CLI uses `sudo -E` internally for privileged operations (TUN setup,
/// routing, etc.). When run without a TTY there is no way for sudo to prompt
/// for a password, so we must either use the cached credentials or supply
/// the password ourselves via stdin with `sudo -S`.
///
/// We also pass `-E` so sudo preserves the user's environment (HOME, etc.),
/// which the CLI needs to find its credential store.
///
/// - `password = None`  → `sudo -n -E` (uses cached credentials).
///   Returns `Err("SUDO_PASSWORD_REQUIRED")` when the cache is expired.
/// - `password = Some(pwd)` → `sudo -S -E` with the password piped to stdin.
///   Returns `Err("SUDO_AUTH_FAILED")` on wrong password.
fn run_privileged(args: &[&str], password: Option<&str>) -> Result<String, String> {
    match password {
        None => {
            let out = Command::new("sudo")
                .args(["-n", "-E"])
                .arg("adguardvpn-cli")
                .args(args)
                .output()
                .map_err(|e| format!("Failed to run sudo: {e}"))?;

            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();

            if out.status.success() {
                return Ok(stdout);
            }

            // sudo writes "sudo: …" to stderr on auth failure — locale-independent.
            if stderr.trim_start().starts_with("sudo:") {
                Err("SUDO_PASSWORD_REQUIRED".to_string())
            } else {
                Err(if !stderr.is_empty() { stderr } else { stdout })
            }
        }

        Some(pwd) => {
            let mut child = Command::new("sudo")
                .args(["-S", "-E", "-p", ""])
                .arg("adguardvpn-cli")
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("Failed to spawn sudo: {e}"))?;

            if let Some(mut stdin) = child.stdin.take() {
                let _ = writeln!(stdin, "{pwd}");
            }

            let out = child
                .wait_with_output()
                .map_err(|e| format!("Failed waiting for sudo: {e}"))?;

            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();

            if out.status.success() {
                return Ok(stdout);
            }

            let combined = format!("{stdout}{stderr}").to_lowercase();
            if combined.contains("incorrect")
                || combined.contains("sorry")
                || combined.contains("authentication failure")
                || combined.contains("try again")
                || combined.contains("falsch")       // German
                || combined.contains("fehlgeschlagen")
            {
                Err("SUDO_AUTH_FAILED".to_string())
            } else {
                Err(if !stderr.is_empty() { stderr } else { stdout })
            }
        }
    }
}

// ── Parsing ───────────────────────────────────────────────────────────────────

fn parse_status(output: &str) -> VpnStatus {
    let clean = strip_ansi(output);
    let first_line = clean.lines().next().unwrap_or("").to_lowercase();

    if first_line.contains("connected") && !first_line.contains("disconnected") {
        let lower = clean.to_lowercase();
        let location = lower.find("connected to ").map(|pos| {
            let start = pos + "connected to ".len();
            let end = clean[start..].find('\n').map(|p| start + p).unwrap_or(clean.len());
            clean[start..end].trim().to_string()
        });

        let ip = clean
            .lines()
            .find(|l| {
                let lo = l.to_lowercase();
                lo.contains("outbound ip") || lo.contains("ip address") || lo.contains("ip:")
            })
            .and_then(|l| l.splitn(2, ':').nth(1))
            .map(|s| s.trim().to_string());

        VpnStatus { connected: true, location, ip }
    } else {
        VpnStatus { connected: false, location: None, ip: None }
    }
}

fn parse_locations(output: &str) -> Vec<Location> {
    let clean = strip_ansi(output);
    let mut locations = Vec::new();

    for line in clean.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let mut chars = line.chars();
        let c0 = chars.next().unwrap_or(' ');
        let c1 = chars.next().unwrap_or(' ');
        let c2 = chars.next().unwrap_or(' ');
        if !c0.is_ascii_uppercase() || !c1.is_ascii_uppercase() || c2 != ' ' {
            continue;
        }

        let parts = split_columns(line);
        if parts.len() >= 3 {
            locations.push(Location {
                iso: parts[0].clone(),
                country: parts[1].clone(),
                city: parts[2].clone(),
                ping: parts.get(3).and_then(|s| s.parse::<u32>().ok()),
            });
        }
    }

    locations
}

fn parse_license(output: &str) -> LicenseInfo {
    let clean = strip_ansi(output);

    if clean.to_lowercase().contains("not logged in") || clean.to_lowercase().contains("log in") {
        return LicenseInfo {
            logged_in: false,
            email: None,
            plan: None,
            devices: None,
            valid_until: None,
        };
    }

    let email = clean
        .lines()
        .find(|l| l.contains("Logged in as"))
        .and_then(|l| l.splitn(2, "Logged in as").nth(1))
        .map(|s| s.trim().to_string());

    let plan = clean.lines().find(|l| l.contains("You are using")).map(|l| {
        if l.contains("PREMIUM") || l.contains("premium") {
            "PREMIUM".to_string()
        } else {
            "FREE".to_string()
        }
    });

    let devices = clean
        .lines()
        .find(|l| l.contains("Up to"))
        .and_then(|l| l.splitn(2, "Up to").nth(1))
        .map(|s| s.trim().split_whitespace().next().unwrap_or("").to_string());

    let valid_until = clean
        .lines()
        .find(|l| l.contains("valid until"))
        .and_then(|l| l.splitn(2, "valid until").nth(1))
        .map(|s| s.trim().to_string());

    LicenseInfo { logged_in: true, email, plan, devices, valid_until }
}

fn parse_config(output: &str) -> ConfigInfo {
    let clean = strip_ansi(output);

    let get_val = |key: &str| -> String {
        clean
            .lines()
            .find(|l| l.contains(key))
            .and_then(|l| l.splitn(2, ':').nth(1))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };

    let bool_val = |key: &str| -> bool { get_val(key).to_lowercase().starts_with("on") };

    ConfigInfo {
        mode: get_val("Mode"),
        dns: get_val("DNS upstream"),
        protocol: get_val("Protocol"),
        tunnel_routing_mode: get_val("Tunnel routing mode"),
        update_channel: get_val("Update channel"),
        change_system_dns: bool_val("Change system DNS"),
        post_quantum: get_val("Post-quantum cryptography").to_lowercase().contains("on"),
        crash_reporting: bool_val("Crash reporting"),
        telemetry: bool_val("Send anonymized usage data"),
        debug_logging: bool_val("Debug logging"),
        show_notifications: bool_val("Show notifications"),
    }
}

// ── Tauri commands ────────────────────────────────────────────────────────────

#[tauri::command]
fn get_status() -> Result<VpnStatus, String> {
    Ok(parse_status(&run_cli(&["status"])?))
}

#[tauri::command]
fn list_locations() -> Result<Vec<Location>, String> {
    Ok(parse_locations(&run_cli(&["list-locations"])?))
}

#[tauri::command]
fn vpn_connect(
    location: Option<String>,
    fastest: bool,
    sudo_password: Option<String>,
) -> Result<String, String> {
    if fastest {
        return run_privileged(&["connect", "--yes", "--fastest"], sudo_password.as_deref());
    }
    if let Some(ref loc) = location {
        let args = ["connect", "--yes", "-l", loc.as_str()];
        run_privileged(&args, sudo_password.as_deref())
    } else {
        run_privileged(&["connect", "--yes"], sudo_password.as_deref())
    }
}

#[tauri::command]
fn vpn_disconnect(sudo_password: Option<String>) -> Result<String, String> {
    run_privileged(&["disconnect"], sudo_password.as_deref())
}

#[tauri::command]
fn get_license() -> Result<LicenseInfo, String> {
    Ok(parse_license(&run_cli(&["license"])?))
}

#[tauri::command]
fn get_config() -> Result<ConfigInfo, String> {
    Ok(parse_config(&run_cli(&["config", "show"])?))
}

#[tauri::command]
fn config_set_mode(mode: String, sudo_password: Option<String>) -> Result<String, String> {
    run_privileged(&["config", "set-mode", &mode], sudo_password.as_deref())
}

#[tauri::command]
fn config_set_dns(dns: String, sudo_password: Option<String>) -> Result<String, String> {
    run_privileged(&["config", "set-dns", &dns], sudo_password.as_deref())
}

#[tauri::command]
fn config_set_protocol(protocol: String, sudo_password: Option<String>) -> Result<String, String> {
    run_privileged(&["config", "set-protocol", &protocol], sudo_password.as_deref())
}

#[tauri::command]
fn config_set_post_quantum(enabled: bool, sudo_password: Option<String>) -> Result<String, String> {
    run_privileged(
        &["config", "set-post-quantum", if enabled { "true" } else { "false" }],
        sudo_password.as_deref(),
    )
}

#[tauri::command]
fn config_set_crash_reporting(
    enabled: bool,
    sudo_password: Option<String>,
) -> Result<String, String> {
    run_privileged(
        &["config", "set-crash-reporting", if enabled { "true" } else { "false" }],
        sudo_password.as_deref(),
    )
}

#[tauri::command]
fn config_set_telemetry(enabled: bool, sudo_password: Option<String>) -> Result<String, String> {
    run_privileged(
        &["config", "set-telemetry", if enabled { "true" } else { "false" }],
        sudo_password.as_deref(),
    )
}

#[tauri::command]
fn vpn_logout(sudo_password: Option<String>) -> Result<String, String> {
    run_privileged(&["logout"], sudo_password.as_deref())
}

#[tauri::command]
fn get_exclusions() -> Result<(String, Vec<String>), String> {
    let mode_out = run_cli(&["site-exclusions", "mode"])?;
    let list_out = run_cli(&["site-exclusions", "show"])?;

    let mode = strip_ansi(&mode_out).trim().to_string();
    let exclusions: Vec<String> = strip_ansi(&list_out)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.to_lowercase().contains("no exclusions"))
        .collect();

    Ok((mode, exclusions))
}

#[tauri::command]
fn add_exclusion(site: String, sudo_password: Option<String>) -> Result<String, String> {
    run_privileged(&["site-exclusions", "add", &site], sudo_password.as_deref())
}

#[tauri::command]
fn remove_exclusion(site: String, sudo_password: Option<String>) -> Result<String, String> {
    run_privileged(&["site-exclusions", "remove", &site], sudo_password.as_deref())
}

#[tauri::command]
fn set_exclusions_mode(mode: String, sudo_password: Option<String>) -> Result<String, String> {
    run_privileged(&["site-exclusions", "mode", &mode], sudo_password.as_deref())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_status,
            list_locations,
            vpn_connect,
            vpn_disconnect,
            get_license,
            get_config,
            config_set_mode,
            config_set_dns,
            config_set_protocol,
            config_set_post_quantum,
            config_set_crash_reporting,
            config_set_telemetry,
            vpn_logout,
            get_exclusions,
            add_exclusion,
            remove_exclusion,
            set_exclusions_mode,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
