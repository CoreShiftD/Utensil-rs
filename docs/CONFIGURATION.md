# Configuration

Config file: `/data/local/tmp/utensil/utensil.conf` — `key=value` format.  
ROM default: `/system/etc/utensil/utensil.conf` (seeded on first run if present).

Print current defaults:

```
utensil-poker print-default-config
```

## Battery

| Key | Default | Description |
|---|---|---|
| `battery.enabled` | `true` | Enable battery policy |
| `battery.dynamic_threshold` | `25` | Initial low-battery threshold (%) |
| `battery.min_threshold` | `25` | Learning floor |
| `battery.max_threshold` | `60` | Learning ceiling |
| `battery.learn_min_points` | `5` | Min charge points before threshold adapts |
| `battery.learn_max_points` | `25` | Max learning history kept |
| `battery.force_app_standby` | `false` | Also set `forced_app_standby_enabled` |
| `battery.dnd_while_charging` | `false` | Enable DND (alarms-only) while charging |
| `battery.refresh_rate_low` | *(unset)* | Refresh rate string to apply at low battery |
| `battery.refresh_rate_default` | *(unset)* | Refresh rate string to restore above threshold |
| `battery.screen_timeout_low_ms` | *(unset)* | Screen timeout ms to apply at low battery |
| `battery.screen_timeout_default_ms` | *(unset)* | Screen timeout ms to restore above threshold |
| `battery.samsung_restricted_performance` | `false` | Set `restricted_device_performance` on Samsung |
| `battery.background_process_limit_low` | `2` | Background process limit at low battery |
| `battery.background_process_limit_default` | *(unset)* | Background process limit above threshold |

## Thermal

| Key | Default | Description |
|---|---|---|
| `thermal.enabled` | `false` | Override thermalservice status |
| `thermal.charging_status` | `1` | Status while charging below max threshold |
| `thermal.charging_done_status` | `0` | Status while charging above max threshold |
| `thermal.discharging_normal` | `1` | Status while discharging above threshold |
| `thermal.discharging_low` | `2` | Status while discharging at/below threshold |
| `thermal.discharging_critical` | `3` | Status while discharging at/below critical |
| `thermal.critical_level` | `20` | Level (%) considered critical |

## Features

| Key | Default | Description |
|---|---|---|
| `sensors.enabled` | `false` | Restrict sensorservice at low battery |
| `airplane.enabled` | `false` | Airplane mode during low-battery charging (keeps WiFi) |
| `doze.enabled` | `true` | Force deep doze on screen-off (Android 12+) |
| `drain.enabled` | `false` | Track drain rate and publish to `debug.dcx.drain` |
| `notification.enabled` | `false` | Post Android notifications for key events |
| `logging.enabled` | `true` | Log to drain log file |
| `dry_run` | `false` | Probe state and log without applying changes |

## Compaction

| Key | Default | Description |
|---|---|---|
| `compaction.enabled` | `true` | Run `am compact` on screen-off (Android 12+) |
| `compaction.hardlock_ticks` | `4` | Screen-off cycles before compacting |

## Watchdog

| Key | Default | Description |
|---|---|---|
| `watchdog.enabled` | `false` | Force-stop apps idle longer than timeout |
| `watchdog.jobscheduler` | `false` | Also cancel pending JobScheduler jobs |
| `watchdog.timeout_secs` | `300` | Seconds before a background app is stopped |
| `watchdog.idle_threshold_secs` | `500` | Seconds screen-off before jobscheduler fires |

## Paths

| Key | Default | Description |
|---|---|---|
| `path.learning_file` | `<home>/battery_learning.dat` | Charge-point history |
| `path.threshold_file` | `<home>/battery_threshold.dat` | Derived threshold |
| `path.whitelist_file` | `<home>/fileconfig.txt` | Packages exempt from watchdog |
| `path.drain_log_file` | `/sdcard/chargelog.txt` | Log output file |
| `path.packages_xml` | `/data/system/packages.xml` | Android package registry |

## Runtime Files

| Path | Description |
|---|---|
| `<home>/battery_learning.dat` | Charge-level history used to derive threshold |
| `<home>/battery_threshold.dat` | Current derived threshold |
| `<home>/screentimeout.dat` | Backup of original screen timeout |
| `<home>/has_high_rr` | Marker: device has high refresh rate display |
| `<home>/default_rr_val` | Backup of original peak refresh rate |
| `debug.dcx.drain` | System property: `X%/hr\|on:Ym\|off:Zm\|dur:Ws\|24h:V%` |
| `debug.dcx.screenstate` | System property: screen state fallback |
| `debug.dcx.batterylevel` | System property: last seen battery level |
| `debug.dcx.chargestate` | System property: last seen charge state |
