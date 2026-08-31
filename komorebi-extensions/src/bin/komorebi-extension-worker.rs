fn main() {
    let code = komorebi_extensions::run_worker_containment_probe().map_or_else(
        komorebi_extensions::WorkerContainmentFailure::exit_code,
        |()| 0,
    );
    std::process::exit(code);
}
