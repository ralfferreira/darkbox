//! Nmap adapter. Scans only the ports the scope policy allows and parses XML
//! without trusting tool output size or structure.

use std::path::PathBuf;
use std::time::Duration;

use darkbox_contracts::{Capability, CapabilityState};
use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum NmapError {
    #[error("nmap binary not found")]
    Unavailable,
    #[error("invalid target: {0}")]
    InvalidTarget(String),
    #[error("invalid port list: {0}")]
    InvalidPorts(String),
    #[error("nmap timed out")]
    Timeout,
    #[error("nmap failed: {0}")]
    Failed(String),
    #[error("xml: {0}")]
    Xml(String),
    #[error("output exceeded limit")]
    OutputLimit,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NmapPort {
    pub port: u16,
    pub protocol: String,
    pub state: String,
    pub service: Option<String>,
    pub product: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NmapHost {
    pub address: String,
    pub hostname: Option<String>,
    pub ports: Vec<NmapPort>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NmapReport {
    pub hosts: Vec<NmapHost>,
    pub xml: String,
    pub version: String,
    pub command: String,
}

#[derive(Debug, Clone)]
pub struct NmapAdapter {
    binary: PathBuf,
    timeout: Duration,
    max_output_bytes: usize,
}

impl NmapAdapter {
    pub fn detect() -> Result<Self, NmapError> {
        let binary = which("nmap").ok_or(NmapError::Unavailable)?;
        Ok(Self {
            binary,
            timeout: Duration::from_secs(90),
            max_output_bytes: 8 * 1024 * 1024,
        })
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn binary(&self) -> &std::path::Path {
        &self.binary
    }

    pub fn capability() -> Capability {
        match which("nmap") {
            Some(path) => Capability {
                id: "network.nmap".into(),
                provider: "nmap".into(),
                version: version_of(&path)
                    .map(|value| {
                        value
                            .replace("Nmap version ", "")
                            .split_whitespace()
                            .next()
                            .unwrap_or("detected")
                            .to_string()
                    })
                    .unwrap_or_else(|| "detected".into()),
                state: CapabilityState::Available,
                detail: "TCP connect scan limited to policy-approved hosts and ports; XML parsed and stored as evidence".into(),
            },
            None => Capability {
                id: "network.nmap".into(),
                provider: "nmap".into(),
                version: "missing".into(),
                state: CapabilityState::Unavailable,
                detail: "nmap is not installed on the worker host".into(),
            },
        }
    }

    /// Runs a TCP connect scan. Raw socket scans are deliberately not used.
    pub async fn scan(&self, hosts: &[String], ports: &[u16]) -> Result<NmapReport, NmapError> {
        if hosts.is_empty() {
            return Err(NmapError::InvalidTarget("no hosts".into()));
        }
        for host in hosts {
            validate_host(host)?;
        }
        let mut ports: Vec<u16> = ports.iter().copied().filter(|port| *port > 0).collect();
        ports.sort_unstable();
        ports.dedup();
        if ports.is_empty() {
            return Err(NmapError::InvalidPorts("no ports".into()));
        }
        if ports.len() > 200 {
            return Err(NmapError::InvalidPorts("more than 200 ports".into()));
        }
        let port_list = ports
            .iter()
            .map(|port| port.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let targets_display = hosts.join(" ");
        let mut command = tokio::process::Command::new(&self.binary);
        command
            .arg("-sT")
            .arg("-Pn")
            .arg("-n")
            .arg("-T3")
            .arg("--max-retries")
            .arg("1")
            .arg("--host-timeout")
            .arg("25s")
            .arg("-p")
            .arg(&port_list)
            .arg("-oX")
            .arg("-")
            .args(hosts)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let child = command.spawn()?;
        let pid = child.id();
        let output = match tokio::time::timeout(self.timeout, child.wait_with_output()).await {
            Ok(result) => result?,
            Err(_) => {
                #[cfg(unix)]
                if let Some(pid) = pid {
                    unsafe {
                        libc::kill(-(pid as i32), libc::SIGKILL);
                    }
                }
                return Err(NmapError::Timeout);
            }
        };
        if output.stdout.len() > self.max_output_bytes {
            return Err(NmapError::OutputLimit);
        }
        let xml = String::from_utf8_lossy(&output.stdout).to_string();
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(NmapError::Failed(format!(
                "status {}: {}",
                output.status,
                stderr.chars().take(500).collect::<String>()
            )));
        }
        let parsed_hosts = parse_nmap_xml(&xml)?;
        let version = version_of(&self.binary).unwrap_or_else(|| "unknown".into());
        Ok(NmapReport {
            hosts: parsed_hosts,
            xml,
            version,
            command: format!(
                "nmap -sT -Pn -n -T3 --max-retries 1 --host-timeout 25s -p {} -oX - {}",
                port_list, targets_display
            ),
        })
    }
}

fn validate_host(host: &str) -> Result<(), NmapError> {
    let host = host.trim();
    if host.is_empty()
        || host.len() > 253
        || host.starts_with('-')
        || host.contains(char::is_whitespace)
        || host.contains('/')
        || host.contains('\0')
    {
        return Err(NmapError::InvalidTarget(host.into()));
    }
    Ok(())
}

pub fn parse_nmap_xml(xml: &str) -> Result<Vec<NmapHost>, NmapError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut hosts: Vec<NmapHost> = Vec::new();
    let mut current: Option<NmapHost> = None;
    let mut current_port: Option<NmapPort> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => match event.name().as_ref() {
                b"host" => {
                    current = Some(NmapHost {
                        address: String::new(),
                        hostname: None,
                        ports: Vec::new(),
                        status: "unknown".into(),
                    });
                }
                b"status" => {
                    if let Some(host) = current.as_mut()
                        && let Some(state) = attribute(&event, "state")
                    {
                        host.status = state;
                    }
                }
                b"hostname" => {
                    if let Some(host) = current.as_mut()
                        && host.hostname.is_none()
                        && let Some(name) = attribute(&event, "name")
                    {
                        host.hostname = Some(name);
                    }
                }
                b"address" => {
                    if let Some(host) = current.as_mut()
                        && (attribute(&event, "addrtype").as_deref() == Some("ipv4")
                            || attribute(&event, "addrtype").as_deref() == Some("ipv6"))
                        && let Some(address) = attribute(&event, "addr")
                    {
                        host.address = address;
                    }
                }
                b"port" => {
                    let port = attribute(&event, "portid")
                        .and_then(|value| value.parse::<u16>().ok())
                        .unwrap_or(0);
                    current_port = Some(NmapPort {
                        port,
                        protocol: attribute(&event, "protocol").unwrap_or_else(|| "tcp".into()),
                        state: "unknown".into(),
                        service: None,
                        product: None,
                        version: None,
                    });
                }
                b"state" => {
                    if let Some(port) = current_port.as_mut()
                        && let Some(state) = attribute(&event, "state")
                    {
                        port.state = state;
                    }
                }
                b"service" => {
                    if let Some(port) = current_port.as_mut() {
                        port.service = attribute(&event, "name");
                        port.product = attribute(&event, "product");
                        port.version = attribute(&event, "version");
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(event)) => match event.name().as_ref() {
                b"status" => {
                    if let Some(host) = current.as_mut()
                        && let Some(state) = attribute(&event, "state")
                    {
                        host.status = state;
                    }
                }
                b"hostname" => {
                    if let Some(host) = current.as_mut()
                        && host.hostname.is_none()
                        && let Some(name) = attribute(&event, "name")
                    {
                        host.hostname = Some(name);
                    }
                }
                b"address" => {
                    if let Some(host) = current.as_mut()
                        && (attribute(&event, "addrtype").as_deref() == Some("ipv4")
                            || attribute(&event, "addrtype").as_deref() == Some("ipv6"))
                        && let Some(address) = attribute(&event, "addr")
                    {
                        host.address = address;
                    }
                }
                b"state" => {
                    if let Some(port) = current_port.as_mut()
                        && let Some(state) = attribute(&event, "state")
                    {
                        port.state = state;
                    }
                }
                b"service" => {
                    if let Some(port) = current_port.as_mut() {
                        port.service = attribute(&event, "name");
                        port.product = attribute(&event, "product");
                        port.version = attribute(&event, "version");
                    }
                }
                _ => {}
            },
            Ok(Event::End(event)) => match event.name().as_ref() {
                b"port" => {
                    if let (Some(host), Some(port)) = (current.as_mut(), current_port.take())
                        && port.port > 0
                    {
                        host.ports.push(port);
                    }
                }
                b"host" => {
                    if let Some(host) = current.take()
                        && !host.address.is_empty()
                    {
                        hosts.push(host);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(error) => return Err(NmapError::Xml(error.to_string())),
            _ => {}
        }
    }
    Ok(hosts)
}

fn attribute(event: &quick_xml::events::BytesStart<'_>, name: &str) -> Option<String> {
    event
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.as_ref() == name.as_bytes())
        .and_then(|attribute| attribute.unescape_value().ok())
        .map(|value| value.into_owned())
}

fn which(binary: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
}

fn version_of(binary: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().find(|line| line.contains("Nmap version"))?;
    Some(first.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<?xml version="1.0"?>
<nmaprun scanner="nmap">
  <host starttime="1" endtime="2">
    <status state="up" reason="user-set"/>
    <address addr="127.0.0.1" addrtype="ipv4"/>
    <hostnames><hostname name="lab.local" type="user"/></hostnames>
    <ports>
      <port protocol="tcp" portid="8080">
        <state state="open" reason="syn-ack"/>
        <service name="http" product="Synthetic Lab" version="1.0"/>
      </port>
      <port protocol="tcp" portid="9099">
        <state state="closed" reason="conn-refused"/>
      </port>
      <extraports state="filtered" count="1"/>
    </ports>
  </host>
</nmaprun>"#;

    #[test]
    fn parses_open_ports_and_services() {
        let hosts = parse_nmap_xml(FIXTURE).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].address, "127.0.0.1");
        assert_eq!(hosts[0].hostname.as_deref(), Some("lab.local"));
        assert_eq!(hosts[0].ports.len(), 2);
        assert_eq!(hosts[0].ports[0].port, 8080);
        assert_eq!(hosts[0].ports[0].state, "open");
        assert_eq!(hosts[0].ports[0].product.as_deref(), Some("Synthetic Lab"));
    }

    #[test]
    fn rejects_option_injection() {
        assert!(validate_host("-iL").is_err());
        assert!(validate_host("a b").is_err());
        assert!(validate_host("example.com").is_ok());
    }

    #[test]
    fn reports_capability_without_panicking_when_missing() {
        let capability = NmapAdapter::capability();
        assert_eq!(capability.id, "network.nmap");
        assert!(matches!(
            capability.state,
            CapabilityState::Available | CapabilityState::Unavailable
        ));
    }
}
