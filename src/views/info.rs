//! System information component for the Peillute application
//!
//! This module provides a component for displaying system-wide information,
//! including network details, logical clock states, and peer connections.

use dioxus::prelude::*;

/// Server function to retrieve the local network address
#[server]
async fn get_local_addr() -> Result<String, ServerFnError> {
    use crate::state::LOCAL_APP_STATE;
    let state = LOCAL_APP_STATE.lock().await;
    Ok(state.get_site_addr_as_string())
}

/// Server function to retrieve the current site ID
#[server]
async fn get_site_id() -> Result<String, ServerFnError> {
    use crate::state::LOCAL_APP_STATE;
    let state = LOCAL_APP_STATE.lock().await;
    Ok(state.get_site_id().to_string())
}

/// Server function to retrieve the list of connected peers
#[server]
async fn get_peers() -> Result<Vec<String>, ServerFnError> {
    use crate::state::LOCAL_APP_STATE;
    let state = LOCAL_APP_STATE.lock().await;
    Ok(state.get_cli_peers_addrs_as_string())
}

/// Server function to retrieve the current Lamport clock value
#[server]
async fn get_lamport() -> Result<i64, ServerFnError> {
    use crate::state::LOCAL_APP_STATE;
    let state = LOCAL_APP_STATE.lock().await;
    Ok(*state.get_clock().get_lamport())
}

/// Server function to retrieve the current vector clock state
#[server]
async fn get_vector_clock() -> Result<String, ServerFnError> {
    use crate::state::LOCAL_APP_STATE;
    let state = LOCAL_APP_STATE.lock().await;
    let vector_clock = state.get_clock().get_vector_clock_values();
    let vector_clock_string = vector_clock
        .iter()
        .map(|x| x.to_string())
        .collect::<Vec<String>>()
        .join(", ");
    Ok(vector_clock_string)
}

/// Server function to retrieve the database path
#[server]
async fn get_db_path() -> Result<String, ServerFnError> {
    let conn = crate::db::DB_CONN.lock().unwrap();
    let path = conn.path().unwrap();
    //keep only the name of the file (after the last "/")
    Ok(path.to_string().split("/").last().unwrap().to_string())
}

/// Server function to retrieve the number of neighbours in the network
#[server]
async fn get_nb_connected_neighbours() -> Result<i64, ServerFnError> {
    use crate::state::LOCAL_APP_STATE;
    let state = LOCAL_APP_STATE.lock().await;
    Ok(state.get_nb_connected_neighbours())
}

/// Server function to retrieve the number of cli peers
#[server]
async fn get_nb_cli_peers() -> Result<i64, ServerFnError> {
    use crate::state::LOCAL_APP_STATE;
    let state = LOCAL_APP_STATE.lock().await;
    Ok(state.get_cli_peers_addrs().len() as i64)
}

/// Server function to retrieve the list of connected neighbours
#[server]
async fn get_connected_neighbours() -> Result<Vec<String>, ServerFnError> {
    use crate::state::LOCAL_APP_STATE;
    let state = LOCAL_APP_STATE.lock().await;
    Ok(state.get_connected_nei_addr_string())
}

/// Server function to retrieve the list of peer addresses
#[server]
async fn get_peer_addrs() -> Result<Vec<String>, ServerFnError> {
    use crate::state::LOCAL_APP_STATE;
    let state = LOCAL_APP_STATE.lock().await;
    Ok(state.get_cli_peers_addrs_as_string())
}

/// Ask for a snapshot
#[server]
async fn ask_for_snapshot() -> Result<(), ServerFnError> {
    if let Err(e) =
        crate::control::enqueue_critical(crate::control::CriticalCommands::FileSnapshot).await
    {
        return Err(ServerFnError::new(format!(
            "[SERVER] Failed make the local snapshot: {e}"
        )));
    }
    Ok(())
}

/// Get the latest snapshot content if any
#[server]
async fn get_snapshot_content() -> Result<Option<String>, ServerFnError> {
    use crate::snapshot::LOCAL_SNAPSHOT_MANAGER;
    use tokio::fs::File;
    use tokio::io::AsyncReadExt;

    let maybe_filename = {
        let state = LOCAL_SNAPSHOT_MANAGER.lock().await;
        state.path.clone()
    };

    if let Some(filename) = maybe_filename {
        let mut file = File::open(&filename).await?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).await?;
        Ok(Some(contents))
    } else {
        Ok(None)
    }
}

/// System information component
///
/// Displays real-time information about the distributed system, including:
/// - Database info
/// - Local network address
/// - Site ID
/// - Lamport timestamp
/// - Vector clock state
/// - Number of connected sites
/// - List of connected peers
/// - Snapshot button
#[component]
pub fn Info() -> Element {
    let mut local_addr = use_signal(|| "".to_string());
    let mut site_id = use_signal(|| "".to_string());
    let mut peers_addr = use_signal(|| Vec::new());
    let mut connected_neighbours = use_signal(|| Vec::new());
    let mut lamport = use_signal(|| 0i64);
    let mut vector_clock = use_signal(|| "".to_string());
    let mut nb_neighbours = use_signal(|| 0i64);
    let mut nb_peers = use_signal(|| 0i64);
    let mut db_path = use_signal(|| "".to_string());
    let mut snapshot_content = use_signal(|| None::<String>);

    use_future(move || async move {
        // Fetch local address
        if let Ok(data) = get_local_addr().await {
            local_addr.set(data);
        } else {
            // Optional: Handle error, e.g., log or set a default error message
            local_addr.set("Error fetching local address".to_string());
        }

        // Fetch site ID
        if let Ok(data) = get_site_id().await {
            site_id.set(data);
        } else {
            site_id.set("Error fetching site ID".to_string());
        }

        // Fetch peers
        if let Ok(data) = get_peers().await {
            peers_addr.set(data);
        } // else: peers remains empty or you could set an error state if needed

        // Fetch connected neighbours
        if let Ok(data) = get_connected_neighbours().await {
            connected_neighbours.set(data);
        } // else: connected_neighbours remains empty or you could set an error state if needed

        // Fetch Lamport clock
        if let Ok(data) = get_lamport().await {
            lamport.set(data);
        } // else: lamport remains 0 or handle error

        // Fetch vector clock (example value)
        if let Ok(data) = get_vector_clock().await {
            vector_clock.set(data);
        } // else: vector_clock remains 0 or handle error

        // Fetch number of sites
        if let Ok(data) = get_nb_connected_neighbours().await {
            nb_neighbours.set(data);
        } // else: nb_sites remains 0 or handle error

        // Fetch number of CLI peers
        if let Ok(data) = get_nb_cli_peers().await {
            nb_peers.set(data);
        } // else : nb_peers remains 0 or handle error

        // Fetch database path
        if let Ok(data) = get_db_path().await {
            db_path.set(data);
        } // else: db_path remains "" or handle error

        // Fetch snapshot content
        if let Ok(data) = get_snapshot_content().await {
            snapshot_content.set(data);
        } // else: snapshot_content remains None or handle error
    });

    rsx! {
        div { class: "info-panel", // You can style this class with CSS
            h2 { "System Information" }

            div { class: "info-item",
                strong { "💾 Database : " }
                span { "{db_path}" }
            }

            div { class: "info-item",
                strong { "🌐 Site Address: " }
                span { "{local_addr}" }
            }
            div { class: "info-item",
                strong { "🆔 Site ID: " }
                span { "{site_id}" }
            }
            div { class: "info-item",
                strong { "⏰ Lamport Timestamp: " }
                span { "{lamport}" }
            }
            div { class: "info-item",
                strong { "⏱️ Vector Clock : " }
                span { "{vector_clock}" }
            }
            div { class: "info-item",
                strong { "🌍 Number of connected neighbours: " }
                span { "{nb_neighbours}" }
            }

            div { class: "info-item",
                strong { "🤝 Connected Neighbours: " }
                if connected_neighbours.read().is_empty() {
                    span { "No peers currently connected." }
                } else {
                    ul { class: "peer-list",
                        for adr in connected_neighbours.read().iter() {
                            li { key: "{adr}", "{adr}" }
                        }
                    }
                }
            }

            div { class: "info-item",
                strong { "🌍 Number of CLI peers: " }
                span { "{nb_peers}" }
            }

            div { class: "info-item",
                strong { "🤝 CLI peers adresses: " }
                if peers_addr.read().is_empty() {
                    span { "No peers currently connected." }
                } else {
                    ul { class: "peer-list",
                        for adr in peers_addr.read().iter() {
                            li { key: "{adr}", "{adr}" }
                        }
                    }
                }
            }

            div {
                class: "info-item",
                style: "display: flex; justify-content: center;",
                form {
                    button {
                        class: "snapshot",
                        r#type: "button",
                        onclick: move |_| {
                            async move {
                                if let Err(e) = ask_for_snapshot().await {
                                    tracing::error!("Error taking snapshot: {e}");
                                }
                            }
                        },
                        "Take a snapshot"
                    }
                }
            }

            div { class: "info-item",
                strong { "📄 Last Snapshot Content:" }
                if let Some(content) = snapshot_content.read().as_ref() {
                    pre { "{content}" }
                } else {
                    span { "No snapshot available." }
                }
            }
        }
    }
}
