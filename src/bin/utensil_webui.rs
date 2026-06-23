use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;

const UTENSIL_DIR: &str = "/data/local/tmp/utensil";
const UTENSIL_CONFIG: &str = "/data/local/tmp/utensil/utensil.conf";
const ROM_UTENSIL_CONFIG: &str = "/system/etc/utensil/utensil.conf";

fn main() {
    if let Err(err) = run() {
        coreshift_core::alog_error!("utensil-webui", "{err}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let args = Args::parse()?;
    if !is_loopback(&args.listen) {
        return Err(io::Error::other("listen address must be loopback"));
    }

    let listener = TcpListener::bind(&args.listen)?;
    coreshift_core::alog_info!("utensil-webui", "listening on http://{}", args.listen);

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                if let Err(err) = handle_request(&args, &mut stream) {
                    coreshift_core::alog_error!("utensil-webui", "request failed: {err}");
                }
            }
            Err(err) => coreshift_core::alog_error!("utensil-webui", "utensil-webui: accept failed: {err}"),
        }
    }

    Ok(())
}

#[derive(Debug)]
struct Args {
    listen: String,
    webroot: PathBuf,
}

impl Args {
    fn parse() -> io::Result<Self> {
        let mut listen = "127.0.0.1:8787".to_string();
        let mut webroot = PathBuf::from("/system/etc/utensil/webroot");
        let mut args = env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--listen" => {
                    listen = args
                        .next()
                        .ok_or_else(|| io::Error::other("missing --listen value"))?;
                }
                "--webroot" => {
                    webroot = args
                        .next()
                        .map(PathBuf::from)
                        .ok_or_else(|| io::Error::other("missing --webroot value"))?;
                }
                _ => return Err(io::Error::other(format!("unknown argument `{arg}`"))),
            }
        }

        Ok(Self { listen, webroot })
    }
}

fn handle_request(args: &Args, stream: &mut TcpStream) -> io::Result<()> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if buffer.len() > 64 * 1024 {
            return write_response(stream, 413, "text/plain", "request too large\n");
        }
    }

    let request = String::from_utf8_lossy(&buffer);
    let mut lines = request.lines();
    let first = lines.next().unwrap_or("");
    let mut first_parts = first.split_whitespace();
    let method = first_parts.next().unwrap_or("");
    let path = first_parts.next().unwrap_or("/");
    let content_length = request
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length:"))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);

    let header_end = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|idx| idx + 4)
        .unwrap_or(buffer.len());
    let mut body = buffer[header_end..].to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(content_length);

    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => {
            match load_index(args) {
                Ok(html) => write_response(stream, 200, "text/html; charset=utf-8", &html),
                Err(err) => write_response(
                    stream,
                    500,
                    "text/plain",
                    &format!("failed to load webroot: {err}\n"),
                ),
            }
        }
        ("GET", "/health") => write_response(stream, 200, "text/plain", "ok\n"),
        ("POST", "/api/exec") => {
            let command = String::from_utf8_lossy(&body);
            let result = run_shell_bridge(command.trim())?;
            write_response(stream, 200, "application/json", &result.to_json())
        }
        _ => write_response(stream, 404, "text/plain", "not found\n"),
    }
}

fn load_index(args: &Args) -> io::Result<String> {
    let raw = fs::read_to_string(args.webroot.join("index.html"))?;
    Ok(patch_index(&raw))
}

fn patch_index(raw: &str) -> String {
    raw.replace(
        "import { exec, fullScreen } from \"ax://kernelsu.js\";",
        "\
const exec = async (command) => {
  const response = await fetch('/api/exec', { method: 'POST', body: command });
  if (!response.ok) throw new Error(await response.text());
  return await response.json();
};
const fullScreen = () => {};
",
    )
    .replace("exec(`utensil ${command}`)", "exec(`utensil-poker ${command}`)")
}

fn run_shell_bridge(command: &str) -> io::Result<ExecResult> {
    if let Some(result) = handle_config_bridge(command)? {
        return Ok(result);
    }

    let output = Command::new("/system/bin/sh")
        .arg("-c")
        .arg(command)
        .env("PATH", "/system/bin:/system/xbin:/vendor/bin:/product/bin")
        .output()?;
    Ok(ExecResult {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(255),
    })
}

fn handle_config_bridge(command: &str) -> io::Result<Option<ExecResult>> {
    if command.starts_with("getprop ") {
        let prop = command.strip_prefix("getprop ").unwrap().trim();
        let value = coreshift_core::android_property::android_property_get(prop).unwrap_or_default();
        return Ok(Some(ExecResult::stdout(&format!("{}\n", value))));
    }

    if command.starts_with("setprop ") {
        let parts: Vec<&str> = command.strip_prefix("setprop ").unwrap().split_whitespace().collect();
        if parts.len() == 2 {
            let name = parts[0];
            let value = parts[1];
            let _ = coreshift_core::android_property::android_property_set(name, value);
            return Ok(Some(ExecResult::stdout("")));
        }
    }

    if let Some(marker) = parse_marker_exists(command) {
        if marker_config_key(marker).is_none() {
            return Ok(None);
        }
        let config = read_config_map()?;
        return Ok(Some(ExecResult::stdout(if marker_enabled(&config, marker) {
            "1\n"
        } else {
            "0\n"
        })));
    }

    if let Some(marker) = parse_touch_marker(command) {
        if marker_config_key(marker).is_none() {
            return Ok(None);
        }
        set_marker(marker, true)?;
        return Ok(Some(ExecResult::stdout("")));
    }

    if let Some(marker) = parse_rm_marker(command) {
        if marker_config_key(marker).is_none() {
            return Ok(None);
        }
        set_marker(marker, false)?;
        return Ok(Some(ExecResult::stdout("")));
    }

    if command == "cat /data/local/tmp/utensil/thermalconfiguration 2>/dev/null || echo \"\"" {
        let config = read_config_map()?;
        let body = format!(
            "thermal_discharging_normal={}\nthermal_discharging_low={}\nthermal_discharging_critical={}\nthermal_critical_level={}\n",
            config_value(&config, "thermal.discharging_normal", "1"),
            config_value(&config, "thermal.discharging_low", "2"),
            config_value(&config, "thermal.discharging_critical", "3"),
            config_value(&config, "thermal.critical_level", "20"),
        );
        return Ok(Some(ExecResult::stdout(&body)));
    }

    if command.starts_with("printf '%s\\n' ") && command.ends_with(" > /data/local/tmp/utensil/thermalconfiguration") {
        let values = parse_printf_values(command);
        let mut updates = BTreeMap::new();
        for value in values {
            if let Some((key, val)) = value.split_once('=') {
                match key {
                    "thermal_discharging_normal" => {
                        updates.insert("thermal.discharging_normal".to_string(), val.to_string());
                    }
                    "thermal_discharging_low" => {
                        updates.insert("thermal.discharging_low".to_string(), val.to_string());
                    }
                    "thermal_discharging_critical" => {
                        updates.insert("thermal.discharging_critical".to_string(), val.to_string());
                    }
                    "thermal_critical_level" => {
                        updates.insert("thermal.critical_level".to_string(), val.to_string());
                    }
                    _ => {}
                }
            }
        }
        updates.insert("thermal.enabled".to_string(), "true".to_string());
        update_config(updates)?;
        return Ok(Some(ExecResult::stdout("")));
    }

    Ok(None)
}

fn parse_marker_exists(command: &str) -> Option<&str> {
    let prefix = "[ -f \"/data/local/tmp/utensil/";
    let suffix = "\" ] && echo 1 || echo 0";
    command
        .strip_prefix(prefix)?
        .strip_suffix(suffix)
        .filter(|marker| marker.chars().all(is_marker_char))
}

fn parse_touch_marker(command: &str) -> Option<&str> {
    let marker = command.strip_prefix("touch /data/local/tmp/utensil/")?;
    marker.chars().all(is_marker_char).then_some(marker)
}

fn parse_rm_marker(command: &str) -> Option<&str> {
    let marker = command.strip_prefix("rm -f /data/local/tmp/utensil/")?;
    marker.chars().all(is_marker_char).then_some(marker)
}

fn is_marker_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

fn marker_enabled(config: &BTreeMap<String, String>, marker: &str) -> bool {
    match marker_config_key(marker) {
        Some((key, expected)) => config.get(key).map(|value| value == expected).unwrap_or(false),
        None => false,
    }
}

fn set_marker(marker: &str, enabled: bool) -> io::Result<()> {
    let Some((key, enabled_value)) = marker_config_key(marker) else {
        return Ok(());
    };
    let disabled_value = marker_disabled_value(marker);
    let mut updates = BTreeMap::new();
    updates.insert(
        key.to_string(),
        if enabled {
            enabled_value.to_string()
        } else {
            disabled_value.to_string()
        },
    );

    if marker == "cmdthermal" && enabled {
        updates.insert("thermal.enabled".to_string(), "true".to_string());
    }
    update_config(updates)
}

fn marker_config_key(marker: &str) -> Option<(&'static str, &'static str)> {
    match marker {
        "draincheck" => Some(("drain.enabled", "true")),
        "sensorconfiguration" => Some(("sensors.enabled", "true")),
        "airplaneconfiguration" => Some(("airplane.enabled", "true")),
        "charglogconfiguration" => Some(("logging.enabled", "true")),
        "compactionconfiguration" => Some(("compaction.enabled", "true")),
        "timeoutflag" => Some(("battery.screen_timeout_low_ms", "72000")),
        "notificationconfiguration" => Some(("notification.enabled", "true")),
        "dndconfiguration" => Some(("battery.dnd_while_charging", "true")),
        "watchdogconfiguration" => Some(("watchdog.enabled", "true")),
        "jobschedulerconfiguration" => Some(("watchdog.jobscheduler", "true")),
        "cmdthermal" => Some(("thermal.enabled", "true")),
        "bgapplimits" => Some(("battery.background_process_limit_low", "3")),
        "refreshrateconfig" => Some(("battery.refresh_rate_low", "60.0")),
        "restricteddeviceperformance" => Some(("battery.samsung_restricted_performance", "true")),
        _ => None,
    }
}

fn marker_disabled_value(marker: &str) -> &'static str {
    match marker {
        "timeoutflag" => "",
        "bgapplimits" => "",
        "refreshrateconfig" => "",
        _ => "false",
    }
}

fn parse_printf_values(command: &str) -> Vec<String> {
    let body = command
        .trim_start_matches("printf '%s\\n' ")
        .trim_end_matches(" > /data/local/tmp/utensil/thermalconfiguration");
    let mut values = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find('\'') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('\'') else {
            break;
        };
        values.push(rest[..end].to_string());
        rest = &rest[end + 1..];
    }
    values
}

fn config_value<'a>(config: &'a BTreeMap<String, String>, key: &str, default: &'a str) -> &'a str {
    config.get(key).map(String::as_str).unwrap_or(default)
}

fn ensure_config() -> io::Result<()> {
    let path = PathBuf::from(UTENSIL_CONFIG);
    if path.exists() {
        return Ok(());
    }
    fs::create_dir_all(UTENSIL_DIR)?;
    if PathBuf::from(ROM_UTENSIL_CONFIG).exists() {
        fs::copy(ROM_UTENSIL_CONFIG, UTENSIL_CONFIG)?;
    } else {
        fs::write(UTENSIL_CONFIG, default_config_text())?;
    }
    Ok(())
}

fn read_config_map() -> io::Result<BTreeMap<String, String>> {
    ensure_config()?;
    let mut map = BTreeMap::new();
    for line in fs::read_to_string(UTENSIL_CONFIG)?.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    Ok(map)
}

fn update_config(updates: BTreeMap<String, String>) -> io::Result<()> {
    ensure_config()?;
    let text = fs::read_to_string(UTENSIL_CONFIG)?;
    let mut seen = BTreeMap::new();
    let mut lines = Vec::new();
    for line in text.lines() {
        if let Some((key, _)) = line.split_once('=') {
            let key = key.trim();
            if let Some(value) = updates.get(key) {
                lines.push(format!("{key}={value}"));
                seen.insert(key.to_string(), true);
                continue;
            }
        }
        lines.push(line.to_string());
    }
    for (key, value) in updates {
        if !seen.contains_key(&key) {
            lines.push(format!("{key}={value}"));
        }
    }
    fs::write(UTENSIL_CONFIG, format!("{}\n", lines.join("\n")))
}

fn default_config_text() -> &'static str {
    include_str!("../../../packaging/magisk/utensil.conf")
}

struct ExecResult {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

impl ExecResult {
    fn stdout(stdout: &str) -> Self {
        Self {
            stdout: stdout.to_string(),
            stderr: String::new(),
            exit_code: 0,
        }
    }

    fn to_json(&self) -> String {
        format!(
            "{{\"stdout\":\"{}\",\"stderr\":\"{}\",\"exitCode\":{}}}",
            json_escape(&self.stdout),
            json_escape(&self.stderr),
            self.exit_code
        )
    }
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        413 => "Payload Too Large",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn is_loopback(value: &str) -> bool {
    value.starts_with("127.") || value.starts_with("localhost:")
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => escaped.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_kernelsu_import_and_utensil_command() {
        let patched = patch_index(
            "import { exec, fullScreen } from \"ax://kernelsu.js\";\nexec(`utensil ${command}`)",
        );
        assert!(!patched.contains("ax://"));
        assert!(patched.contains("/api/exec"));
        assert!(patched.contains("utensil-poker ${command}"));
    }

    #[test]
    fn recognizes_webui_config_markers() {
        assert_eq!(
            parse_marker_exists("[ -f \"/data/local/tmp/utensil/sensorconfiguration\" ] && echo 1 || echo 0"),
            Some("sensorconfiguration")
        );
        assert_eq!(
            marker_config_key("sensorconfiguration"),
            Some(("sensors.enabled", "true"))
        );
        assert_eq!(
            marker_config_key("refreshrateconfig"),
            Some(("battery.refresh_rate_low", "60.0"))
        );
        assert!(marker_config_key("has_high_rr").is_none());
    }

    #[test]
    fn parses_thermal_printf_payload() {
        let values = parse_printf_values(
            "printf '%s\\n' 'thermal_discharging_normal=1' 'thermal_discharging_low=4' 'thermal_discharging_critical=5' 'thermal_critical_level=30' > /data/local/tmp/utensil/thermalconfiguration",
        );
        assert_eq!(
            values,
            vec![
                "thermal_discharging_normal=1",
                "thermal_discharging_low=4",
                "thermal_discharging_critical=5",
                "thermal_critical_level=30"
            ]
        );
    }
}
