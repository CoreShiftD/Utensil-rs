# utensil

Android battery and idle policy daemon — adapts system behavior based on battery level and screen state. No polling; wakes on screen property change.

```
utensil-poker daemon
utensil-poker once [--dry-run]
utensil-poker status
utensil-poker check-idle
utensil-poker check-learning
utensil-poker force-threshold 0-100
utensil-poker clear-learning
utensil-poker export
utensil-poker import
utensil-poker setup-defaults
utensil-poker uninstall-restore
utensil-poker print-default-config
```

Config: `/data/local/tmp/utensil/utensil.conf`

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Configuration](docs/CONFIGURATION.md)
- [Battery Learning](docs/BATTERY_LEARNING.md)

## Credits

Original shell implementation by **DroidCmdX** — [t.me/dcx4020](https://t.me/dcx4020).

## License

Mozilla Public License 2.0. See [LICENSE](LICENSE).
