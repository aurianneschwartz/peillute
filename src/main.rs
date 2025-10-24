//! Peillute - A distributed financial transaction system
//!
//! This module serves as the main entry point for the Peillute application, handling both
//! server and client-side functionality. The application supports distributed transactions
//! with vector clock synchronization and peer-to-peer communication.

#![allow(non_snake_case)]

mod clock;
mod control;
mod db;
mod message;
mod network;
mod snapshot;
mod state;
mod utils;

/// Command-line arguments for configuring the Peillute application
#[derive(clap::Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Unique identifier for this site in the network
    #[arg(long, default_value_t = String::new())]
    cli_site_id: String,

    /// Port number for peer-to-peer communication
    #[arg(long, default_value_t = 0)]
    cli_port: u16,

    /// List of peer addresses to connect to
    #[arg(long, value_delimiter = ',')]
    cli_peers: Vec<String>,

    /// IP address to bind to
    #[arg(long, default_value_t = String::from("127.0.0.1"))]
    cli_ip: String,

    /// ID for the batabase path
    #[arg(long, default_value_t = 0)]
    cli_db_id: u16,
}

#[cfg(feature = "server")]
#[tokio::main]
async fn main() -> rusqlite::Result<(), Box<dyn std::error::Error>> {
    use crate::state::LOCAL_APP_STATE;
    use clap::Parser;
    use std::io::{self as std_io, Write};
    use std::net::SocketAddr;
    use tokio::io::{self as tokio_io, AsyncBufReadExt, BufReader};
    use tokio::net::TcpListener;

    const LOW_PORT: u16 = 10000;
    const HIGH_PORT: u16 = 11000;
    const PORT_OFFSET: u16 = HIGH_PORT - LOW_PORT + 1;

    if !db::is_database_initialized()? {
        let _ = db::init_db();
    }

    control::control_worker();
    tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
    .init();

    let args = Args::parse();

    let port_range = LOW_PORT..=HIGH_PORT;
    let selected_port = if args.cli_port == 0 {
        port_range
            .into_iter()
            .find(|port| std::net::TcpListener::bind(("127.0.0.1", *port)).is_ok())
            .unwrap_or(LOW_PORT)
    } else {
        args.cli_port
    };

    let final_site_addr: SocketAddr = format!("{}:{}", &args.cli_ip, selected_port).parse()?;
    let client_server_interaction_addr: SocketAddr =
        format!("{}:{}", &args.cli_ip, selected_port + PORT_OFFSET).parse()?;

    let final_cli_peers_addrs: Vec<SocketAddr> = args
        .cli_peers
        .into_iter()
        .filter_map(|peer| peer.parse::<SocketAddr>().ok())
        .collect();

    let (final_site_id, final_clock, needs_sync) = match utils::reload_existing_site().await {
        Ok((site_id_from_db, clock_from_db)) => (site_id_from_db, clock_from_db, true),
        Err(_) => {
            let generated_site_id = if args.cli_site_id.is_empty() {
                utils::get_mac_address().unwrap_or_default() + "_" + &std::process::id().to_string()
            } else {
                args.cli_site_id.clone()
            };
            (generated_site_id, crate::clock::Clock::new(), false)
        }
    };

    {
        let mut state = LOCAL_APP_STATE.lock().await;
        state.init_site_id(final_site_id.clone());
        state.init_site_addr(final_site_addr);
        state.init_clock(final_clock);
        state.init_parent_addr_for_transaction_wave();
        state.init_cli_peer_addrs(final_cli_peers_addrs);
        state.init_sync(needs_sync);
    }

    // Create the network listener
    let network_listener_local_addr = final_site_addr.clone();
    let listener: TcpListener = TcpListener::bind(network_listener_local_addr).await?;
    tracing::debug!("Listening on: {}", network_listener_local_addr);

    // Create the web app listener
    let router = axum::Router::new().serve_dioxus_application(ServeConfigBuilder::default(), App);
    let router = router.into_make_service();
    let backend_listener = tokio::net::TcpListener::bind(client_server_interaction_addr)
        .await
        .unwrap();

    // Create the stdin listener for the CLI
    let stdin: tokio_io::Stdin = tokio_io::stdin();
    let reader: BufReader<tokio_io::Stdin> = BufReader::new(stdin);
    let mut lines: tokio_io::Lines<_> = reader.lines();

    // Announce our presence to the network
    network::announce(&args.cli_ip, LOW_PORT, HIGH_PORT, selected_port).await;

    println!(
        "\n\
        ===================================================\n\
            💰  Welcome to Peillute! 💰\n\
        ===================================================\n\
        \n\
            📌 Write /help to get the command list.\n\
            🌐 Access the web interface at: http://{}\n\
        ===================================================\n\
        ",
        client_server_interaction_addr
    );
    print!("> ");
    std_io::stdout().flush().unwrap();

    let main_loop_app_state = LOCAL_APP_STATE.clone();

    // Spawn the web server
    let server_task = tokio::spawn(async move {
        axum::serve(backend_listener, router).await.unwrap();
    });

    main_loop(main_loop_app_state, &mut lines, listener).await;

    // Ensure the server task finishes cleanly if ever reached
    server_task.await?;

    Ok(())
}

#[cfg(feature = "server")]
async fn main_loop(
    _state: std::sync::Arc<tokio::sync::Mutex<crate::state::AppState>>,
    lines: &mut tokio::io::Lines<tokio::io::BufReader<tokio::io::Stdin>>,
    listener: tokio::net::TcpListener,
) {
    use crate::control::{parse_command, process_cli_command};
    use std::io::{self as std_io, Write};
    use tokio::select;

    loop {
        select! {
            line = lines.next_line() => {
                let command = parse_command(line);
                if let Err(e) = process_cli_command(command).await{
                    tracing::error!("Error handling a cli command:\n{}", e);
                }
                print!("> ");
                std_io::stdout().flush().unwrap();
            }
            Ok((stream, addr)) = listener.accept() => {
                let _ = crate::network::start_listening(stream, addr).await;
            }
            _ = tokio::signal::ctrl_c() => {
                disconnect().await;
                std::process::exit(0);
            }
        }
    }
}

#[cfg(feature = "server")]
async fn disconnect() {
    use crate::message::{MessageInfo, NetworkMessageCode};
    use crate::state::LOCAL_APP_STATE;
    use tracing::{error, info};

    let (local_addr, site_id, connected_nei_addr) = {
        let state = LOCAL_APP_STATE.lock().await;
        (
            state.get_site_addr(),
            state.get_site_id().to_string(),
            state.get_connected_nei_addr(),
        )
    };

    info!("Shutting down site {}.", site_id);
    for peer_addr in connected_nei_addr {
        // increment the clock for every deconnection
        let clock = {
            let mut state = LOCAL_APP_STATE.lock().await;
            state.update_clock(None).await;
            state.get_clock().clone()
        };

        if let Err(e) = crate::network::send_message(
            peer_addr,
            MessageInfo::None,
            None,
            NetworkMessageCode::Disconnect,
            local_addr,
            &site_id,
            &site_id,
            local_addr,
            clock.clone(),
        )
        .await
        {
            error!("Error sending message to {}: {}", peer_addr, e);
        }
    }
}

use dioxus::prelude::*;

#[cfg(not(feature = "server"))]
fn main() {
    dioxus::launch(App);
}

mod views;
use views::*;

const FAVICON: Asset = asset!("/assets/icon.png");
const MAIN_CSS: Asset = asset!("/assets/styling/main.css");

/// Main application component that sets up the web interface
#[component]
fn App() -> Element {
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }

        Router::<Route> {}
    }
}

/// Defines the routing structure for the web application
#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[layout(Navbar)]
        #[route("/")]
        Home {},
        #[route("/info")]
        Info {},
        #[nest("/:name")]
        #[layout(User)]
            #[route("/history")]
            History {
                name: String,
            },
            #[route("/withdraw")]
            Withdraw {
                name: String,
            },
            #[route("/pay")]
            Pay {
                name: String,
            },
            #[route("/refund")]
            Refund {
                name: String,
            },
            #[route("/transfer")]
            Transfer {
                name: String,
            },
            #[route("/deposit")]
            Deposit {
                name: String,
            },
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    #[test]
    fn test_args_parsing() {
        use super::Args;
        let args = Args::parse_from(vec![
            "my_program",
            "--cli-site-id",
            "A",
            "--cli-port",
            "8080",
            "--cli-peers",
            "127.0.0.1:8081,127.0.0.1:8082",
        ]);
        assert_eq!(args.cli_site_id, "A");
        assert_eq!(args.cli_port, 8080);
        assert_eq!(args.cli_peers.len(), 2);
        assert_eq!(args.cli_peers[0], "127.0.0.1:8081");
        assert_eq!(args.cli_peers[1], "127.0.0.1:8082");
    }

    #[test]
    fn test_args_parsing_no_peers() {
        use super::Args;
        let args = Args::parse_from(vec![
            "my_program",
            "--cli-site-id",
            "A",
            "--cli-port",
            "8080",
        ]);
        assert_eq!(args.cli_site_id, "A");
        assert_eq!(args.cli_port, 8080);
        assert_eq!(args.cli_peers.len(), 0);
    }
}
