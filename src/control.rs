//! Command handling and CLI interface
//!
//! This module provides the command-line interface and command handling functionality
//! for the Peillute application, including both local and network command processing.

#![cfg(feature = "server")]
/// Worker that handles critical commands
pub fn control_worker() {
    tokio::spawn(async {
        use crate::state::LOCAL_APP_STATE;

        loop {
            // Récupérer Notify sans garder le verrou
            let notify = {
                let st = LOCAL_APP_STATE.lock().await;
                st.notify_sc.clone()
            };

            // réveille dès qu'on a la section critique
            notify.notified().await;

            // Vider la file de tsx en attente
            {
                let (in_st, waiting, nb_pending) = {
                    let st = LOCAL_APP_STATE.lock().await;
                    (st.in_sc, st.waiting_sc, st.pending_commands.len())
                };

                if !waiting && nb_pending > 0 && !in_st {
                    let mut st = LOCAL_APP_STATE.lock().await;
                    let _ = st.acquire_mutex().await;
                    continue;
                }

                if in_st && nb_pending > 0 {
                    tracing::info!("Début de la section critique");
                    loop {
                        let cmd_opt = {
                            let mut st = LOCAL_APP_STATE.lock().await;
                            st.pending_commands.pop_front()
                        };
                        if let Some(cmd) = cmd_opt {
                            tracing::info!("Execute critical command");
                            if let Err(e) = crate::control::execute_critical(cmd).await {
                                tracing::error!("Erreur exécution commande critique : {}", e);
                            }
                        } else {
                            break;
                        }
                    }
                    tracing::info!("Fin de la section critique");
                }
            }
        }
    });
}

#[cfg(feature = "server")]
/// Parse a line of input from the CLI and converts it to a Command
pub fn parse_command(line: Result<Option<String>, std::io::Error>) -> Command {
    match line {
        Ok(Some(cmd)) => {
            let command = match cmd.trim() {
                "/create_user" => Command::CreateUser,
                "/user_accounts" => Command::UserAccounts,
                "/print_user_tsx" => Command::PrintUserTransactions,
                "/print_tsx" => Command::PrintTransactions,
                "/deposit" => Command::Deposit,
                "/withdraw" => Command::Withdraw,
                "/transfer" => Command::Transfer,
                "/pay" => Command::Pay,
                "/refund" => Command::Refund,
                "/help" => Command::Help,
                "/info" => Command::Info,
                "/start_snapshot" => Command::Snapshot,
                other => Command::Unknown(other.to_string()),
            };
            command
        }
        Ok(None) => {
            println!("Aucun input");
            Command::Unknown("Aucun input".to_string())
        }
        Err(e) => {
            tracing::error!("Erreur de lecture stdin : {}", e);
            Command::Error("Erreur de lecture stdin".to_string())
        }
    }
}

#[cfg(feature = "server")]
/// Available commands in the system
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub enum Command {
    /// Create a new user account
    CreateUser,
    /// List all user accounts
    UserAccounts,
    /// Display transactions for a specific user
    PrintUserTransactions,
    /// Display all system transactions
    PrintTransactions,
    /// Deposit money into an account
    Deposit,
    /// Withdraw money from an account
    Withdraw,
    /// Transfer money between accounts
    Transfer,
    /// Make a payment
    Pay,
    /// Process a refund
    Refund,
    /// Display help information
    Help,
    /// Display system information
    Info,
    /// Unknown command
    Unknown(String),
    /// Error command
    Error(String),
    /// Start a system snapshot
    Snapshot,
}

#[cfg(feature = "server")]
/// Critical commands that can be executed on our site
#[derive(Debug, Clone, PartialEq)]
pub enum CriticalCommands {
    /// Create a new user account
    CreateUser { name: String },
    /// Deposit money into an account
    Deposit { name: String, amount: f64 },
    /// Withdraw money from an account
    Withdraw { name: String, amount: f64 },
    /// Transfer money between accounts
    Transfer {
        from: String,
        to: String,
        amount: f64,
    },
    /// Make a payment
    Pay { name: String, amount: f64 },
    /// Process a refund
    Refund {
        name: String,
        lamport: i64,
        node: String,
    },
    /// Request a snapshot to save as a JSON
    FileSnapshot,
    /// Request a snapshot to update our database
    SyncSnapshot,
}

#[cfg(feature = "server")]
/// Enqueue a critical command
pub async fn enqueue_critical(cmd: CriticalCommands) -> Result<(), Box<dyn std::error::Error>> {
    use crate::state::LOCAL_APP_STATE;
    let mut st = LOCAL_APP_STATE.lock().await;

    st.pending_commands.push_back(cmd);

    // si on n’est ni en SC ni déjà en attente → on déclenche la vague

    tracing::debug!("is in sc {}", !st.in_sc);
    tracing::debug!("is waiting {}", !st.waiting_sc);

    if !st.in_sc && !st.waiting_sc {
        st.acquire_mutex().await?;
    }
    Ok(())
}

#[cfg(feature = "server")]
/// Execute a critical command on our site
///
/// Called by the control worker only when the Mutex is acquired
pub async fn execute_critical(cmd: CriticalCommands) -> Result<(), Box<dyn std::error::Error>> {
    use crate::message::{Message, MessageInfo, NetworkMessageCode};
    use crate::network::diffuse_message;
    use crate::state::LOCAL_APP_STATE;

    let (clock, site_addr, site_id) = {
        let mut state = LOCAL_APP_STATE.lock().await;
        let local_addr = state.get_site_addr();
        let node = state.get_site_id();
        let _ = state.update_clock(None);
        let clock = state.get_clock();
        (clock, local_addr, node)
    };

    let msg;

    match cmd {
        CriticalCommands::CreateUser { name } => {
            use crate::message::CreateUser;
            if name.is_empty() {
                tracing::warn!("Skipping CreateUser command with empty username");
                return Ok(());
            }
            super::db::create_user(&name)?;
            msg = Message {
                command: Some(Command::CreateUser),
                info: MessageInfo::CreateUser(CreateUser::new(name)),
                code: NetworkMessageCode::Transaction,
                clock: clock,
                sender_addr: site_addr,
                sender_id: site_id.to_string(),
                message_initiator_id: site_id.to_string(),
                message_initiator_addr: site_addr,
            };
        }
        CriticalCommands::Deposit { name, amount } => {
            use crate::message::Deposit;

            super::db::deposit(
                &name,
                amount,
                clock.get_lamport(),
                site_id.as_str(),
                clock.get_vector_clock_map(),
            )?;

            msg = Message {
                command: Some(Command::Deposit),
                info: MessageInfo::Deposit(Deposit::new(name, amount)),
                code: NetworkMessageCode::Transaction,
                clock: clock,
                sender_addr: site_addr,
                sender_id: site_id.to_string(),
                message_initiator_id: site_id.to_string(),
                message_initiator_addr: site_addr,
            };
        }
        CriticalCommands::Withdraw { name, amount } => {
            use crate::message::Withdraw;
            super::db::withdraw(
                &name,
                amount,
                clock.get_lamport(),
                site_id.as_str(),
                clock.get_vector_clock_map(),
            )?;

            msg = Message {
                command: Some(Command::Withdraw),
                info: MessageInfo::Withdraw(Withdraw::new(name, amount)),
                code: NetworkMessageCode::Transaction,
                clock: clock,
                sender_addr: site_addr,
                sender_id: site_id.to_string(),
                message_initiator_id: site_id.to_string(),
                message_initiator_addr: site_addr,
            };
        }
        CriticalCommands::Transfer { from, to, amount } => {
            use crate::message::Transfer;
            super::db::create_transaction(
                &from,
                &to,
                amount,
                clock.get_lamport(),
                site_id.as_str(),
                "",
                clock.get_vector_clock_map(),
            )?;
            msg = Message {
                command: Some(Command::Transfer),
                info: MessageInfo::Transfer(Transfer::new(from.clone(), to.clone(), amount)),
                code: NetworkMessageCode::Transaction,
                clock: clock,
                sender_addr: site_addr,
                sender_id: site_id.to_string(),
                message_initiator_id: site_id.to_string(),
                message_initiator_addr: site_addr,
            };
        }
        CriticalCommands::Pay { name, amount } => {
            use crate::message::Pay;
            super::db::create_transaction(
                &name,
                "NULL",
                amount,
                clock.get_lamport(),
                site_id.as_str(),
                "",
                clock.get_vector_clock_map(),
            )?;
            msg = Message {
                command: Some(Command::Pay),
                info: MessageInfo::Pay(Pay::new(name, amount)),
                code: NetworkMessageCode::Transaction,
                clock: clock,
                sender_addr: site_addr,
                sender_id: site_id.to_string(),
                message_initiator_id: site_id.to_string(),
                message_initiator_addr: site_addr,
            };
        }
        CriticalCommands::Refund {
            name,
            lamport,
            node,
        } => {
            use crate::message::Refund;
            super::db::refund_transaction(
                lamport,
                &node.as_str(),
                clock.get_lamport(),
                site_id.as_str(),
                clock.get_vector_clock_map(),
            )?;
            msg = Message {
                command: Some(Command::Refund),
                info: MessageInfo::Refund(Refund::new(name, lamport, node)),
                code: NetworkMessageCode::Transaction,
                clock: clock,
                sender_addr: site_addr,
                sender_id: site_id.to_string(),
                message_initiator_id: site_id.to_string(),
                message_initiator_addr: site_addr,
            };
        }
        CriticalCommands::FileSnapshot => {
            use crate::snapshot;
            snapshot::start_snapshot(snapshot::SnapshotMode::FileMode).await?;

            msg = Message {
                command: None,
                code: NetworkMessageCode::SnapshotRequest,
                info: MessageInfo::None,
                sender_addr: site_addr,
                sender_id: site_id.to_string(),
                message_initiator_id: site_id.to_string(),
                message_initiator_addr: site_addr,
                clock: clock.clone(),
            };
        }
        CriticalCommands::SyncSnapshot => {
            use crate::snapshot;
            snapshot::start_snapshot(snapshot::SnapshotMode::SyncMode).await?;

            msg = Message {
                command: None,
                code: NetworkMessageCode::SnapshotRequest,
                info: MessageInfo::None,
                sender_addr: site_addr,
                sender_id: site_id.to_string(),
                message_initiator_id: site_id.to_string(),
                message_initiator_addr: site_addr,
                clock: clock.clone(),
            };
        }
    }

    let should_diffuse = {
        // initialisation des paramètres avant la diffusion d'un message
        let mut state = LOCAL_APP_STATE.lock().await;
        let nb_neigh = state.get_nb_connected_neighbours();
        state.set_parent_addr(site_id.to_string(), site_addr);
        state.set_nb_nei_for_wave(site_id.to_string(), nb_neigh);
        nb_neigh > 0
    };

    if should_diffuse {
        diffuse_message(&msg).await?;
    } else {
        // pas release depuis le réseau si on est tout seul
        // on doit relacher le mutex directement
        let mut state = LOCAL_APP_STATE.lock().await;
        let _ = state.release_mutex().await;
    };
    Ok(())
}

#[cfg(feature = "server")]
/// Execute a command from the CLI
/// Update the clock of the site
/// Interact with the database
/// Implement our wave diffusion protocol
pub async fn process_cli_command(cmd: Command) -> Result<(), Box<dyn std::error::Error>> {
    use crate::state::LOCAL_APP_STATE;

    match cmd {
        Command::CreateUser => {
            let name = prompt("Username");
            if name.is_empty() {
                println!("❌ Username cannot be empty");
                return Ok(());
            }
            enqueue_critical(CriticalCommands::CreateUser { name }).await?;
        }

        Command::UserAccounts => {
            super::db::print_users()?;
        }

        Command::PrintUserTransactions => {
            let name = prompt("Username");
            super::db::print_transaction_for_user(&name)?;
        }

        Command::PrintTransactions => {
            super::db::print_transactions()?;
        }

        Command::Deposit => {
            let name = prompt("Username");
            let amount = prompt_parse::<f64>("Deposit amount");
            enqueue_critical(CriticalCommands::Deposit {
                name: name,
                amount: amount,
            })
            .await?;
        }

        Command::Withdraw => {
            let name = prompt("Username");
            let amount = prompt_parse::<f64>("Withdraw amount");
            if amount < 0.0 {}
            enqueue_critical(CriticalCommands::Withdraw {
                name: name,
                amount: amount,
            })
            .await?;
        }

        Command::Transfer => {
            let name = prompt("Username");

            let amount = prompt_parse::<f64>("Transfer amount");
            let _ = super::db::print_users();
            let beneficiary = prompt("Beneficiary");

            enqueue_critical(CriticalCommands::Transfer {
                from: name.clone(),
                to: beneficiary.clone(),
                amount,
            })
            .await?;
        }

        Command::Pay => {
            let name = prompt("Username");
            let amount = prompt_parse::<f64>("Payment amount");

            if amount <= 0.0 {
                println!("❌ Amount must be positive");
                return Ok(());
            }
            enqueue_critical(CriticalCommands::Pay {
                name: name.clone(),
                amount,
            })
            .await?;
        }

        Command::Refund => {
            let name = prompt("Username");
            super::db::print_transaction_for_user(&name).unwrap();

            let transac_time = prompt_parse::<i64>("Lamport time");
            let transac_node = prompt("Node");

            enqueue_critical(CriticalCommands::Refund {
                name: name.clone(),
                lamport: transac_time,
                node: transac_node.clone(),
            })
            .await?;
        }

        Command::Help => {
            println!("📜 Command list:");
            println!("----------------------------------------");
            println!("/create_user      - Create a personal account");
            println!("/user_accounts    - List all users");
            println!("/print_user_tsx   - Show a user's transactions");
            println!("/print_tsx        - Show all system transactions");
            println!("/deposit          - Deposit money to an account");
            println!("/withdraw         - Withdraw money from an account");
            println!("/transfer         - Transfer money to another user");
            println!("/pay              - Make a payment (to NULL)");
            println!("/refund           - Refund a transaction");
            println!("/info             - Show system information");
            println!("/start_snapshot   - Start a snapshot");
            println!("/help             - Show this help message");
            println!("----------------------------------------");
        }

        Command::Snapshot => {
            println!("📸 Starting snapshot...");
            enqueue_critical(CriticalCommands::FileSnapshot).await?;
        }

        Command::Info => {
            let (
                site_addr,
                site_id,
                peer_addrs,
                clock,
                nb_connected_neighbours,
                connected_neighbours_addrs,
                parent_addr_for_transaction_wave,
                attended_neighbours_nb_for_transaction_wave,
            ) = {
                let state = LOCAL_APP_STATE.lock().await;
                (
                    state.get_site_addr(),
                    state.get_site_id().to_string(),
                    state.get_cli_peers_addrs(),
                    state.get_clock(),
                    state.get_nb_connected_neighbours(),
                    state.get_connected_nei_addr(),
                    state.get_parent_for_wave_map(),
                    state.get_nb_nei_for_wave(),
                )
            };

            let db_path = {
                let conn = crate::db::DB_CONN.lock().unwrap();
                let path = conn.path().unwrap();
                // keep only the name of the file (after the last "/")
                path.to_string().split("/").last().unwrap().to_string()
            };

            println!("📊 System Information:");
            println!("----------------------------------------");
            println!("Database : {}", db_path);
            println!("Local Address: {}", site_addr);
            println!("Site ID: {}", site_id);
            println!("Number of CLI peers: {}", peer_addrs.len());
            println!("CLI peers: {:?}", peer_addrs);
            println!("Number of connected neighbors: {}", nb_connected_neighbours);
            println!(
                "Number of connected neighbors: {:?}",
                connected_neighbours_addrs
            );
            println!("Vector Clock: {:?}", clock.get_vector_clock_map());
            println!("Lamport Clock: {}", clock.get_lamport());
            println!("--------- Wave diffusion info ------------");
            println!(
                "Parent addresses for wave (if any): {:?}",
                parent_addr_for_transaction_wave
            );
            println!(
                "Attended neighbours for wave (if any): {:?}",
                attended_neighbours_nb_for_transaction_wave
            );
            println!("----------------------------------------");
        }

        Command::Unknown(msg) => {
            println!("❌ Unknown command: {}", msg);
        }

        Command::Error(msg) => {
            println!("❌ Error: {}", msg);
        }
    }

    Ok(())
}

#[cfg(feature = "server")]
/// Process commands received from the network
/// Update the clock of the site
/// Interact with the database
pub async fn process_network_command(
    msg: crate::message::MessageInfo,
    received_clock: crate::clock::Clock,
    sender_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::message::MessageInfo;


    let message_lamport_time = received_clock.get_lamport();
    let message_vc_clock = received_clock.get_vector_clock_map();

    if crate::db::transaction_exists(*message_lamport_time, sender_id)? {
        tracing::info!("Transaction allready exists, skipping");
        return Ok(());
    }

    match msg {
        crate::message::MessageInfo::CreateUser(create_user) => {
            if create_user.name.is_empty() {
                tracing::warn!("Received CreateUser message with empty username, skipping");
                return Ok(());
            }
            if crate::db::user_exists(&create_user.name)? {
                tracing::info!("User already exists, skipping");
                return Ok(());
            }
            super::db::create_user(&create_user.name)?;
        }
        crate::message::MessageInfo::Deposit(deposit) => {
            super::db::deposit(
                &deposit.name,
                deposit.amount,
                &message_lamport_time,
                sender_id,
                &message_vc_clock,
            )?;
        }

        MessageInfo::Withdraw(withdraw) => {
            super::db::withdraw(
                &withdraw.name,
                withdraw.amount,
                &message_lamport_time,
                sender_id,
                &message_vc_clock,
            )?;
        }

        MessageInfo::Transfer(transfer) => {
            super::db::create_transaction(
                &transfer.name,
                &transfer.beneficiary,
                transfer.amount,
                &message_lamport_time,
                sender_id,
                "",
                &message_vc_clock,
            )?;
        }

        MessageInfo::Pay(pay) => {
            super::db::create_transaction(
                &pay.name,
                "NULL",
                pay.amount,
                &message_lamport_time,
                sender_id,
                "",
                &message_vc_clock,
            )?;
        }

        MessageInfo::Refund(refund) => {
            super::db::refund_transaction(
                refund.transac_time,
                &refund.transac_node,
                &message_lamport_time,
                sender_id,
                &message_vc_clock,
            )?;
        }
        crate::message::MessageInfo::SnapshotResponse(_) => {
            tracing::error!("Should not process snapshot response");
        }
        crate::message::MessageInfo::AckMutex(_) => {
            // Handle mutex acknowledgment
        }
        crate::message::MessageInfo::AcquireMutex(_) => {
            // Handle mutex request
        }
        crate::message::MessageInfo::ReleaseMutex(_) => {
            // Handle mutex release
        }
        crate::message::MessageInfo::None => {
            tracing::error!("Should not process None message");
        }
        crate::message::MessageInfo::Acknowledge(_) => {
            tracing::error!("Should not process Acknowledge message");
        }
    }

    Ok(())
}

#[cfg(feature = "server")]
/// Prompts the user for input with a label
fn prompt(label: &str) -> String {
    use std::io::{self, Write};
    print!("{}: ", label);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

#[cfg(feature = "server")]
/// Prompts the user for input and parses it to a specific type
fn prompt_parse<T: std::str::FromStr>(label: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    use std::io::{self, Write};
    loop {
        print!("{}: ", label);
        io::stdout().flush().unwrap();
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        match input.trim().parse() {
            Ok(value) => return value,
            Err(e) => println!("Invalid input: {:?}", e),
        }
    }
}

#[cfg(feature = "server")]
#[tokio::test]
async fn test_mutex_critical_section_high_load() {
    use crate::state::{AppState, MutexStamp, MutexTag};
    use std::net::SocketAddr;

    let local_addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
    let mut state = AppState::new(
        "A".to_string(),
        vec![
            "127.0.0.1:9001".parse().unwrap(),
            "127.0.0.1:9002".parse().unwrap(),
        ],
        local_addr,
    );

    // Set manually the number of connected neighbours
    state.set_nb_connected_neighbours(2);

    // Simulate remote requests in FIFO before our own
    state.global_mutex_fifo.insert(
        "B".to_string(),
        MutexStamp {
            tag: MutexTag::Request,
            date: 1,
        },
    );

    state.global_mutex_fifo.insert(
        "C".to_string(),
        MutexStamp {
            tag: MutexTag::Request,
            date: 2,
        },
    );

    // Now request our own access with a higher Lamport (should wait)
    for _ in 0..3 {
        state.update_clock(None).await;
    }
    let _ = state.acquire_mutex().await;

    // Our site should not be in SC yet
    assert_eq!(state.in_sc, false);

    // Insert ACKs from all peers with lower Lamport (simulate reception)
    state.global_mutex_fifo.insert(
        "B".to_string(),
        MutexStamp {
            tag: MutexTag::Ack,
            date: 1,
        },
    );
    state.global_mutex_fifo.insert(
        "C".to_string(),
        MutexStamp {
            tag: MutexTag::Ack,
            date: 2,
        },
    );

    // Manually call try_enter_sc() to simulate triggering by incoming ack
    state.try_enter_sc();

    // Now we should be in the section critique
    assert_eq!(state.in_sc, true);

    // Simulate some work and then release
    let _ = state.release_mutex().await;

    // After release, should no longer be in critical section
    assert_eq!(state.in_sc, false);
    assert_eq!(state.waiting_sc, false);

    // All entries should be cleaned up
    assert!(!state.global_mutex_fifo.contains_key("A"));

    // Simulate again to check order with large number of requests
    for i in 0..100 {
        let site = format!("S{}", i);
        state.global_mutex_fifo.insert(
            site.clone(),
            MutexStamp {
                tag: MutexTag::Request,
                date: i,
            },
        );
    }

    // Now site A requests with date = 50 (should wait since lower stamps exist)
    for _ in 0..50 {
        state.update_clock(None).await;
    }
    let _ = state.acquire_mutex().await;
    state.try_enter_sc();
    assert_eq!(state.in_sc, false); // can't enter yet

    // Now convert all others to ACK
    for i in 0..100 {
        let site = format!("S{}", i);
        state.global_mutex_fifo.insert(
            site.clone(),
            MutexStamp {
                tag: MutexTag::Ack,
                date: i,
            },
        );
    }

    // Try entering again
    state.try_enter_sc();
    assert_eq!(state.in_sc, true); // should succeed now
}
