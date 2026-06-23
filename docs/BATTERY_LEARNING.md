# Battery Learning

Utensil adapts the low-battery threshold to your actual charging routine rather than using a fixed value.

## How It Works

Each time the device is plugged in, the current battery level is recorded to `battery_learning.dat` (one level per line). When enough samples are collected, the threshold is recomputed:

1. Sort all recorded levels
2. Trim the outer 25% (remove outliers)
3. Average the middle 50%
4. Blend with the previous threshold: `new = 0.3 × avg + 0.7 × old`
5. Clamp to `[min_threshold, max_threshold]`

The result is written to `battery_threshold.dat` and read each cycle.

## Commands

```
utensil-poker check-learning     # show current points and derived threshold
utensil-poker force-threshold N  # override threshold to N% (0–100)
utensil-poker random-learning    # seed with random data (testing)
utensil-poker clear-learning     # delete history and threshold files
utensil-poker export             # export learning data to stdout
utensil-poker import             # import learning data from stdin
```

## Config Keys

| Key | Default | Effect |
|---|---|---|
| `battery.min_threshold` | `25` | Threshold never drops below this |
| `battery.max_threshold` | `60` | Threshold never rises above this |
| `battery.learn_min_points` | `5` | Minimum samples before adapting |
| `battery.learn_max_points` | `25` | History capped at this many samples |
| `battery.dynamic_threshold` | `25` | Starting value before enough samples exist |
