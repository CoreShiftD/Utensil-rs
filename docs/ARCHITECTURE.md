# Architecture

Utensil is an Android battery and idle policy daemon. It monitors device state — battery level, screen, charging — and applies system adjustments to reduce drain and background activity.

## Components

### Library (`src/lib.rs`)
Public entry point exposing `utensil::run()` for the `utensil-poker` binary.

### Runtime (`src/runtime.rs`)
Core daemon logic. Owns all state and drives the main cycle:

```
wait_for_boot_completed()
loop {
    probe_state()          // battery level, screen, charging, Android version
    apply_cycle()          // all policy decisions
    wait_for_next_cycle()  // backoff sleep or property_wait on screen state prop
}
```

State is split into sub-structs:

| Struct | Purpose |
|---|---|
| `PreviousState` | Dedup — skip redundant writes |
| `DrainState` | Hourly + 24h drain tracking |
| `CompactionState` | Screen-off tick counter for compaction |
| `WatchdogState` | Per-package idle timers, job scheduler state |

### Config (`src/config.rs`)
Parsed from `/data/local/tmp/utensil/utensil.conf`. All features are independently toggleable. See [CONFIGURATION.md](CONFIGURATION.md).

### Learning (`src/learning.rs`)
Records charge-end battery levels. Derives a dynamic threshold via trimmed-mean smoothing. Threshold is written to `battery_threshold.dat` and read back each cycle.

## Sleep / Wakeup

`wait_for_next_cycle` uses `android_property_wait` on `debug.tracing.screen_state` when available — wakes immediately on screen state change. Backoff:

- **Screen off**: exponential 60 → 120 → 240 → 320s cap
- **Screen on / charging**: linear 85 + step×5s, cap 6 steps, resets on screen-off

## Package Cache

`enabled_third_party_packages` parses `/data/system/packages.xml` once and caches the result. Re-parsed only when mtime changes — no `pm list packages` subprocess on the hot path.

## Running User Packages

`running_user_packages` reads PIDs from `/dev/cpuset/top-app/cgroup.procs` and `/dev/cpuset/foreground/cgroup.procs`. UID is resolved via `stat /proc/<pid>` (file owner = process UID). No `ps` subprocess.

## Foreground Package

Queries `coreshift-foreground` daemon over `@coreshift` abstract socket (`status` command). Parsed from `foreground: <pkg>` response line.

## Credits

Original shell implementation by **DroidCmdX** — [t.me/dcx4020](https://t.me/dcx4020).
