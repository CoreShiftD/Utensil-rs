use coreshift_core::process::{
    close_fds_from, fork, redirect_fd_to, redirect_stdio_to_devnull, set_pdeathsig, setpgid,
    setsid, ForkResult,
};
use coreshift_core::signal::{signal_ignore, SIGHUP, SIGPIPE, SIGTERM};
use coreshift_core::spawn::{ExitStatus, Process};
use std::fs;
use std::time::{Duration, Instant};

const HOME: &str = "/data/local/tmp/utensil";
const PID_FILE: &str = "/data/local/tmp/utensil/daemon.pid";
const LOG_FILE: &str = "/data/local/tmp/utensil/daemon.log";

fn main() {
    let first_arg = std::env::args().nth(1);
    if first_arg.as_deref() == Some("daemon") {
        if let Err(err) = run_supervisor() {
            coreshift_core::alog_error!("utensil-poker", "{err}");
            std::process::exit(1);
        }
    } else if let Err(err) = utensil::run() {
        coreshift_core::alog_error!("utensil-poker", "{err}");
        std::process::exit(1);
    }
}

fn run_supervisor() -> Result<(), Box<dyn std::error::Error>> {
    let _ = fs::create_dir_all(HOME);

    // Double-fork: CLI parent returns once middle child exits
    match unsafe { fork()? } {
        ForkResult::Parent(pid) => {
            let _ = Process::new(pid).wait_blocking();
            return Ok(());
        }
        ForkResult::Child => {}
    }

    // Middle child
    let _ = setsid();
    let _ = setpgid(0, 0);

    match unsafe { fork()? } {
        ForkResult::Parent(_) => std::process::exit(0),
        ForkResult::Child => {}
    }

    // Grandchild: supervisor loop adopted by init
    unsafe {
        let _ = redirect_stdio_to_devnull();
    }

    let mut crash_count: u64 = 0;
    let mut last_crash_window = Instant::now();

    loop {
        match unsafe { fork()? } {
            ForkResult::Parent(daemon_pid) => {
                let _ = fs::write(PID_FILE, daemon_pid.to_string());
                let process = Process::new(daemon_pid);
                let status = process.wait_blocking();
                let _ = fs::remove_file(PID_FILE);

                if let Ok(ExitStatus::Exited(0)) = status {
                    std::process::exit(0);
                }

                crash_count += 1;
                if last_crash_window.elapsed() > Duration::from_secs(10) {
                    crash_count = 1;
                    last_crash_window = Instant::now();
                }

                if crash_count >= 5 {
                    eprintln!("utensil-poker: crashed 5 times in 10s, giving up");
                    std::process::exit(1);
                }

                std::thread::sleep(Duration::from_millis(500 * crash_count));
            }
            ForkResult::Child => {
                let _ = set_pdeathsig(SIGTERM);
                unsafe {
                    signal_ignore(SIGHUP);
                    signal_ignore(SIGPIPE);
                }
                close_fds_from(3);

                if let Ok(f) = fs::OpenOptions::new().create(true).append(true).open(LOG_FILE) {
                    use std::os::unix::io::IntoRawFd;
                    unsafe { redirect_fd_to(f.into_raw_fd(), 2) };
                }

                if let Err(err) = utensil::run() {
                    coreshift_core::alog_error!("utensil-poker", "{err}");
                    std::process::exit(1);
                }
                std::process::exit(0);
            }
        }
    }
}
