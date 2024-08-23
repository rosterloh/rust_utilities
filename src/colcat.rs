// use anyhow::Result;
use core::panic;
// use core::result::Result;
use std::{
    env, io::Read, io::Result, io::Write, os::unix::net::UnixStream, path::Path, time::Duration,
};

static DEFAULT_SOCKET_PATH: &'static str = "/var/run/collectd-unixsock";

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let socket_path: String;

    if args.len() < 2 {
        socket_path = DEFAULT_SOCKET_PATH.to_string();
    } else {
        socket_path = args[1].clone();
    }

    let socket = Path::new(&socket_path);

    if !socket.exists() {
        panic!("No file found at {}", socket_path);
    }

    println!("Connecting to socket");
    let mut stream = match UnixStream::connect(&socket) {
        Err(_) => panic!("Could not connect to socket at {}", socket_path),
        Ok(stream) => stream,
    };

    // Set read write timeout for socket
    stream.set_read_timeout(Some(Duration::new(2, 0)))?;
    stream.set_write_timeout(Some(Duration::new(2, 0)))?;

    println!("Requsting list of all values");
    stream.write_all(b"LISTVAL\n")?;

    //let mut response = String::new();
    let mut response: Vec<u8> = vec![0; 10];
    //stream.read_to_string(&mut response)?;
    // stream.read_to_end(&mut response)?;
    stream.read_exact(&mut response)?;

    let response_string = String::from_utf8_lossy(&response).to_string();
    println!("{response_string}");

    Ok(())
}
