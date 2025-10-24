#[cfg(feature = "server")]
pub fn get_mac_address() -> Option<String> {
    use pnet::datalink;
    let interfaces = datalink::interfaces();
    for iface in interfaces {
        // Ignore loopback et interfaces sans MAC
        if iface.is_up() && !iface.is_loopback() {
            if let Some(mac) = iface.mac {
                if mac.octets() != [0, 0, 0, 0, 0, 0] {
                    return Some(mac.to_string().replace(":", ""));
                }
            }
        }
    }
    None
}
#[cfg(feature = "server")]
pub async fn reload_existing_site() -> Result<(String, crate::clock::Clock), String> {
    use tracing::info;
    match crate::db::get_local_state() {
        Ok((site_id, clock)) => {
            info!("Existing site state reloaded");
            Ok((site_id, clock))
        }
        Err(e) => {
            info!("No existing site state found, creating a new one.");
            Err(format!("Failed to reload existing site: {}", e))
        }
    }
}
