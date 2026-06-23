pub mod config;
pub mod learning;
pub mod runtime;

use config::{ConfigError, UtensilConfig, load_config, resolve_config_path};
use learning::{
    clear_learning, export_learning, force_threshold, import_learning, print_learning,
    print_whitelist, random_learning,
};
use runtime::{RealCommandRunner, Runtime};
use std::io;
use std::path::Path;

pub fn run() -> Result<(), AppError> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "help".to_string());
    let config_path = resolve_config_path();

    match command.as_str() {
        "daemon" => {
            let config = load_config_from_path(&config_path)?;
            let mut runtime = Runtime::new(config, RealCommandRunner);
            runtime.run_daemon()?;
        }
        "once" => {
            let dry_run = args.any(|arg| arg == "--dry-run");
            let mut config = load_config_from_path(&config_path)?;
            config.dry_run = dry_run || config.dry_run;
            let mut runtime = Runtime::new(config, RealCommandRunner);
            runtime.run_once()?;
        }
        "status" => {
            let config = load_config_from_path(&config_path)?;
            let mut runtime = Runtime::new(config, RealCommandRunner);
            runtime.print_status();
        }
        "force-threshold" | "forcepush" => {
            let value = args
                .next()
                .ok_or_else(|| AppError::Usage("missing threshold value".to_string()))?;
            let value = parse_percent(&value)?;
            let config = load_config_from_path(&config_path)?;
            force_threshold(&config.paths.threshold_file, value)?;
            println!("battery_threshold has been overridden");
        }
        "random-learning" | "stringsfile" => {
            let config = load_config_from_path(&config_path)?;
            random_learning(&config.paths.learning_file)?;
            println!("data learning has been overridden");
        }
        "clear-learning" | "clearlearningdata" => {
            let config = load_config_from_path(&config_path)?;
            clear_learning(&config.paths.learning_file, &config.paths.threshold_file)?;
            println!("data cleanup is finished");
        }
        "check-learning" | "checkdatalearning" => {
            let config = load_config_from_path(&config_path)?;
            print_learning(&config.paths.learning_file)?;
        }
        "check-whitelist" | "checkwhitelist" => {
            let config = load_config_from_path(&config_path)?;
            print_whitelist(&config.paths.whitelist_file)?;
        }
        "check-idle" | "checkidle" => {
            let config = load_config_from_path(&config_path)?;
            let mut runtime = Runtime::new(config, RealCommandRunner);
            runtime.print_idle_history()?;
        }
        "export" => {
            let config = load_config_from_path(&config_path)?;
            export_learning(&config.paths.learning_file, &config.paths.threshold_file)?;
            println!("data learning saved at /sdcard/");
        }
        "import" => {
            let config = load_config_from_path(&config_path)?;
            import_learning(&config.paths.learning_file, &config.paths.threshold_file)?;
            println!("data files has been imported");
        }
        "setup-defaults" => {
            let config = load_config_from_path(&config_path)?;
            let mut runtime = Runtime::new(config, RealCommandRunner);
            runtime.setup_defaults()?;
        }
        "uninstall-restore" => {
            let config = load_config_from_path(&config_path)?;
            let mut runtime = Runtime::new(config, RealCommandRunner);
            runtime.uninstall_restore();
        }
        "print-default-config" => {
            let config = UtensilConfig::for_config_path(&config_path);
            print!("{}", config.to_config_text());
        }
        "help" | "-h" | "--help" => print_help(),
        _ => return Err(AppError::Usage(format!("unknown command `{command}`"))),
    }

    Ok(())
}

fn load_config_from_path(path: &Path) -> Result<UtensilConfig, AppError> {
    Ok(load_config(path)?)
}

fn parse_percent(value: &str) -> Result<u8, AppError> {
    let value = value
        .parse::<u8>()
        .map_err(|_| AppError::Usage("threshold must be 0-100".to_string()))?;
    if value > 100 {
        return Err(AppError::Usage("threshold must be 0-100".to_string()));
    }
    Ok(value)
}

fn print_help() {
    println!("Utensil Poker Rust");
    println!();
    println!("commands:");
    println!("  daemon");
    println!("  once [--dry-run]");
    println!("  status");
    println!("  force-threshold 0-100");
    println!("  random-learning");
    println!("  clear-learning");
    println!("  check-learning");
    println!("  check-whitelist");
    println!("  check-idle");
    println!("  export");
    println!("  import");
    println!("  setup-defaults");
    println!("  uninstall-restore");
    println!("  print-default-config");
}

#[derive(Debug)]
pub enum AppError {
    Io(io::Error),
    Config(ConfigError),
    Usage(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Config(err) => write!(f, "{err}"),
            Self::Usage(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<io::Error> for AppError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ConfigError> for AppError {
    fn from(value: ConfigError) -> Self {
        Self::Config(value)
    }
}
