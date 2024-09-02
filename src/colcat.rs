use core::panic;
use std::{
    env, io::BufRead, io::BufReader, io::Result, io::Write, os::unix::net::UnixStream, path::Path,
    time::Duration,
};

static DEFAULT_SOCKET_PATH: &'static str = "/var/run/collectd-unixsock";

fn read_from_socket(reader: &mut BufReader<UnixStream>) -> Vec<String> {
    let mut results: Vec<String> = vec![];

    let mut response = String::new();
    let count = reader.read_line(&mut response).unwrap();
    if count > 0 {
        let mut num_values: u32 = 0;
        if let Some((first, _)) = response.split_once(" ") {
            num_values = first.parse().unwrap();
        }

        for _ in 0..num_values {
            let mut response = String::new();
            reader.read_line(&mut response).unwrap();
            let values: Vec<&str> = response.split_whitespace().collect();
            if values.len() == 1 {
                // Result of GETVAL
                results.push(values[0].to_string());
            } else {
                // Result of LISTVAL
                let mut tmp = values[1].to_string();
                if tmp.ends_with("\n") {
                    tmp.pop();
                }
                results.push(tmp);
            }
        }
    }

    // TODO: check len results == num_values
    results
}

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

    // let mut response: Vec<u8> = vec![0; 10];
    // stream.read_exact(&mut response)?;
    // let response_string = String::from_utf8_lossy(&response).to_string();
    // println!("{response_string}");

    // let mut response = String::new();
    let mut reader = BufReader::new(stream.try_clone().unwrap());

    let results = read_from_socket(&mut reader);
    // println!("{:?}", results);
    if results.len() > 0 {
        for metric in results {
            let get_val = format!("GETVAL {metric}\n");
            stream.write_all(get_val.as_bytes())?;

            let metric_data = read_from_socket(&mut reader);
            println!("{metric}: {:?}", metric_data);
        }
    } else {
        println!("No metrics found");
    }

    Ok(())
}
