use super::{CmdResult, StringifyErr as _};
use crate::{
    config::{Config, IProfiles, IVerge, PrfItem},
    core::{CoreManager, handle, service, tray::Tray},
    feat,
    utils::dirs,
};
use anyhow::{Context as _, Result, anyhow, bail};
use clash_verge_logging::{Type, logging};
use serde::Serialize;
use smartstring::alias::String;
use std::collections::HashMap;
use tauri_plugin_clash_verge_sysinfo::is_current_app_handle_admin;
use tokio::fs;

const MANAGED_PROFILE_UID: &str = "kaif-vpnchik-managed";
const MANAGED_PROFILE_FILE: &str = "kaif-vpnchik.yaml";
const MANAGED_PROFILE_NAME: &str = "kaif vpnchik";
const WIREGUARD_NODE_NAME: &str = "WireGuard_Node";

#[derive(Debug, Clone, PartialEq, Eq)]
struct WireGuardConfig {
    private_key: std::string::String,
    ip: std::string::String,
    dns: Vec<std::string::String>,
    public_key: std::string::String,
    pre_shared_key: Option<std::string::String>,
    allowed_ips: Option<Vec<std::string::String>>,
    server: std::string::String,
    port: u16,
    mtu: u16,
    persistent_keepalive: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireGuardImportResult {
    pub profile_uid: String,
    pub profile_name: String,
    pub server: String,
    pub port: u16,
    pub replaced_existing: bool,
    pub warnings: Vec<String>,
}

#[tauri::command]
pub async fn import_wireguard_conf(
    file_data: std::string::String,
    file_name: Option<std::string::String>,
) -> CmdResult<WireGuardImportResult> {
    let config = parse_wireguard_config(&file_data)
        .stringify_err_log(|err| logging!(warn, Type::Cmd, "WireGuard import failed: {err}"))?;
    let yaml = generate_clash_yaml(&config).stringify_err()?;
    let replaced_existing = save_managed_profile(yaml, file_name.as_deref()).await.stringify_err()?;

    let patch = IProfiles {
        current: Some(MANAGED_PROFILE_UID.into()),
        items: None,
    };
    super::profile::patch_profiles_config(patch).await?;

    Ok(WireGuardImportResult {
        profile_uid: MANAGED_PROFILE_UID.into(),
        profile_name: MANAGED_PROFILE_NAME.into(),
        server: config.server.into(),
        port: config.port,
        replaced_existing,
        warnings: vec![],
    })
}

async fn save_managed_profile(yaml: std::string::String, file_name: Option<&str>) -> Result<bool> {
    let profiles_dir = dirs::app_profiles_dir()?;
    fs::create_dir_all(&profiles_dir).await?;
    fs::write(profiles_dir.join(MANAGED_PROFILE_FILE), yaml.as_bytes())
        .await
        .context("failed to save generated Clash profile")?;

    let source_desc = file_name
        .filter(|name| !name.trim().is_empty())
        .map(|name| format!("Imported from {name}"))
        .unwrap_or_else(|| "Imported WireGuard config".into());

    let item = PrfItem {
        uid: Some(MANAGED_PROFILE_UID.into()),
        itype: Some("local".into()),
        name: Some(MANAGED_PROFILE_NAME.into()),
        file: Some(MANAGED_PROFILE_FILE.into()),
        desc: Some(source_desc.into()),
        updated: Some(chrono::Local::now().timestamp() as usize),
        ..PrfItem::default()
    };

    let replaced_existing = Config::profiles()
        .await
        .with_data_modify(move |mut profiles| async move {
            let replaced = {
                let items = profiles.items.get_or_insert_with(Vec::new);
                let before_len = items.len();
                items.retain(|item| {
                    item.uid.as_deref() != Some(MANAGED_PROFILE_UID)
                        && item.file.as_deref() != Some(MANAGED_PROFILE_FILE)
                });
                let replaced = before_len != items.len();
                items.push(item);
                replaced
            };
            profiles.current = Some(MANAGED_PROFILE_UID.into());
            profiles.save_file().await?;
            Ok((profiles, replaced))
        })
        .await?;

    if let Err(e) = CoreManager::global().update_config().await {
        logging!(warn, Type::Cmd, "Failed to refresh config after WireGuard import: {e}");
    }
    handle::Handle::refresh_clash();
    handle::Handle::notify_profile_changed(&MANAGED_PROFILE_UID.into());
    if let Err(e) = Tray::global().update_part().await {
        logging!(warn, Type::Tray, "Failed to update tray after WireGuard import: {e}");
    }

    Ok(replaced_existing)
}

pub async fn apply_kaif_vpn_enabled(enable: bool) -> Result<bool> {
    let has_profile = Config::profiles()
        .await
        .latest_arc()
        .get_item(MANAGED_PROFILE_UID)
        .is_ok();
    if enable && !has_profile {
        bail!("Import a WireGuard .conf file first");
    }
    if enable
        && !is_current_app_handle_admin(handle::Handle::app_handle())
        && service::is_service_available().await.is_err()
    {
        bail!(
            "TUN mode requires the helper service. Please approve the macOS helper installation prompt and try again."
        );
    }

    let patch = if enable {
        IVerge {
            enable_system_proxy: Some(false),
            enable_tun_mode: Some(true),
            ..IVerge::default()
        }
    } else {
        IVerge {
            enable_tun_mode: Some(false),
            ..IVerge::default()
        }
    };
    feat::patch_verge(&patch, false).await?;
    handle::Handle::refresh_verge();
    Ok(enable)
}

#[tauri::command]
pub async fn set_kaif_vpn_enabled(enable: bool) -> CmdResult<bool> {
    apply_kaif_vpn_enabled(enable).await.stringify_err()
}

fn parse_wireguard_config(content: &str) -> Result<WireGuardConfig> {
    let sections = parse_sections(content);
    let interface = sections
        .get("interface")
        .ok_or_else(|| anyhow!("Missing [Interface] section"))?;
    let peer_sections = count_sections(content, "peer");
    if peer_sections != 1 {
        bail!("Only WireGuard configs with exactly one [Peer] section are supported");
    }
    let peer = sections.get("peer").ok_or_else(|| anyhow!("Missing [Peer] section"))?;

    let private_key = required(interface, "privatekey")?;
    let public_key = required(peer, "publickey")?;
    let address = required(interface, "address")?;
    let ip = parse_ipv4_address(&address)?;
    let endpoint = required(peer, "endpoint")?;
    let (server, port) = parse_endpoint(&endpoint)?;
    let mtu = optional(interface, "mtu")
        .map(|raw| parse_u16(&raw, "MTU"))
        .transpose()?
        .unwrap_or(1420);
    let dns = optional(interface, "dns")
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|dns| !dns.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|dns| !dns.is_empty())
        .unwrap_or_else(|| vec!["1.1.1.1".to_owned()]);
    let persistent_keepalive = optional(peer, "persistentkeepalive")
        .map(|raw| parse_u16(&raw, "PersistentKeepalive"))
        .transpose()?;
    let allowed_ips = optional(peer, "allowedips").map(|raw| {
        raw.split(',')
            .map(str::trim)
            .filter(|ip| !ip.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    });
    let pre_shared_key = optional(peer, "presharedkey");

    Ok(WireGuardConfig {
        private_key,
        ip,
        dns,
        public_key,
        pre_shared_key,
        allowed_ips,
        server,
        port,
        mtu,
        persistent_keepalive,
    })
}

fn count_sections(content: &str, section_name: &str) -> usize {
    content
        .lines()
        .filter(|line| line.trim().eq_ignore_ascii_case(&format!("[{section_name}]")))
        .count()
}

fn parse_sections(content: &str) -> HashMap<std::string::String, HashMap<std::string::String, std::string::String>> {
    let mut sections = HashMap::new();
    let mut current_section: Option<std::string::String> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(section) = parse_section_header(line) {
            current_section = Some(section.clone());
            sections.entry(section).or_insert_with(HashMap::new);
            continue;
        }
        if let (Some(section), Some((key, value))) = (current_section.as_ref(), parse_key_value(line)) {
            sections
                .entry(section.clone())
                .or_insert_with(HashMap::new)
                .insert(key, value);
        }
    }

    sections
}

fn parse_section_header(line: &str) -> Option<std::string::String> {
    let name = line.strip_prefix('[')?.strip_suffix(']')?.trim();
    if name.chars().all(|ch| ch.is_ascii_alphabetic()) {
        Some(name.to_ascii_lowercase())
    } else {
        None
    }
}

fn parse_key_value(line: &str) -> Option<(std::string::String, std::string::String)> {
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() || !key.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return None;
    }
    Some((key.to_ascii_lowercase(), value.trim().to_owned()))
}

fn required(map: &HashMap<std::string::String, std::string::String>, key: &str) -> Result<std::string::String> {
    optional(map, key).ok_or_else(|| anyhow!("Missing required WireGuard field `{key}`"))
}

fn optional(map: &HashMap<std::string::String, std::string::String>, key: &str) -> Option<std::string::String> {
    map.get(key)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn parse_ipv4_address(address: &str) -> Result<std::string::String> {
    address
        .split(',')
        .map(str::trim)
        .filter_map(|part| part.split('/').next())
        .find(|ip| ip.contains('.') && ip.parse::<std::net::Ipv4Addr>().is_ok())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("Address must contain an IPv4 address"))
}

fn parse_endpoint(endpoint: &str) -> Result<(std::string::String, u16)> {
    if let Some(rest) = endpoint.strip_prefix('[') {
        let (host, port_part) = rest
            .split_once("]:")
            .ok_or_else(|| anyhow!("Endpoint IPv6 address must use [addr]:port format"))?;
        return Ok((host.to_owned(), parse_u16(port_part, "Endpoint port")?));
    }
    let (host, port_part) = endpoint
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("Endpoint must be in host:port format"))?;
    if host.is_empty() || port_part.is_empty() {
        bail!("Endpoint must be in host:port format");
    }
    Ok((host.to_owned(), parse_u16(port_part, "Endpoint port")?))
}

fn parse_u16(value: &str, field: &str) -> Result<u16> {
    value
        .trim()
        .parse::<u16>()
        .with_context(|| format!("{field} must be a number between 0 and 65535"))
}

fn generate_clash_yaml(config: &WireGuardConfig) -> Result<std::string::String> {
    #[derive(Serialize)]
    #[serde(rename_all = "kebab-case")]
    struct Dns {
        enable: bool,
        listen: &'static str,
        enhanced_mode: &'static str,
        fake_ip_range: &'static str,
        nameserver: Vec<&'static str>,
        default_nameserver: Vec<&'static str>,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "kebab-case")]
    struct Peer<'a> {
        server: &'a str,
        port: u16,
        public_key: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        pre_shared_key: Option<&'a str>,
        allowed_ips: &'a [std::string::String],
    }

    #[derive(Serialize)]
    #[serde(rename_all = "kebab-case")]
    struct Proxy<'a> {
        name: &'static str,
        #[serde(rename = "type")]
        proxy_type: &'static str,
        ip: &'a str,
        private_key: &'a str,
        peers: Vec<Peer<'a>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        persistent_keepalive: Option<u16>,
        mtu: u16,
        udp: bool,
        remote_dns_resolve: bool,
        dns: &'a [std::string::String],
    }

    #[derive(Serialize)]
    #[serde(rename_all = "kebab-case")]
    struct ClashConfig<'a> {
        port: u16,
        socks_port: u16,
        allow_lan: bool,
        mode: &'static str,
        log_level: &'static str,
        ipv6: bool,
        dns: Dns,
        proxies: Vec<Proxy<'a>>,
        rules: Vec<&'static str>,
    }

    let default_allowed_ips = vec!["0.0.0.0/0".to_owned()];
    let allowed_ips = config.allowed_ips.as_deref().unwrap_or(default_allowed_ips.as_slice());

    let clash_config = ClashConfig {
        port: 7890,
        socks_port: 7891,
        allow_lan: false,
        mode: "rule",
        log_level: "info",
        ipv6: false,
        dns: Dns {
            enable: true,
            listen: "0.0.0.0:1053",
            enhanced_mode: "fake-ip",
            fake_ip_range: "198.18.0.1/16",
            nameserver: vec!["8.8.8.8", "1.1.1.1"],
            default_nameserver: vec!["223.5.5.5", "77.88.8.8"],
        },
        proxies: vec![Proxy {
            name: WIREGUARD_NODE_NAME,
            proxy_type: "wireguard",
            ip: &config.ip,
            private_key: &config.private_key,
            peers: vec![Peer {
                server: &config.server,
                port: config.port,
                public_key: &config.public_key,
                pre_shared_key: config.pre_shared_key.as_deref(),
                allowed_ips,
            }],
            persistent_keepalive: config.persistent_keepalive,
            mtu: config.mtu,
            udp: true,
            remote_dns_resolve: true,
            dns: &config.dns,
        }],
        rules: vec![
            "DOMAIN-SUFFIX,ru,DIRECT",
            "DOMAIN-SUFFIX,su,DIRECT",
            "DOMAIN-SUFFIX,xn--p1ai,DIRECT",
            "GEOIP,ru,DIRECT",
            "MATCH,WireGuard_Node",
        ],
    };

    serde_yaml_ng::to_string(&clash_config).context("failed to serialize Clash profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r"
[Interface]
PrivateKey = private=
Address = 10.0.0.37/32
DNS = 1.1.1.1

[Peer]
PublicKey = public=
AllowedIPs = 0.0.0.0/0
Endpoint = 91.197.2.188:51820
PersistentKeepalive = 21
";

    fn parse_sample_err(input: &str) -> Result<anyhow::Error> {
        match parse_wireguard_config(input) {
            Ok(config) => bail!("WireGuard sample should fail, parsed instead: {config:?}"),
            Err(err) => Ok(err),
        }
    }

    #[test]
    fn parses_sample_config() -> Result<()> {
        let parsed = parse_wireguard_config(SAMPLE)?;
        assert_eq!(parsed.ip, "10.0.0.37");
        assert_eq!(parsed.server, "91.197.2.188");
        assert_eq!(parsed.port, 51820);
        assert_eq!(parsed.mtu, 1420);
        assert_eq!(parsed.persistent_keepalive, Some(21));
        assert_eq!(parsed.dns, vec!["1.1.1.1"]);
        Ok(())
    }

    #[test]
    fn parses_crlf_and_spacing() -> Result<()> {
        let parsed = parse_wireguard_config(&SAMPLE.replace('\n', "\r\n"))?;
        assert_eq!(parsed.public_key, "public=");
        Ok(())
    }

    #[test]
    fn rejects_missing_required_fields_without_key_leak() -> Result<()> {
        let err = parse_sample_err("[Interface]\nPrivateKey = secret\n[Peer]\n")?;
        let msg = err.to_string();
        assert!(msg.contains("Missing required WireGuard field"));
        assert!(!msg.contains("secret"));
        Ok(())
    }

    #[test]
    fn rejects_multi_peer_config() -> Result<()> {
        let err = parse_sample_err(&format!("{SAMPLE}\n[Peer]\nPublicKey = another\n"))?;
        assert!(err.to_string().contains("exactly one [Peer]"));
        Ok(())
    }

    #[test]
    fn rejects_bad_endpoint() -> Result<()> {
        let err = parse_sample_err(&SAMPLE.replace("91.197.2.188:51820", "91.197.2.188"))?;
        assert!(err.to_string().contains("Endpoint"));
        Ok(())
    }

    #[test]
    fn generated_yaml_contains_expected_rules() -> Result<()> {
        let parsed = parse_wireguard_config(SAMPLE)?;
        let yaml = generate_clash_yaml(&parsed)?;
        assert!(yaml.contains("name: WireGuard_Node"));
        assert!(yaml.contains("peers:"));
        assert!(yaml.contains("remote-dns-resolve: true"));
        assert!(yaml.contains("- DOMAIN-SUFFIX,ru,DIRECT"));
        assert!(yaml.contains("- MATCH,WireGuard_Node"));
        assert!(!yaml.contains("[Interface]"));
        Ok(())
    }
}
