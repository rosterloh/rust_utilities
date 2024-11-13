use clap::Parser;
use log::{debug, error, info};

use std::{any::Any, sync::Arc, time::Duration};
use zeroconf::prelude::*;
use zeroconf::{MdnsBrowser, ServiceDiscovery, ServiceType};

/// Example of a simple mDNS browser
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Name of the service type to browse
    #[clap(short, long, default_value = "workstation")]
    name: String,

    /// Protocol of the service type to browse
    #[clap(short, long, default_value = "tcp")]
    protocol: String,

    /// Sub-type of the service type to browse
    #[clap(short, long)]
    sub_type: Option<String>,
}

fn main() -> zeroconf::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let Args {
        name,
        protocol,
        sub_type,
    } = Args::parse();

    let sub_types: Vec<&str> = match sub_type.as_ref() {
        Some(sub_type) => vec![sub_type],
        None => vec![],
    };

    let service_type =
        ServiceType::with_sub_types(&name, &protocol, sub_types).expect("invalid service type");

    let mut browser = MdnsBrowser::new(service_type);

    browser.set_service_discovered_callback(Box::new(on_service_discovered));

    let event_loop = browser.browse_services()?;

    info!("Scanning for services...");

    loop {
        // calling `poll()` will keep this browser alive
        event_loop.poll(Duration::from_secs(0))?;
    }
}

fn on_service_discovered(
    result: zeroconf::Result<ServiceDiscovery>,
    _context: Option<Arc<dyn Any>>,
) {
    if let Ok(sd) = result {
        if sd.name().starts_with("G") {
            info!("{}", format!("{} {}", sd.name(), sd.address()));
        } else {
            debug!("{:?}", sd);
        }
    } else {
        error!("Service discovery failed");
    }
}
