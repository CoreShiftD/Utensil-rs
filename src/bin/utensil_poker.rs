fn main() {
    if let Err(err) = utensil::run() {
        coreshift_core::alog_error!("utensil-poker", "{err}");
        std::process::exit(1);
    }
}
