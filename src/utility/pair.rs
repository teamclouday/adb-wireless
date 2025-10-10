use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use rand::Rng;
use std::net::Ipv4Addr;

use super::error::CliError;

const SERVICE_TYPE_PAIRING: &str = "_adb-tls-pairing._tcp.local.";
const SERVICE_TYPE_CONNECT: &str = "_adb-tls-connect._tcp.local.";

pub struct PairService {
    pub service_name: String,
    pub password: String,
    mdns: ServiceDaemon,
}

pub struct DeviceInfo {
    pub address: Ipv4Addr,
    pub pairing_port: u16,
    pub debugging_port: u16,
}

fn random_number_string(length: usize) -> String {
    let mut rng = rand::rng();
    (0..length)
        .map(|_| rng.random_range(0..10).to_string())
        .collect()
}

impl PairService {
    pub fn new() -> Result<Self, CliError> {
        let mdns = ServiceDaemon::new()?;

        let service_name = format!("adb-wireless-{}", random_number_string(6));
        let password = random_number_string(8);

        Ok(Self {
            service_name,
            password,
            mdns,
        })
    }

    pub fn qrtext(&self) -> String {
        format!("WIFI:T:ADB;S:{};P:{};;", self.service_name, self.password)
    }

    pub fn start_discovery(&self) -> Result<(), CliError> {
        let service_info = ServiceInfo::new(
            SERVICE_TYPE_PAIRING,
            &self.service_name,
            &format!("{}.local.", self.service_name),
            "",
            0,
            None,
        )?;

        self.mdns.register(service_info)?;

        Ok(())
    }

    pub fn wait_for_pairing(&self) -> Result<DeviceInfo, CliError> {
        // first browse for pairing service
        let receiver = self.mdns.browse(SERVICE_TYPE_PAIRING)?;

        let (address, pairing_port) = loop {
            match receiver.recv() {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    if info.get_fullname().contains(&self.service_name) {
                        let client_addresses = info.get_addresses_v4();
                        let port = info.get_port();

                        let _ = self.mdns.stop_browse(SERVICE_TYPE_PAIRING);
                        if let Some(addr) = client_addresses
                            .iter()
                            .filter(|addr| addr.is_private())
                            .next()
                        {
                            break (**addr, port);
                        } else {
                            return Err(CliError::MdnsError(mdns_sd::Error::Msg(
                                "No client address found".to_string(),
                            )));
                        }
                    }
                }
                Ok(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
                Err(err) => {
                    let _ = self.mdns.stop_browse(SERVICE_TYPE_PAIRING);
                    return Err(CliError::UnexpectedError(err.to_string()));
                }
            }
        };

        // then browse for connect service
        let receiver = self.mdns.browse(SERVICE_TYPE_CONNECT)?;

        let start_time = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(15);

        let debugging_port = loop {
            if start_time.elapsed() > timeout {
                return Err(CliError::MdnsError(mdns_sd::Error::Msg(
                    "Cannot find device to connect. Please check if wireless debugging is enabled."
                        .to_string(),
                )));
            }

            match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    let port = info.get_port();

                    if info.get_addresses_v4().iter().any(|&&addr| addr == address) {
                        let _ = self.mdns.stop_browse(SERVICE_TYPE_CONNECT);
                        break port;
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(100))
                    }
                }
                Ok(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
                Err(flume::RecvTimeoutError::Timeout) => continue,
                Err(err) => {
                    let _ = self.mdns.stop_browse(SERVICE_TYPE_CONNECT);
                    return Err(CliError::UnexpectedError(err.to_string()));
                }
            }
        };

        Ok(DeviceInfo {
            address,
            pairing_port,
            debugging_port,
        })
    }
}

impl Drop for PairService {
    fn drop(&mut self) {
        let _ = self.mdns.stop_browse(SERVICE_TYPE_PAIRING);
        let _ = self.mdns.stop_browse(SERVICE_TYPE_CONNECT);
        let _ = self.mdns.unregister(&self.service_name);
    }
}
