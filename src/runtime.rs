use crate::config::UtensilConfig;
use crate::learning::{learn_charge_point, read_threshold};
extern crate coreshift_foreground;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const SCREEN_STATE_PROP: &str = "debug.tracing.screen_state";
const FALLBACK_SCREEN_STATE_PROP: &str = "debug.dcx.screenstate";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceState {
    pub display_on: bool,
    pub charging: bool,
    pub level: u8,
    pub android_major: u32,
}

struct IdleHistoryEntry {
    state: String,
    timestamp: String,
    reason: String,
    seconds_ago: Option<u64>,
}

pub trait CommandRunner {
    fn output(&mut self, program: &str, args: &[&str]) -> io::Result<String>;
    fn status(&mut self, program: &str, args: &[&str]) -> io::Result<()>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn output(&mut self, program: &str, args: &[&str]) -> io::Result<String> {
        let output = Command::new(resolve_android_tool(program)).args(args).output()?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn status(&mut self, program: &str, args: &[&str]) -> io::Result<()> {
        let status = Command::new(resolve_android_tool(program)).args(args).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("{program} exited with {status}")))
        }
    }
}

fn resolve_android_tool(program: &str) -> &str {
    match program {
        "cmd" => "/system/bin/cmd",
        "dumpsys" => "/system/bin/dumpsys",
        "service" => "/system/bin/service",
        "setprop" => "/system/bin/setprop",
        "getprop" => "/system/bin/getprop",
        "ps" => "/system/bin/ps",
        "svc" => "/system/bin/svc",
        _ => program,
    }
}

pub struct Runtime<R> {
    config: UtensilConfig,
    runner: R,
    previous: PreviousState,
    drain: DrainState,
    compaction: CompactionState,
    watchdog: WatchdogState,
    /// Exponential backoff counter for screen-off cycles (doubles each tick, caps at max_backoff_sec).
    backoff: u32,
    /// Linear counter for screen-on/charging cycles (increments up to backoff_on_cap).
    backoff_on: u32,
    /// Cached list of enabled third-party packages parsed from packages.xml.
    pkg_cache: Vec<String>,
    /// mtime of packages.xml at last parse; None = never loaded.
    pkg_cache_mtime: Option<std::time::SystemTime>,
}

#[derive(Default)]
struct PreviousState {
    charging: Option<bool>,
    display_on: Option<bool>,
    level: Option<u8>,
    battery_low: Option<bool>,
    thermal: Option<u8>,
    sensors_off: Option<bool>,
    airplane_on: Option<bool>,
    forced_idle: Option<u8>,
    deep_idle_once: bool,
}

#[derive(Default)]
struct DrainState {
    reference_level: Option<u8>,
    reference_time: Option<Instant>,
    last_tick: Option<Instant>,
    last_display_on: bool,
    screen_on: Duration,
    screen_off: Duration,
    drain_24h_start_level: u8,
    drain_24h_start_time: Option<Instant>,
    drain_24h: u8,
}

#[derive(Default)]
struct CompactionState {
    screen_off_ticks: u32,
    compacted: bool,
}

#[derive(Default)]
struct WatchdogState {
    screen_off_since: Option<Instant>,
    job_done: bool,
    first_seen: BTreeMap<String, Instant>,
}

impl<R: CommandRunner> Runtime<R> {
    pub fn new(config: UtensilConfig, runner: R) -> Self {
        Self {
            config,
            runner,
            previous: PreviousState::default(),
            drain: DrainState::default(),
            compaction: CompactionState::default(),
            watchdog: WatchdogState::default(),
            backoff: 1,
            backoff_on: 1,
            pkg_cache: Vec::new(),
            pkg_cache_mtime: None,
        }
    }

    pub fn run_daemon(&mut self) -> io::Result<()> {
        self.wait_for_boot_completed();
        loop {
            let state = self.probe_state();
            self.apply_cycle(state)?;
            self.wait_for_next_cycle(state);
        }
    }

    pub fn run_once(&mut self) -> io::Result<()> {
        let state = self.probe_state();
        self.apply_cycle(state)
    }

    pub fn print_status(&mut self) {
        let state = self.probe_state();
        let threshold = self.dynamic_threshold();
        println!("home={}", self.config.paths.home.display());
        println!("display_on={}", state.display_on);
        println!("charging={}", state.charging);
        println!("level={}", state.level);
        println!("android_major={}", state.android_major);
        println!("dynamic_threshold={threshold}");
        println!("foreground={}",
            self.current_foreground_package()
                .unwrap_or_else(|| "unknown".to_string())
        );
        println!("dry_run={}", self.config.dry_run);
    }

    pub fn print_idle_history(&mut self) -> io::Result<()> {
        let entries = parse_deviceidle_history(&self.runner.output("dumpsys", &["deviceidle"])?);
        println!("{:<16} | {:<8} | {:<16} | reason", "status", "duration", "timestamp");
        println!("--------------------------------------------------------");

        let mut last_seconds: Option<u64> = None;
        for entry in entries.iter().rev() {
            let duration = match (last_seconds, entry.seconds_ago) {
                (Some(last), Some(current)) => format_duration(last.abs_diff(current)),
                (None, Some(_)) => "current".to_string(),
                _ => "--".to_string(),
            };
            println!("{:<16} | {:<8} | {:<16} | {}",
                entry.state, duration, entry.timestamp, entry.reason
            );
            if entry.seconds_ago.is_some() {
                last_seconds = entry.seconds_ago;
            }
        }
        Ok(())
    }

    pub fn setup_defaults(&mut self) -> io::Result<()> {
        fs::create_dir_all(&self.config.paths.home)?;
        if let Some(parent) = self.config.paths.whitelist_file.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Some(parent) = self.config.paths.learning_file.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Some(parent) = self.config.paths.threshold_file.parent() {
            fs::create_dir_all(parent)?;
        }
        if !self.config.paths.whitelist_file.exists() {
            let launcher = self.default_launcher().unwrap_or_default();
            let keyboard = self.default_keyboard().unwrap_or_default();
            let mut file = fs::File::create(&self.config.paths.whitelist_file)?;
            writeln!(file, "# Protected apps")?;
            for package in [launcher.as_str(), keyboard.as_str(), "com.zing.zalo"] {
                if !package.is_empty() {
                    writeln!(file, "{package}")?;
                }
            }
        }

        let forced = self
            .runner
            .output(
                "cmd",
                &["settings", "get", "global", "forced_app_standby_enabled"],
            )
            .unwrap_or_default();
        if forced.is_empty() || forced == "null" {
            fs::write(self.config.paths.home.join("placeholdersettings"), b"")?;
        } else {
            fs::write(
                self.config.paths.home.join("backupforceappstandby"),
                format!("{forced}\n"),
            )?;
        }
        self.backup_screen_timeout()?;
        self.backup_refresh_rate()?;

        self.cmd(
            "cmd",
            &[
                "settings",
                "put",
                "global",
                "activity_manager_constants",
                "power_check_max_cpu_1=30,power_check_max_cpu_2=30,power_check_max_cpu_3=15,power_check_max_cpu_4=5,power_check_interval=600000,gc_min_interval=180000",
            ],
        )?;
        println!("Default Utensil config written to {}", self.config.paths.home.display());
        Ok(())
    }

    pub fn uninstall_restore(&mut self) {
        self.cmd_ignore("service", &["call", "batterystats", "75", "i32", "0"]);
        self.cmd_ignore("cmd", &["connectivity", "airplane-mode", "disable"]);
        self.cmd_ignore("cmd", &["power", "set-mode", "0"]);
        self.cmd_ignore("cmd", &["power", "set-adaptive-power-saver-enabled", "false"]);
        self.cmd_ignore(
            "cmd",
            &[
                "settings",
                "put",
                "global",
                "battery_stats_constants",
                "track_cpu_times_by_proc_state=false",
            ],
        );
        for key in [
            "battery_saver_constants",
            "binder_calls_stats",
            "activity_manager_constants",
            "device_idle_constants",
            "forced_app_standby_for_small_battery_enabled",
            "kernel_cpu_thread_reader",
        ] {
            self.cmd_ignore("cmd", &["settings", "delete", "global", key]);
        }

        let backup = self.config.paths.home.join("backupforceappstandby");
        let placeholder = self.config.paths.home.join("placeholdersettings");
        if let Ok(value) = fs::read_to_string(&backup) {
            self.cmd_ignore(
                "cmd",
                &[
                    "settings",
                    "put",
                    "global",
                    "forced_app_standby_enabled",
                    value.trim(),
                ],
            );
        } else if placeholder.exists() {
            self.cmd_ignore(
                "cmd",
                &["settings", "delete", "global", "forced_app_standby_enabled"],
            );
        }

        self.restore_screen_timeout();
        self.restore_refresh_rate();
        self.cmd_ignore("cmd", &["thermalservice", "reset"]);
        self.cmd_ignore("dumpsys", &["sensorservice", "enable"]);
        self.cmd_ignore("cmd", &["notification", "set_dnd", "off"]);
        self.set_property(FALLBACK_SCREEN_STATE_PROP, "none");
        self.set_property("debug.dcx.batterylevel", "none");
        self.set_property("debug.dcx.chargestate", "none");
        self.set_property("debug.dcx.drain", "");
        let _ = fs::remove_file(&self.config.paths.learning_file);
        let _ = fs::remove_file(&self.config.paths.threshold_file);
        let _ = fs::remove_file(&self.config.paths.whitelist_file);
        self.remove_restore_markers();
        println!("Utensil restore completed");
    }

    fn apply_cycle(&mut self, state: DeviceState) -> io::Result<()> {
        let threshold = self.dynamic_threshold();
        self.manage_charging_state(state)?;
        self.manage_battery_state(state, threshold)?;
        self.manage_thermal(state, threshold)?;
        self.manage_sensors(state, threshold)?;
        self.manage_drain(state)?;
        self.fast_idle(state, threshold)?;
        self.manage_airplane(state, threshold)?;
        self.manage_compaction(state)?;
        self.manage_watchdog(state)?;
        Ok(())
    }

    fn wait_for_boot_completed(&mut self) {
        loop {
            if self
                .get_property("sys.boot_completed")
                .map(|value| value == "1")
                .unwrap_or(true)
            {
                return;
            }
            thread::sleep(Duration::from_secs(5));
        }
    }

    fn probe_state(&mut self) -> DeviceState {
        let android_major = self
            .get_property("ro.build.version.release")
            .and_then(|value| value.split('.').next().unwrap_or("").parse::<u32>().ok())
            .unwrap_or(0);
        let display_on = self
            .get_property(SCREEN_STATE_PROP)
            .filter(|value| !value.is_empty())
            .or_else(|| self.runner.output("cmd", &["deviceidle", "get", "screen"]).ok())
            .map(|value| screen_value_is_on(&value))
            .unwrap_or(true);
        let level = read_trimmed("/sys/class/power_supply/battery/capacity")
            .and_then(|value| value.parse::<u8>().ok())
            .or_else(|| {
                self.runner
                    .output("cmd", &["battery", "get", "level"])
                    .ok()
                    .and_then(|value| value.parse::<u8>().ok())
            })
            .unwrap_or(0);
        let charging = read_trimmed("/sys/class/power_supply/battery/status")
            .map(|value| value == "Charging" || value == "Full")
            .or_else(|| {
                self.get_property("debug.tracing.plug_type")
                    .map(|value| value != "0" && !value.is_empty())
            })
            .unwrap_or(false);

        let state = DeviceState {
            display_on,
            charging,
            level,
            android_major,
        };
        self.publish_state(state);
        state
    }

    fn get_property(&mut self, key: &str) -> Option<String> {
        coreshift_core::android_property::android_property_get(key)
            .or_else(|| self.runner.output("getprop", &[key]).ok())
    }

    fn set_property(&mut self, key: &str, value: &str) {
        if coreshift_core::android_property::android_property_set(key, value).is_err() {
            self.cmd_ignore("setprop", &[key, value]);
        }
    }

    fn publish_state(&mut self, state: DeviceState) {
        if self.previous.display_on != Some(state.display_on) {
            self.set_property(
                FALLBACK_SCREEN_STATE_PROP,
                if state.display_on { "true" } else { "false" },
            );
            self.previous.display_on = Some(state.display_on);
        }
        if self.previous.level != Some(state.level) {
            self.set_property("debug.dcx.batterylevel", &state.level.to_string());
            self.previous.level = Some(state.level);
        }
        if self.previous.charging != Some(state.charging) {
            self.set_property(
                "debug.dcx.chargestate",
                if state.charging { "yes" } else { "no" },
            );
        }
    }

    fn dynamic_threshold(&self) -> u8 {
        read_threshold(
            &self.config.paths.threshold_file,
            self.config.battery.dynamic_threshold,
        )
    }

    fn manage_charging_state(&mut self, state: DeviceState) -> io::Result<()> {
        if self.previous.charging == Some(state.charging) {
            return Ok(());
        }
        if state.charging {
            let threshold = learn_charge_point(
                &self.config.paths.learning_file,
                &self.config.paths.threshold_file,
                state.level,
                self.config.battery.learn_min_points,
                self.config.battery.learn_max_points,
                self.config.battery.min_threshold,
                self.config.battery.max_threshold,
            )?;
            self.log(&format!("plugged in; threshold={threshold}"))?;
            if self.config.battery.dnd_while_charging {
                self.cmd("cmd", &["notification", "set_dnd", "alarms"])?;
            }
        } else {
            self.log("unplugged")?;
            if self.config.battery.dnd_while_charging {
                self.cmd("cmd", &["notification", "set_dnd", "off"])?;
            }
        }
        self.previous.charging = Some(state.charging);
        Ok(())
    }

    fn manage_battery_state(&mut self, state: DeviceState, threshold: u8) -> io::Result<()> {
        if !self.config.battery.enabled {
            return Ok(());
        }
        let low = state.level <= threshold;
        if self.previous.battery_low == Some(low) {
            return Ok(());
        }
        self.battery_setting(low)?;
        self.previous.battery_low = Some(low);
        Ok(())
    }

    fn battery_setting(&mut self, low: bool) -> io::Result<()> {
        let value = if low { "1" } else { "0" };
        self.cmd("cmd", &["power", "set-mode", value])?;
        self.cmd(
            "cmd",
            &[
                "settings",
                "put",
                "global",
                "forced_app_standby_for_small_battery_enabled",
                value,
            ],
        )?;
        self.cmd(
            "cmd",
            &["settings", "put", "global", "low_power_sticky", "0"],
        )?;
        if self.config.battery.force_app_standby {
            self.cmd(
                "cmd",
                &[
                    "settings",
                    "put",
                    "global",
                    "forced_app_standby_enabled",
                    value,
                ],
            )?;
        }
        if low {
            if let Some(rate) = self.low_refresh_rate() {
                self.cmd(
                    "cmd",
                    &["settings", "put", "system", "peak_refresh_rate", &rate],
                )?;
            }
            if let Some(timeout) = self.low_screen_timeout_ms() {
                self.cmd(
                    "cmd",
                    &[
                        "settings",
                        "put",
                        "system",
                        "screen_off_timeout",
                        &timeout.to_string(),
                    ],
                )?;
            }
            if self.config.battery.samsung_restricted_performance {
                self.cmd(
                    "cmd",
                    &[
                        "settings",
                        "put",
                        "global",
                        "restricted_device_performance",
                        "1,1",
                    ],
                )?;
            }
            if let Some(limit) = self.config.battery.background_process_limit_low {
                self.set_limit_processes(limit)?;
            }
        } else {
            if let Some(rate) = self.default_refresh_rate() {
                self.cmd(
                    "cmd",
                    &["settings", "put", "system", "peak_refresh_rate", &rate],
                )?;
            }
            if let Some(timeout) = self.default_screen_timeout_ms() {
                self.cmd(
                    "cmd",
                    &[
                        "settings",
                        "put",
                        "system",
                        "screen_off_timeout",
                        &timeout.to_string(),
                    ],
                )?;
            }
            if self.config.battery.samsung_restricted_performance {
                self.cmd(
                    "cmd",
                    &[
                        "settings",
                        "put",
                        "global",
                        "restricted_device_performance",
                        "0,0",
                    ],
                )?;
            }
            if let Some(limit) = self.config.battery.background_process_limit_default {
                self.set_limit_processes(limit)?;
            }
        }
        Ok(())
    }

    fn backup_screen_timeout(&mut self) -> io::Result<()> {
        let timeout_file = self.config.paths.home.join("screentimeout.dat");
        if timeout_file.exists() {
            return Ok(());
        }
        let timeout = self
            .runner
            .output("cmd", &["settings", "get", "system", "screen_off_timeout"])
            .unwrap_or_default();
        let Some(timeout_ms) = parse_u64_setting(&timeout) else {
            return Ok(());
        };
        fs::write(&timeout_file, format!("{timeout_ms}\n"))?;
        let modified = timeout_ms.saturating_mul(9) / 15;
        fs::write(
            self.config.paths.home.join("screentimeoutconfiguration"),
            format!("{modified}\n"),
        )?;
        Ok(())
    }

    fn backup_refresh_rate(&mut self) -> io::Result<()> {
        let max_rate = self
            .runner
            .output("dumpsys", &["display"])
            .ok()
            .and_then(|output| parse_max_refresh_rate(&output))
            .unwrap_or(60);
        let current = self
            .runner
            .output("cmd", &["settings", "get", "system", "peak_refresh_rate"])
            .unwrap_or_default();
        let current_rate = parse_refresh_rate_int(&current).unwrap_or(60);

        if max_rate >= 90 && current_rate >= 90 {
            fs::write(self.config.paths.home.join("has_high_rr"), b"")?;
            fs::write(
                self.config.paths.home.join("default_rr_val"),
                format!("{}\n", current.trim()),
            )?;
        } else {
            let _ = fs::remove_file(self.config.paths.home.join("has_high_rr"));
        }
        Ok(())
    }

    fn restore_screen_timeout(&mut self) {
        if let Some(timeout) = read_trimmed(self.config.paths.home.join("screentimeout.dat")) {
            self.cmd_ignore(
                "cmd",
                &["settings", "put", "system", "screen_off_timeout", &timeout],
            );
        }
    }

    fn restore_refresh_rate(&mut self) {
        let high_rr = self.config.paths.home.join("has_high_rr");
        if !high_rr.exists() {
            return;
        }
        let rate = read_trimmed(self.config.paths.home.join("default_rr_val")).or_else(|| {
            self.runner
                .output("dumpsys", &["display"])
                .ok()
                .and_then(|output| parse_max_refresh_rate(&output))
                .map(|rate| format!("{rate}.0"))
        });
        if let Some(rate) = rate {
            self.cmd_ignore("cmd", &["settings", "put", "system", "peak_refresh_rate", &rate]);
        }
    }

    fn remove_restore_markers(&self) {
        for file in [
            "screentimeout.dat",
            "screentimeoutconfiguration",
            "timeoutflag",
            "has_high_rr",
            "default_rr_val",
            "refreshrateconfig",
        ] {
            let _ = fs::remove_file(self.config.paths.home.join(file));
        }
    }

    fn low_refresh_rate(&self) -> Option<String> {
        self.config.battery.refresh_rate_low.clone().or_else(|| {
            self.config
                .paths
                .home
                .join("has_high_rr")
                .exists()
                .then(|| "60.0".to_string())
        })
    }

    fn default_refresh_rate(&self) -> Option<String> {
        self.config
            .battery
            .refresh_rate_default
            .clone()
            .or_else(|| read_trimmed(self.config.paths.home.join("default_rr_val")))
    }

    fn low_screen_timeout_ms(&self) -> Option<u64> {
        self.config.battery.screen_timeout_low_ms.or_else(|| {
            read_trimmed(self.config.paths.home.join("screentimeoutconfiguration"))
                .and_then(|value| value.parse::<u64>().ok())
        })
    }

    fn default_screen_timeout_ms(&self) -> Option<u64> {
        self.config.battery.screen_timeout_default_ms.or_else(|| {
            read_trimmed(self.config.paths.home.join("screentimeout.dat"))
                .and_then(|value| value.parse::<u64>().ok())
        })
    }

    fn set_limit_processes(&mut self, limit: i32) -> io::Result<()> {
        let Some(mapping) = self.activity_service_limit_mapping() else {
            return Err(io::Error::other("unsupported Android version for setlimitprocesses"));
        };
        self.cmd(
            "service",
            &[
                "call",
                "activity",
                &mapping.to_string(),
                "i32",
                &limit.to_string(),
            ],
        )
    }

    fn activity_service_limit_mapping(&mut self) -> Option<u32> {
        let android = self
            .get_property("ro.build.version.release")
            .and_then(|value| value.split('.').next().unwrap_or("").parse::<u32>().ok())?;
        match android {
            9 => Some(47),
            10 => Some(40),
            11 => Some(43),
            12 | 13 => Some(44),
            14 => Some(51),
            15 | 16 => {
                let build_id = self.get_property("ro.build.id").unwrap_or_default();
                if build_id.starts_with("BP4A") {
                    Some(58)
                } else if build_id.starts_with("BP3A") {
                    Some(55)
                } else {
                    Some(52)
                }
            }
            _ => None,
        }
    }

    fn manage_thermal(&mut self, state: DeviceState, threshold: u8) -> io::Result<()> {
        if !self.config.thermal.enabled {
            return Ok(());
        }
        let next = if state.charging {
            if state.level > self.config.battery.max_threshold {
                self.config.thermal.charging_done_status
            } else {
                self.config.thermal.charging_status
            }
        } else if state.level <= self.config.thermal.critical_level {
            self.config.thermal.discharging_critical
        } else if state.level <= threshold {
            self.config.thermal.discharging_low
        } else {
            self.config.thermal.discharging_normal
        };
        if self.previous.thermal == Some(next) {
            return Ok(());
        }
        self.cmd(
            "cmd",
            &["thermalservice", "override-status", &next.to_string()],
        )?;
        self.previous.thermal = Some(next);
        Ok(())
    }

    fn manage_sensors(&mut self, state: DeviceState, threshold: u8) -> io::Result<()> {
        if !self.config.sensors.enabled {
            return Ok(());
        }
        let off = state.level <= threshold;
        if self.previous.sensors_off == Some(off) {
            return Ok(());
        }
        if off {
            self.cmd("dumpsys", &["sensorservice", "restrict", "com.android.shell"])?;
        } else {
            self.cmd("dumpsys", &["sensorservice", "enable"])?;
        }
        self.previous.sensors_off = Some(off);
        Ok(())
    }

    fn fast_idle(&mut self, state: DeviceState, _threshold: u8) -> io::Result<()> {
        if !self.config.doze.enabled || state.android_major <= 11 {
            return Ok(());
        }
        if state.display_on || state.charging {
            if self.previous.forced_idle.is_some() {
                self.cmd_ignore("service", &["call", "batterystats", "75", "i32", "0"]);
                self.previous.forced_idle = None;
            }
            self.previous.deep_idle_once = false;
            return Ok(());
        }
        if self.previous.deep_idle_once {
            return Ok(());
        }
        let idle = self.runner.output("cmd", &["deviceidle", "get", "light"])?;
        if idle != "INACTIVE" {
            if self.previous.forced_idle != Some(1) {
                self.cmd_ignore("service", &["call", "batterystats", "75", "i32", "1"]);
                self.previous.forced_idle = Some(1);
            }
            return Ok(());
        }
        /*
        let steps = if state.level <= threshold { 1 } else { 4 };
        for _ in 0..steps {
            self.cmd("cmd", &["deviceidle", "step", "deep"])?;
        }
        */
        self.cmd("service", &["call", "batterystats", "75", "i32", "2"])?;
        self.previous.forced_idle = Some(2);
        self.previous.deep_idle_once = true;
        Ok(())
    }

    fn manage_airplane(&mut self, state: DeviceState, threshold: u8) -> io::Result<()> {
        if !self.config.airplane.enabled {
            return Ok(());
        }
        let enable = !state.display_on && state.charging && state.level <= threshold;
        let disable = state.display_on || state.level > self.config.battery.max_threshold;
        if enable && self.previous.airplane_on != Some(true) {
            self.cmd("cmd", &["connectivity", "airplane-mode", "enable"])?;
            let wifi_on = self
                .runner
                .output("cmd", &["settings", "get", "global", "wifi_on"])?;
            if wifi_on == "0" {
                self.cmd("svc", &["wifi", "enable"])?;
            }
            self.previous.airplane_on = Some(true);
        } else if disable && self.previous.airplane_on != Some(false) {
            self.cmd("cmd", &["connectivity", "airplane-mode", "disable"])?;
            self.previous.airplane_on = Some(false);
        }
        Ok(())
    }

    fn manage_drain(&mut self, state: DeviceState) -> io::Result<()> {
        if !self.config.drain.enabled {
            return Ok(());
        }
        if state.charging {
            self.drain = DrainState::default();
            return Ok(());
        }
        let now = Instant::now();
        if let Some(last_tick) = self.drain.last_tick {
            let delta = now.saturating_duration_since(last_tick);
            if self.drain.last_display_on {
                self.drain.screen_on += delta;
            } else {
                self.drain.screen_off += delta;
            }
        }
        self.drain.last_tick = Some(now);
        self.drain.last_display_on = state.display_on;
        let Some(reference_time) = self.drain.reference_time else {
            self.drain.reference_time = Some(now);
            self.drain.reference_level = Some(state.level);
            return Ok(());
        };
        let elapsed = now.saturating_duration_since(reference_time);
        if elapsed < Duration::from_secs(3600) {
            return Ok(());
        }
        let reference_level = self.drain.reference_level.unwrap_or(state.level);
        let drain = reference_level.saturating_sub(state.level);
        let per_hour = u64::from(drain) * 3600 / elapsed.as_secs().max(1);

        // 24h tracking: reset start point once 86400s elapsed
        let drain_24h_start_time = self.drain.drain_24h_start_time.get_or_insert(now);
        let elapsed_24h = now.saturating_duration_since(*drain_24h_start_time);
        if elapsed_24h >= Duration::from_secs(86400) {
            let start_level = self.drain.drain_24h_start_level;
            self.drain.drain_24h = start_level.saturating_sub(state.level);
            self.drain.drain_24h_start_level = state.level;
            self.drain.drain_24h_start_time = Some(now);
        }
        let drain_24h = self.drain.drain_24h;

        let on_min = self.drain.screen_on.as_secs() / 60;
        let off_min = self.drain.screen_off.as_secs() / 60;
        let value = format!(
            "{}%/hr|on:{}m|off:{}m|dur:{}s|24h:{}%",
            per_hour, on_min, off_min, elapsed.as_secs(), drain_24h
        );
        self.set_property("debug.dcx.drain", &value);
        self.log(&format!(
            "Drain: {}%/hr | on:{}m | off:{}m | 24h:{}%",
            per_hour, on_min, off_min, drain_24h
        ))?;

        // Carry 24h state forward; reset hourly tracking
        let drain_24h_sl = self.drain.drain_24h_start_level;
        let drain_24h_st = self.drain.drain_24h_start_time;
        self.drain = DrainState {
            reference_level: Some(state.level),
            reference_time: Some(now),
            last_tick: None,
            last_display_on: state.display_on,
            screen_on: Duration::default(),
            screen_off: Duration::default(),
            drain_24h_start_level: drain_24h_sl,
            drain_24h_start_time: drain_24h_st,
            drain_24h,
        };
        Ok(())
    }

    fn manage_compaction(&mut self, state: DeviceState) -> io::Result<()> {
        if !self.config.compaction.enabled || state.android_major < 12 {
            return Ok(());
        }
        if state.display_on {
            self.compaction = CompactionState::default();
            return Ok(());
        }
        if self.compaction.compacted {
            return Ok(());
        }
        self.compaction.screen_off_ticks += 1;
        if self.compaction.screen_off_ticks >= self.config.compaction.hardlock_ticks {
            self.run_compaction(state.android_major)?;
            self.compaction.compacted = true;
        }
        Ok(())
    }

    fn run_compaction(&mut self, android_major: u32) -> io::Result<()> {
        for package in self.running_user_packages()? {
            if android_major < 14 {
                self.cmd_ignore("cmd", &["activity", "compact", "some", &package]);
            }
            self.cmd_ignore("cmd", &["activity", "compact", "full", &package]);
        }
        self.cmd_ignore("cmd", &["activity", "compact", "system"]);
        Ok(())
    }

    fn manage_watchdog(&mut self, state: DeviceState) -> io::Result<()> {
        if !self.config.watchdog.enabled || state.display_on {
            self.watchdog = WatchdogState::default();
            return Ok(());
        }
        let now = Instant::now();
        let screen_off_since = *self.watchdog.screen_off_since.get_or_insert(now);
        let idle_duration = now.saturating_duration_since(screen_off_since);

        if self.config.watchdog.jobscheduler {
            if idle_duration >= Duration::from_secs(self.config.watchdog.idle_threshold_secs)
                && !self.watchdog.job_done
            {
                let protected = self.protected_packages_with_foreground();
                for package in self.enabled_third_party_packages()? {
                    if protected.iter().any(|protected| protected == &package) {
                        continue;
                    }
                    self.cmd("cmd", &["jobscheduler", "cancel", &package])?;
                }
                self.watchdog.job_done = true;
            }
        }

        let protected = self.protected_packages_with_foreground();
        let mut next_seen = BTreeMap::new();
        for package in self.running_user_packages()? {
            if protected.iter().any(|protected| protected == &package) {
                continue;
            }
            let first_seen = self
                .watchdog
                .first_seen
                .get(&package)
                .copied()
                .unwrap_or(now);
            if now.saturating_duration_since(first_seen)
                >= Duration::from_secs(self.config.watchdog.timeout_secs)
            {
                self.cmd("cmd", &["activity", "force-stop", &package])?;
                self.log(&format!("force stopped {package}"))?;
            } else {
                next_seen.insert(package, first_seen);
            }
        }
        self.watchdog.first_seen = next_seen;
        Ok(())
    }

    fn wait_for_next_cycle(&mut self, state: DeviceState) {
        const MAX_BACKOFF_SEC: u32 = 320;
        const MAX_BACKOFF_ON_CAP: u32 = 6;
        const BASE_OFF_INJECT: u32 = 60; // padding_sleep called with inject=60
        const DEFAULT_ON: u32 = 80;      // padding_sleep called with default=80

        let timeout = if state.display_on || state.charging {
            self.backoff = 1;
            self.backoff_on = (self.backoff_on + 1).min(MAX_BACKOFF_ON_CAP);
            Duration::from_secs(u64::from(DEFAULT_ON + self.backoff_on * 5))
        } else {
            self.backoff_on = 0;
            let duration = (BASE_OFF_INJECT * self.backoff).min(MAX_BACKOFF_SEC);
            self.backoff = (self.backoff * 2).min(MAX_BACKOFF_SEC);
            Duration::from_secs(u64::from(duration))
        };

        // Use property_wait when available so we wake immediately on screen change.
        if let Some(info) = coreshift_core::android_property::android_property_find(SCREEN_STATE_PROP)
            && let Ok(value) = coreshift_core::android_property::android_property_read(info)
        {
            match coreshift_core::android_property::android_property_wait(
                info,
                value.serial,
                Some(timeout),
            ) {
                Ok(Some(_)) => return,
                Ok(None) => return,
                Err(err) => {
                    eprintln!("utensil-poker: screen property wait failed; falling back to sleep: {err}"
                    );
                }
            }
        }
        thread::sleep(timeout);
    }

    fn current_foreground_package(&mut self) -> Option<String> {
        let status = coreshift_foreground::socket::request(b"coreshift", "status").ok()?;
        parse_focusd_status_foreground(&status)
    }

    /// Returns enabled third-party packages, cached from packages.xml.
    /// Re-parses only when packages.xml mtime changes.
    fn enabled_third_party_packages(&mut self) -> io::Result<Vec<String>> {
        let mtime = fs::metadata(&self.config.paths.packages_xml).ok().and_then(|m| m.modified().ok());
        if mtime.is_some() && mtime == self.pkg_cache_mtime && !self.pkg_cache.is_empty() {
            return Ok(self.pkg_cache.clone());
        }
        let output = self.runner.output("cmd", &["package", "list", "packages", "-3", "-e"])?;
        self.pkg_cache = output
            .lines()
            .filter_map(|l| l.strip_prefix("package:"))
            .map(|s| s.trim().to_string())
            .collect();
        self.pkg_cache_mtime = mtime;
        Ok(self.pkg_cache.clone())
    }
    fn running_user_packages(&mut self) -> io::Result<Vec<String>> {
        // Collect PIDs from cgroup top-app and foreground cpusets instead of
        // scanning all of /proc. stat(/proc/<pid>) gives owner UID via file
        // ownership — no /status parse needed.
        let mut pids: Vec<i32> = Vec::new();
        for cpuset in &[
            "/dev/cpuset/top-app/cgroup.procs",
            "/dev/cpuset/foreground/cgroup.procs",
        ] {
            if let Ok(content) = fs::read_to_string(cpuset) {
                for line in content.lines() {
                    if let Ok(pid) = line.trim().parse::<i32>() {
                        if !pids.contains(&pid) {
                            pids.push(pid);
                        }
                    }
                }
            }
        }

        let mut packages = Vec::new();
        for pid in pids {
            // stat /proc/<pid> — file uid == process uid
            let proc_path = format!("/proc/{}", pid);
            let Ok(meta) = fs::metadata(&proc_path) else { continue };
            use std::os::unix::fs::MetadataExt;
            let uid = meta.uid();
            // u0_a* apps: uid 10000–19999 (primary user)
            if uid < 10000 || uid >= 20000 { continue }
            let Ok(cmdline) = coreshift_core::proc::read_proc_cmdline(pid) else { continue };
            if cmdline.is_empty() { continue }
            let package = cmdline.split(':').next().unwrap_or(&cmdline).trim_matches('\0');
            if package.is_empty() { continue }
            if packages.iter().any(|e: &String| e == package) { continue }
            packages.push(package.to_string());
        }
        Ok(packages)
    }
    fn protected_packages(&self) -> Vec<String> {
        fs::read_to_string(&self.config.paths.whitelist_file)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(ToOwned::to_owned)
            .collect()
    }

    fn protected_packages_with_foreground(&mut self) -> Vec<String> {
        let mut protected = self.protected_packages();
        if let Some(package) = self.current_foreground_package()
            && !protected.iter().any(|entry| entry == &package)
        {
            protected.push(package);
        }
        protected
    }

    fn default_launcher(&mut self) -> io::Result<String> {
        let output = self.runner.output(
            "cmd",
            &[
                "package",
                "resolve-activity",
                "--brief",
                "-a",
                "android.intent.action.MAIN",
                "-c",
                "android.intent.category.HOME",
            ],
        )?;
        Ok(output
            .lines()
            .last()
            .and_then(|line| line.split('/').next())
            .unwrap_or("")
            .to_string())
    }

    fn default_keyboard(&mut self) -> io::Result<String> {
        let output = self.runner.output(
            "cmd",
            &["settings", "get", "secure", "default_input_method"],
        )?;
        Ok(output.split('/').next().unwrap_or("").to_string())
    }

    fn log(&mut self, message: &str) -> io::Result<()> {
        if self.config.logging.enabled {
            coreshift_core::alog_info!("utensil-poker", "{}", message);
        }
        if self.config.notification.enabled {
            self.cmd("cmd", &["notification", "post", "Utensil", message])?;
        }
        Ok(())
    }

    fn cmd(&mut self, program: &str, args: &[&str]) -> io::Result<()> {
        if self.config.dry_run {
            println!("dry-run: {} {}", program, args.join(" "));
            Ok(())
        } else {
            self.runner.status(program, args)
        }
    }

    fn cmd_ignore(&mut self, program: &str, args: &[&str]) {
        if let Err(err) = self.cmd(program, args) {
            coreshift_core::alog_error!("utensil-poker", "ignored command failure: {} {}: {}", program, args.join(" "), err);
        }
    }
}

fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_u64_setting(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() || value == "null" {
        None
    } else {
        value.parse::<u64>().ok()
    }
}

fn parse_refresh_rate_int(value: &str) -> Option<u32> {
    value
        .trim()
        .split('.')
        .next()
        .unwrap_or("")
        .parse::<u32>()
        .ok()
}

fn parse_max_refresh_rate(output: &str) -> Option<u32> {
    let mut max_rate: Option<u32> = None;
    for section in output.split("fps=").skip(1) {
        let value = section
            .chars()
            .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
            .collect::<String>();
        if let Some(rate) = parse_refresh_rate_int(&value) {
            max_rate = Some(max_rate.unwrap_or(0).max(rate));
        }
    }
    max_rate
}

fn parse_deviceidle_history(output: &str) -> Vec<IdleHistoryEntry> {
    output.lines().filter_map(parse_deviceidle_history_line).collect()
}

fn parse_deviceidle_history_line(line: &str) -> Option<IdleHistoryEntry> {
    let line = line.trim();
    let (state, rest) = line.split_once(": -")?;
    if state.is_empty() {
        return None;
    }
    let timestamp = rest.split("ms").next().unwrap_or(rest).trim().to_string();
    let reason = line
        .rfind('(')
        .and_then(|start| line.strip_suffix(')').map(|_| &line[start + 1..line.len() - 1]))
        .unwrap_or("--")
        .to_string();
    Some(IdleHistoryEntry {
        state: rename_idle_state(state),
        seconds_ago: parse_idle_timestamp_seconds(&timestamp),
        timestamp,
        reason,
    })
}

fn parse_idle_timestamp_seconds(value: &str) -> Option<u64> {
    let mut total = 0u64;
    let mut digits = String::new();
    let mut saw_unit = false;
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        if digits.is_empty() {
            continue;
        }
        let amount = digits.parse::<u64>().ok()?;
        digits.clear();
        match ch {
            'd' => {
                total = total.saturating_add(amount.saturating_mul(86_400));
                saw_unit = true;
            }
            'h' => {
                total = total.saturating_add(amount.saturating_mul(3_600));
                saw_unit = true;
            }
            'm' => {
                total = total.saturating_add(amount.saturating_mul(60));
                saw_unit = true;
            }
            's' => {
                total = total.saturating_add(amount);
                saw_unit = true;
            }
            _ => {}
        }
    }
    saw_unit.then_some(total)
}

fn rename_idle_state(state: &str) -> String {
    match state {
        "normal" => "Awake",
        "light-idle" => "light-sleep",
        "deep-idle" => "deep-sleep",
        other => other,
    }
    .to_string()
}

fn format_duration(seconds: u64) -> String {
    if seconds >= 3_600 {
        format!("{}h {}m", seconds / 3_600, (seconds % 3_600) / 60)
    } else if seconds >= 60 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

fn screen_value_is_on(value: &str) -> bool {
    matches!(
        value.trim(),
        "true" | "TRUE" | "on" | "ON" | "2" | "awake" | "Awake"
    )
}

fn parse_focusd_status_foreground(status: &str) -> Option<String> {
    status.lines().find_map(|line| {
        let value = line.strip_prefix("foreground: ")?.trim();
        if value.is_empty() || value == "unknown" {
            None
        } else {
            Some(value.to_string())
        }
    })
}

/// Parse enabled non-system packages from packages.xml.
/// Matches lines like: <package name="com.example" ... flags="..." ...>
/// Bit 1 of flags = SYSTEM (0x1); skip if set.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cmd_package_lines() {
        assert_eq!(
            parse_package_line("package:com.example"),
            Some("com.example".to_string())
        );
    }

    #[test]
    fn parses_corepolicy_status_foreground() {
        assert_eq!(
            parse_focusd_status_foreground("daemon=running\nforeground=com.example\n")
                .as_deref(),
            Some("com.example")
        );
        assert_eq!(
            parse_focusd_status_foreground("foreground=unknown\n"),
            None
        );
    }

    #[test]
    fn parses_display_refresh_rates() {
        assert_eq!(
            parse_max_refresh_rate("mode 1 fps=60.0, mode 2 fps=120.0,"),
            Some(120)
        );
    }

    #[test]
    fn parses_deviceidle_history_lines() {
        let entry = parse_deviceidle_history_line("  deep-idle: -1h2m3s (step)").unwrap();
        assert_eq!(entry.state, "deep-sleep");
        assert_eq!(entry.seconds_ago, Some(3723));
        assert_eq!(entry.reason, "step");
    }
}
