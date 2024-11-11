use core::panic;
use std::{
    env, io::BufRead, io::BufReader, io::Write, os::unix::net::UnixStream, path::PathBuf,
    time::Duration,
};
// use std::str::FromStr;

static DEFAULT_SOCKET_PATH: &'static str = "/var/run/collectd-unixsock";

#[allow(dead_code)]
#[derive(Debug)]
struct Metric {
    host: String,
    plugin: String,
    plugin_instances: Vec<String>,
    values: Vec<f64>,
}

// impl FromStr for Metric {
//     type Err = std::num::ParseIntError;

//     fn from_str(input: &str) -> Result<Self, Self::Err> {
//         let metric_parts: Vec<&str> = input.split("/").collect();
//         if metric_parts.len() != 3 {
//             return Err("Could not extract arguments");
//         }

//         let host = metric_parts[0].to_string();
//         let plugin = metric_parts[1].to_string();
//         let plugin_instance = vec![metric_parts[2].to_string()];
//         let values = vec![];

//         Ok(Metric {
//             host,
//             plugin,
//             plugin_instance,
//             values,
//         })
//     }
// }

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

    // todo!(check len results == num_values)
    results
}

struct AppArgs {
    socket_path: String,
    socket: PathBuf,
}

impl AppArgs {
    fn build(args: &[String]) -> Result<AppArgs, &'static str> {
        let socket_path;

        if args.len() < 2 {
            socket_path = DEFAULT_SOCKET_PATH.to_string();
        } else {
            socket_path = args[1].clone();
        }

        let socket = PathBuf::from(&socket_path);

        if !socket.exists() {
            return Err("No file found at path");
        }

        Ok(AppArgs {
            socket_path,
            socket,
        })
    }
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    let app_args = AppArgs::build(&args).unwrap_or_else(|err| {
        println!("Problem parsing arguments: {err}");
        std::process::exit(1);
    });

    println!("Connecting to socket");
    let mut stream = match UnixStream::connect(app_args.socket) {
        Err(_) => panic!("Could not connect to socket at {}", app_args.socket_path),
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
        let mut metrics: Vec<Metric> = vec![];
        for metric in results {
            let get_val = format!("GETVAL {metric}\n");
            stream.write_all(get_val.as_bytes())?;

            let metric_data = read_from_socket(&mut reader);
            // println!("{metric}: {:?}", metric_data);

            let metric_parts: Vec<&str> = metric.split("/").collect();
            if metric_parts.len() != 3 {
                continue;
            }

            let host = metric_parts[0].to_string();
            let plugin = metric_parts[1].to_string();
            let plugin_instance = metric_parts[2].to_string();
            let value: f64 = metric_data[0].split("=").last().unwrap().parse().unwrap();

            if let Some(m) = metrics.iter_mut().find(|x| x.plugin == plugin) {
                m.plugin_instances.push(plugin_instance);
                m.values.push(value);
            } else {
                metrics.push(Metric {
                    host,
                    plugin,
                    plugin_instances: vec![plugin_instance],
                    values: vec![value],
                })
            }

            println!("{:#?}", metrics);
        }
    } else {
        println!("No metrics found");
    }

    Ok(())
}
