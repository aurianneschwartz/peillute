#[cfg(feature = "server")]
pub fn get_mac_address() -> Option<String> {
    use pnet::datalink;
    use tracing::debug;
    debug!("Searching for valid network interface MAC address");
    let interfaces = datalink::interfaces();
    for iface in interfaces {
        // Ignore loopback et interfaces sans MAC
        if iface.is_up() && !iface.is_loopback() {
            if let Some(mac) = iface.mac {
                if mac.octets() != [0, 0, 0, 0, 0, 0] {
                    debug!(
                        interface = %iface.name,
                        mac_address = %mac,
                        "Found valid MAC address"
                    );
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
            info!(
                site_id = %site_id,
                lamport_clock = clock.get_lamport(),
                peers_known = clock.get_vector_clock_map().len(),
                "Site state reloaded successfully"
            );
            Ok((site_id, clock))
        }
        Err(e) => {
            info!(
                reason = "no_previous_state",
                action = "will_create_new",
                "No existing site state, initializing new site"
            );
            Err(format!("Failed to reload existing site: {}", e))
        }
    }
}
