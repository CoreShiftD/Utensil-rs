use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_CONFIG_PATH: &str = "/data/local/tmp/utensil/utensil.conf";
pub const ROM_DEFAULT_CONFIG_PATH: &str = "/system/etc/utensil/utensil.conf";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UtensilConfig {
    pub paths: PathConfig,
    pub battery: BatteryConfig,
    pub thermal: ThermalConfig,
    pub sensors: FeatureConfig,
    pub airplane: FeatureConfig,
    pub doze: FeatureConfig,
    pub compaction: CompactionConfig,
    pub watchdog: WatchdogConfig,
    pub drain: FeatureConfig,
    pub notification: FeatureConfig,
    pub logging: FeatureConfig,
    pub dry_run: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathConfig {
    pub home: PathBuf,
    pub learning_file: PathBuf,
    pub threshold_file: PathBuf,
    pub whitelist_file: PathBuf,
    pub drain_log_file: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatteryConfig {
    pub enabled: bool,
    pub dynamic_threshold: u8,
    pub min_threshold: u8,
    pub max_threshold: u8,
    pub learn_min_points: usize,
    pub learn_max_points: usize,
    pub force_app_standby: bool,
    pub dnd_while_charging: bool,
    pub refresh_rate_low: Option<String>,
    pub refresh_rate_default: Option<String>,
    pub screen_timeout_low_ms: Option<u64>,
    pub screen_timeout_default_ms: Option<u64>,
    pub samsung_restricted_performance: bool,
    pub background_process_limit_low: Option<i32>,
    pub background_process_limit_default: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThermalConfig {
    pub enabled: bool,
    pub charging_status: u8,
    pub charging_done_status: u8,
    pub discharging_normal: u8,
    pub discharging_low: u8,
    pub discharging_critical: u8,
    pub critical_level: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatureConfig {
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompactionConfig {
    pub enabled: bool,
    pub hardlock_ticks: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchdogConfig {
    pub enabled: bool,
    pub jobscheduler: bool,
    pub timeout_secs: u64,
    pub idle_threshold_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ConfigError {}

impl UtensilConfig {
    pub fn for_config_path(path: &Path) -> Self {
        let home = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("/data/local/tmp/utensil"))
            .to_path_buf();
        Self {
            paths: PathConfig {
                learning_file: home.join("battery_learning.dat"),
                threshold_file: home.join("battery_threshold.dat"),
                whitelist_file: home.join("fileconfig.txt"),
                drain_log_file: PathBuf::from("/sdcard/chargelog.txt"),
                home,
            },
            battery: BatteryConfig {
                enabled: true,
                dynamic_threshold: 25,
                min_threshold: 25,
                max_threshold: 60,
                learn_min_points: 5,
                learn_max_points: 25,
                force_app_standby: false,
                dnd_while_charging: false,
                refresh_rate_low: None,
                refresh_rate_default: None,
                screen_timeout_low_ms: None,
                screen_timeout_default_ms: None,
                samsung_restricted_performance: false,
                background_process_limit_low: Some(2),
                background_process_limit_default: None,
            },
            thermal: ThermalConfig {
                enabled: false,
                charging_status: 1,
                charging_done_status: 0,
                discharging_normal: 1,
                discharging_low: 2,
                discharging_critical: 3,
                critical_level: 20,
            },
            sensors: FeatureConfig { enabled: false },
            airplane: FeatureConfig { enabled: false },
            doze: FeatureConfig { enabled: true },
            compaction: CompactionConfig {
                enabled: true,
                hardlock_ticks: 4,
            },
            watchdog: WatchdogConfig {
                enabled: false,
                jobscheduler: false,
                timeout_secs: 300,
                idle_threshold_secs: 500,
            },
            drain: FeatureConfig { enabled: false },
            notification: FeatureConfig { enabled: false },
            logging: FeatureConfig { enabled: true },
            dry_run: false,
        }
    }

    pub fn sleep_duration(&self, display_on: bool, charging: bool) -> Duration {
        if display_on || charging {
            Duration::from_secs(85)
        } else {
            Duration::from_secs(160)
        }
    }

    pub fn to_config_text(&self) -> String {
        format!(
            "\
# Utensil Poker Rust config
battery.enabled={battery_enabled}
battery.dynamic_threshold={dynamic_threshold}
battery.min_threshold={min_threshold}
battery.max_threshold={max_threshold}
battery.learn_min_points={learn_min_points}
battery.learn_max_points={learn_max_points}
battery.force_app_standby={force_app_standby}
battery.dnd_while_charging={dnd_while_charging}
battery.refresh_rate_low=
battery.refresh_rate_default=
battery.screen_timeout_low_ms=
battery.screen_timeout_default_ms=
battery.samsung_restricted_performance={samsung_restricted_performance}
battery.background_process_limit_low=
battery.background_process_limit_default=
thermal.enabled={thermal_enabled}
thermal.charging_status={thermal_charging_status}
thermal.charging_done_status={thermal_charging_done_status}
thermal.discharging_normal={thermal_discharging_normal}
thermal.discharging_low={thermal_discharging_low}
thermal.discharging_critical={thermal_discharging_critical}
thermal.critical_level={thermal_critical_level}
sensors.enabled={sensors_enabled}
airplane.enabled={airplane_enabled}
doze.enabled={doze_enabled}
compaction.enabled={compaction_enabled}
compaction.hardlock_ticks={compaction_hardlock_ticks}
watchdog.enabled={watchdog_enabled}
watchdog.jobscheduler={watchdog_jobscheduler}
watchdog.timeout_secs={watchdog_timeout_secs}
watchdog.idle_threshold_secs={watchdog_idle_threshold_secs}
drain.enabled={drain_enabled}
notification.enabled={notification_enabled}
logging.enabled={logging_enabled}
dry_run={dry_run}
path.learning_file={learning_file}
path.threshold_file={threshold_file}
path.whitelist_file={whitelist_file}
path.drain_log_file={drain_log_file}
",
            battery_enabled = self.battery.enabled,
            dynamic_threshold = self.battery.dynamic_threshold,
            min_threshold = self.battery.min_threshold,
            max_threshold = self.battery.max_threshold,
            learn_min_points = self.battery.learn_min_points,
            learn_max_points = self.battery.learn_max_points,
            force_app_standby = self.battery.force_app_standby,
            dnd_while_charging = self.battery.dnd_while_charging,
            samsung_restricted_performance = self.battery.samsung_restricted_performance,
            thermal_enabled = self.thermal.enabled,
            thermal_charging_status = self.thermal.charging_status,
            thermal_charging_done_status = self.thermal.charging_done_status,
            thermal_discharging_normal = self.thermal.discharging_normal,
            thermal_discharging_low = self.thermal.discharging_low,
            thermal_discharging_critical = self.thermal.discharging_critical,
            thermal_critical_level = self.thermal.critical_level,
            sensors_enabled = self.sensors.enabled,
            airplane_enabled = self.airplane.enabled,
            doze_enabled = self.doze.enabled,
            compaction_enabled = self.compaction.enabled,
            compaction_hardlock_ticks = self.compaction.hardlock_ticks,
            watchdog_enabled = self.watchdog.enabled,
            watchdog_jobscheduler = self.watchdog.jobscheduler,
            watchdog_timeout_secs = self.watchdog.timeout_secs,
            watchdog_idle_threshold_secs = self.watchdog.idle_threshold_secs,
            drain_enabled = self.drain.enabled,
            notification_enabled = self.notification.enabled,
            logging_enabled = self.logging.enabled,
            dry_run = self.dry_run,
            learning_file = self.paths.learning_file.display(),
            threshold_file = self.paths.threshold_file.display(),
            whitelist_file = self.paths.whitelist_file.display(),
            drain_log_file = self.paths.drain_log_file.display(),
        )
    }
}

pub fn resolve_config_path() -> PathBuf {
    std::env::var_os("UTENSIL_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH))
}

pub fn load_config(path: &Path) -> Result<UtensilConfig, ConfigError> {
    let defaults = UtensilConfig::for_config_path(path);
    seed_config_from_rom_default(path).map_err(|err| ConfigError {
        line: 0,
        message: err.to_string(),
    })?;
    match std::fs::read_to_string(path) {
        Ok(text) => parse_config_with_base(&text, defaults),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(defaults),
        Err(err) => Err(ConfigError {
            line: 0,
            message: err.to_string(),
        }),
    }
}

fn seed_config_from_rom_default(path: &Path) -> std::io::Result<()> {
    if path.exists() || path != Path::new(DEFAULT_CONFIG_PATH) {
        return Ok(());
    }
    let rom_default = Path::new(ROM_DEFAULT_CONFIG_PATH);
    if !rom_default.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(rom_default, path)?;
    Ok(())
}

pub fn parse_config(text: &str) -> Result<UtensilConfig, ConfigError> {
    parse_config_with_base(
        text,
        UtensilConfig::for_config_path(Path::new(DEFAULT_CONFIG_PATH)),
    )
}

pub fn parse_config_with_base(
    text: &str,
    mut config: UtensilConfig,
) -> Result<UtensilConfig, ConfigError> {
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(err(line_no, "expected key=value"));
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "battery.enabled" => config.battery.enabled = parse_bool(value, line_no)?,
            "battery.dynamic_threshold" => {
                config.battery.dynamic_threshold = parse_percent(value, line_no)?
            }
            "battery.min_threshold" => config.battery.min_threshold = parse_percent(value, line_no)?,
            "battery.max_threshold" => config.battery.max_threshold = parse_percent(value, line_no)?,
            "battery.learn_min_points" => {
                config.battery.learn_min_points = parse_usize(value, line_no)?
            }
            "battery.learn_max_points" => {
                config.battery.learn_max_points = parse_usize(value, line_no)?
            }
            "battery.force_app_standby" => {
                config.battery.force_app_standby = parse_bool(value, line_no)?
            }
            "battery.dnd_while_charging" => {
                config.battery.dnd_while_charging = parse_bool(value, line_no)?
            }
            "battery.refresh_rate_low" => config.battery.refresh_rate_low = parse_optional(value),
            "battery.refresh_rate_default" => {
                config.battery.refresh_rate_default = parse_optional(value)
            }
            "battery.screen_timeout_low_ms" => {
                config.battery.screen_timeout_low_ms = parse_optional_u64(value, line_no)?
            }
            "battery.screen_timeout_default_ms" => {
                config.battery.screen_timeout_default_ms = parse_optional_u64(value, line_no)?
            }
            "battery.samsung_restricted_performance" => {
                config.battery.samsung_restricted_performance = parse_bool(value, line_no)?
            }
            "battery.background_process_limit_low" => {
                config.battery.background_process_limit_low = parse_optional_i32(value, line_no)?
            }
            "battery.background_process_limit_default" => {
                config.battery.background_process_limit_default = parse_optional_i32(value, line_no)?
            }
            "thermal.enabled" => config.thermal.enabled = parse_bool(value, line_no)?,
            "thermal.charging_status" => config.thermal.charging_status = parse_percent(value, line_no)?,
            "thermal.charging_done_status" => {
                config.thermal.charging_done_status = parse_percent(value, line_no)?
            }
            "thermal.discharging_normal" => {
                config.thermal.discharging_normal = parse_percent(value, line_no)?
            }
            "thermal.discharging_low" => {
                config.thermal.discharging_low = parse_percent(value, line_no)?
            }
            "thermal.discharging_critical" => {
                config.thermal.discharging_critical = parse_percent(value, line_no)?
            }
            "thermal.critical_level" => config.thermal.critical_level = parse_percent(value, line_no)?,
            "sensors.enabled" => config.sensors.enabled = parse_bool(value, line_no)?,
            "airplane.enabled" => config.airplane.enabled = parse_bool(value, line_no)?,
            "doze.enabled" => config.doze.enabled = parse_bool(value, line_no)?,
            "compaction.enabled" => config.compaction.enabled = parse_bool(value, line_no)?,
            "compaction.hardlock_ticks" => {
                config.compaction.hardlock_ticks = parse_u32(value, line_no)?
            }
            "watchdog.enabled" => config.watchdog.enabled = parse_bool(value, line_no)?,
            "watchdog.jobscheduler" => config.watchdog.jobscheduler = parse_bool(value, line_no)?,
            "watchdog.timeout_secs" => config.watchdog.timeout_secs = parse_u64(value, line_no)?,
            "watchdog.idle_threshold_secs" => {
                config.watchdog.idle_threshold_secs = parse_u64(value, line_no)?
            }
            "drain.enabled" => config.drain.enabled = parse_bool(value, line_no)?,
            "notification.enabled" => config.notification.enabled = parse_bool(value, line_no)?,
            "logging.enabled" => config.logging.enabled = parse_bool(value, line_no)?,
            "dry_run" => config.dry_run = parse_bool(value, line_no)?,
            "path.learning_file" => config.paths.learning_file = PathBuf::from(value),
            "path.threshold_file" => config.paths.threshold_file = PathBuf::from(value),
            "path.whitelist_file" => config.paths.whitelist_file = PathBuf::from(value),
            "path.drain_log_file" => config.paths.drain_log_file = PathBuf::from(value),
            _ => return Err(err(line_no, format!("unknown key `{key}`"))),
        }
    }
    Ok(config)
}

fn parse_optional(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_optional_u64(value: &str, line: usize) -> Result<Option<u64>, ConfigError> {
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(parse_u64(value, line)?))
    }
}

fn parse_optional_i32(value: &str, line: usize) -> Result<Option<i32>, ConfigError> {
    if value.is_empty() {
        Ok(None)
    } else {
        value
            .parse::<i32>()
            .map(Some)
            .map_err(|_| err(line, "invalid signed integer"))
    }
}

fn parse_bool(value: &str, line: usize) -> Result<bool, ConfigError> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(err(line, "invalid boolean")),
    }
}

fn parse_percent(value: &str, line: usize) -> Result<u8, ConfigError> {
    let parsed = value
        .parse::<u8>()
        .map_err(|_| err(line, "invalid 0-100 value"))?;
    if parsed > 100 {
        return Err(err(line, "invalid 0-100 value"));
    }
    Ok(parsed)
}

fn parse_usize(value: &str, line: usize) -> Result<usize, ConfigError> {
    value
        .parse::<usize>()
        .map_err(|_| err(line, "invalid unsigned integer"))
}

fn parse_u32(value: &str, line: usize) -> Result<u32, ConfigError> {
    value
        .parse::<u32>()
        .map_err(|_| err(line, "invalid unsigned integer"))
}

fn parse_u64(value: &str, line: usize) -> Result<u64, ConfigError> {
    value
        .parse::<u64>()
        .map_err(|_| err(line, "invalid unsigned integer"))
}

fn err(line: usize, message: impl Into<String>) -> ConfigError {
    ConfigError {
        line,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_key_value_config() {
        let config = parse_config(
            "\
battery.dynamic_threshold=33
sensors.enabled=yes
thermal.enabled=on
path.whitelist_file=/tmp/apps.txt
",
        )
        .unwrap();
        assert_eq!(config.battery.dynamic_threshold, 33);
        assert!(config.sensors.enabled);
        assert!(config.thermal.enabled);
        assert_eq!(config.paths.whitelist_file, PathBuf::from("/tmp/apps.txt"));
    }

    #[test]
    fn rejects_unknown_keys() {
        let err = parse_config("unknown.key=true").unwrap_err();
        assert_eq!(err.line, 1);
    }

    #[test]
    fn does_not_seed_non_default_config_path() {
        let path = Path::new("/tmp/custom-utensil.conf");
        seed_config_from_rom_default(path).unwrap();
    }
}
