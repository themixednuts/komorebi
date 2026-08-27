use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::thread;
use uds_windows::{UnixListener, UnixStream};

fn probe_path() -> PathBuf {
    std::env::args_os().nth(1).map_or_else(
        || std::env::temp_dir().join(format!("komorebi-uds-probe-{}.sock", std::process::id())),
        PathBuf::from,
    )
}

fn exchange(path: &PathBuf) -> io::Result<()> {
    let listener = UnixListener::bind(path)?;
    let accepting_listener = listener.try_clone()?;
    let accepting = thread::spawn(move || -> io::Result<()> {
        let (mut stream, _) = accepting_listener.accept()?;
        let mut request = [0_u8; 4];
        stream.read_exact(&mut request)?;
        if request != *b"ping" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected request",
            ));
        }
        stream.write_all(b"pong")
    });

    let mut client = UnixStream::connect(path)?;
    client.write_all(b"ping")?;
    let mut response = [0_u8; 4];
    client.read_exact(&mut response)?;
    if response != *b"pong" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected response",
        ));
    }

    accepting
        .join()
        .map_err(|_| io::Error::other("accept thread panicked"))??;
    drop(client);
    drop(listener);
    Ok(())
}

fn query_manager(data_dir: &std::path::Path) -> io::Result<usize> {
    let mut stream = UnixStream::connect(data_dir.join("komorebi.sock"))?;
    stream.write_all(br#"{"type":"State"}"#)?;
    stream.shutdown(std::net::Shutdown::Write)?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    Ok(response.len())
}

fn main() -> io::Result<()> {
    let path = probe_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
    }

    let exchange_result = exchange(&path);
    let cleanup_result = std::fs::remove_file(&path);
    let data_dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let query_result = query_manager(data_dir);
    let report = format!(
        "path={}\ncwd={:?}\nlocalappdata={:?}\nuserprofile={:?}\nexchange={exchange_result:?}\ncleanup={cleanup_result:?}\nquery={query_result:?}\n",
        path.display(),
        std::env::current_dir(),
        std::env::var_os("LOCALAPPDATA"),
        std::env::var_os("USERPROFILE"),
    );
    print!("{report}");
    if let Some(output_path) = std::env::args_os().nth(2) {
        std::fs::write(output_path, report)?;
    }
    exchange_result
}
