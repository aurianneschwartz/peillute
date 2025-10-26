//! Application state management for Peillute
//!
//! This module handles the global application state, including site information,
//! peer management, and logical clock synchronization.

#[cfg(feature = "server")]
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutexTag {
    Request,
    Release,
    #[allow(dead_code)]
    Ack,
}

#[cfg(feature = "server")]
#[derive(serde::Serialize, serde::Deserialize, Copy, Clone, Debug)]
pub struct MutexStamp {
    pub tag: MutexTag,
    pub date: i64,
}

#[cfg(feature = "server")]
/// Represents the global state of a Peillute node
pub struct AppState {
    // --- Site Info ---
    /// Unique identifier for this site
    site_id: String,
    /// Unique address for this site
    site_addr: std::net::SocketAddr,
    /// List of peer addresses given in arguments at the launch of the application
    cli_peer_addrs: Vec<std::net::SocketAddr>,
    /// List of deg(1) neighbours connected addresses
    connected_neighbours_addrs: Vec<std::net::SocketAddr>,
    /// Hashmap of sockets for each deg(1) neighbours
    neighbours_socket: std::collections::HashMap<std::net::SocketAddr, std::net::SocketAddr>,
    /// Synchronization boolean
    sync_needed: bool,
    /// Number of attended neighbours at launch, for the discovery phase
    nb_first_attended_neighbours: i64,

    pub site_ids_to_adr: std::collections::HashMap<std::net::SocketAddr, String>,

    // --- Message Diffusion Info for Transaction ---
    /// Adress of the parent (deg(1) neighbour for this site) for a specific wave from initiator id
    pub parent_addr_for_transaction_wave: std::collections::HashMap<String, std::net::SocketAddr>,
    /// Number of response expected from our direct neighbours (deg(1) neighbours for this site) = nb of connected neighbours - 1 (parent) for a specific wave initiator id
    pub attended_neighbours_nb_for_transaction_wave: std::collections::HashMap<String, i64>,

    // --- Logical Clocks ---
    /// Logical clock implementation for distributed synchronization
    clocks: crate::clock::Clock,

    // GLobal mutex
    pub global_mutex_fifo: std::collections::HashMap<String, MutexStamp>,
    pub waiting_sc: bool,
    pub in_sc: bool,
    pub notify_sc: std::sync::Arc<tokio::sync::Notify>,
    pub pending_commands: std::collections::VecDeque<crate::control::CriticalCommands>,
}

#[cfg(feature = "server")]
impl AppState {
    /// Creates a new AppState instance with the given configuration
    pub fn new(
        site_id: String,
        peer_addrs: Vec<std::net::SocketAddr>,
        local_addr: std::net::SocketAddr,
    ) -> Self {
        let clocks = crate::clock::Clock::new();
        let parent_addr = std::collections::HashMap::new();
        let nb_of_attended_neighbors = std::collections::HashMap::new();
        let in_use_neighbors = Vec::new();
        let sockets_for_connected_peers = std::collections::HashMap::new();
        let gm = std::collections::HashMap::new();
        let waiting_sc = false;
        let in_sc = false;

        Self {
            site_id,
            cli_peer_addrs: peer_addrs,
            neighbours_socket: sockets_for_connected_peers,
            site_addr: local_addr,
            parent_addr_for_transaction_wave: parent_addr,
            attended_neighbours_nb_for_transaction_wave: nb_of_attended_neighbors,
            connected_neighbours_addrs: in_use_neighbors,
            clocks,
            sync_needed: false,
            nb_first_attended_neighbours: 0,
            global_mutex_fifo: gm,
            waiting_sc,
            in_sc,
            notify_sc: std::sync::Arc::new(tokio::sync::Notify::new()),
            pending_commands: std::collections::VecDeque::new(),
            site_ids_to_adr: std::collections::HashMap::new(),
        }
    }

    pub fn get_global_mutex_fifo(&self) -> &std::collections::HashMap<String, MutexStamp> {
        &self.global_mutex_fifo
    }

    pub fn set_global_mutex_fifo(
        &mut self,
        global_mutex_fifo: std::collections::HashMap<String, MutexStamp>,
    ) {
        if self.global_mutex_fifo.len() >= global_mutex_fifo.len() {
            return; // Do not overwrite if the new FIFO is smaller or equal
        }
        self.global_mutex_fifo = global_mutex_fifo;
    }

    pub fn add_site_id(&mut self, site_id: String, addr: std::net::SocketAddr) {
        if !self.site_ids_to_adr.contains_key(&addr) {
            self.site_ids_to_adr.insert(addr, site_id);
        }
    }

    /// Sets the site ID at initialization
    pub fn init_site_id(&mut self, site_id: String) {
        self.site_id = site_id;
    }

    /// Sets the site address at initialization
    pub fn init_site_addr(&mut self, site_addr: std::net::SocketAddr) {
        self.site_addr = site_addr;
    }

    /// Sets the list of CLI peer addresses at initialization
    pub fn init_cli_peer_addrs(&mut self, cli_peer_addrs: Vec<std::net::SocketAddr>) {
        self.cli_peer_addrs = cli_peer_addrs;
    }

    /// Set the clock at initialization
    pub fn init_clock(&mut self, clock: crate::clock::Clock) {
        self.clocks = clock;
    }

    /// Set the sync boolean at initialization
    pub fn init_sync(&mut self, sync_needed: bool) {
        if sync_needed {
            tracing::warn!(
                sync_needed = sync_needed,
                operation = "init_sync",
                "Local site requires synchronization with network"
            );
        }
        self.sync_needed = sync_needed;
    }

    /// Get the sync boolean
    pub fn get_sync(&self) -> bool {
        self.sync_needed
    }

    /// Set the number of attended neighbours at initialization
    pub fn init_nb_first_attended_neighbours(&mut self, nb: i64) {
        tracing::debug!(
            operation = "init_nb_first_attended_neighbours",
            nb_of_attended_neighbors = nb,
            "We will wait for this number of neighbours",
        );
        self.nb_first_attended_neighbours = nb;
    }

    /// Get the number of attended neighbours
    pub fn get_nb_first_attended_neighbours(&self) -> i64 {
        self.nb_first_attended_neighbours
    }

    /// Initialize the parent of the current site as self for the wave protocol
    pub fn init_parent_addr_for_transaction_wave(&mut self) {
        self.parent_addr_for_transaction_wave
            .insert(self.site_id.clone(), self.site_addr.clone());
    }

    /// Adds a new peer to the network and updates the logical clock
    ///
    /// This function should be safe to call multiple times
    ///
    /// If a new site appear on the netword, every peers will launch a wave diffusion to announce the presence of this new site
    pub fn add_incomming_peer(
        &mut self,
        new_addr: std::net::SocketAddr,
        new_socket: std::net::SocketAddr,
        received_clock: crate::clock::Clock,
    ) {
        if !self.connected_neighbours_addrs.contains(&new_addr) {
            self.connected_neighbours_addrs.push(new_addr);
            self.clocks
                .update_clock(self.site_id.clone().as_str(), Some(&received_clock));
            self.neighbours_socket.insert(new_socket, new_addr);
        }
    }

    /// Removes a peer from the network
    ///
    /// This function should be safe to call multiple times
    ///
    /// If a site disappear from the network, every neighbours will detected the closing of the tcp connection and will launch a wave diffusion to announce the disappearance of this site
    ///
    /// If a site is closed properly, it will send a disconnect message to all its neighbours
    pub async fn remove_peer(&mut self, addr_to_remove: std::net::SocketAddr) {
        {
            let mut net_manager = crate::network::NETWORK_MANAGER.lock().await;
            net_manager.remove_connection(&addr_to_remove);
        }

        if let Some(pos) = self
            .connected_neighbours_addrs
            .iter()
            .position(|x| *x == addr_to_remove)
        {
            self.connected_neighbours_addrs.remove(pos);
            let site_id = self.site_ids_to_adr.get(&addr_to_remove);
            if let Some(site_id) = site_id {
                self.global_mutex_fifo.remove(site_id);
                self.attended_neighbours_nb_for_transaction_wave
                    .remove(site_id);
                self.parent_addr_for_transaction_wave.remove(site_id);
                self.site_ids_to_adr.remove(&addr_to_remove);
            }

            // We can keep the clock value for the site we want to remove
            // if the site re-appears, it will be updated with the new clock value
        }
    }

    /// Removes a peer from the network with only an address
    ///
    /// This function should be safe to call multiple times
    ///
    /// If a site disappear from the network, every neighbours will detected the closing of the tcp connection and will launch a wave diffusion to announce the disappearance of this site
    ///
    /// If a site is closed properly, it will send a disconnect message to all its neighbours
    pub async fn remove_peer_from_socket_closed(&mut self, socket_to_remove: std::net::SocketAddr) {
        // Find the site adress based on the socket
        let Some(addr_to_remove) = self.neighbours_socket.get(&socket_to_remove) else {
            tracing::debug!(
                operation = "remove_peer_from_socket_closed",
                "Site not found in the neighbours socket"
            );
            return;
        };

        if let Some(pos) = self
            .connected_neighbours_addrs
            .iter()
            .position(|x| *x == *addr_to_remove)
        {
            self.connected_neighbours_addrs.remove(pos);
            let site_id = self.site_ids_to_adr.get(&addr_to_remove);
            if let Some(site_id) = site_id {
                self.global_mutex_fifo.remove(site_id);
                self.attended_neighbours_nb_for_transaction_wave
                    .remove(site_id);
                self.parent_addr_for_transaction_wave.remove(site_id);
                self.site_ids_to_adr.remove(&addr_to_remove);
            }

            // We can keep the clock value for the site we want to remove
            // if the site re-appears, it will be updated with the new clock value
        }
    }

    /// Returns the local address as a string
    pub fn get_site_addr(&self) -> std::net::SocketAddr {
        self.site_addr.clone()
    }

    pub async fn acquire_mutex(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        use crate::message::{Message, MessageInfo, NetworkMessageCode};
        use crate::network::diffuse_message_without_lock;

        self.update_clock(None).await;

        self.global_mutex_fifo.insert(
            self.site_id.clone(),
            MutexStamp {
                tag: MutexTag::Request,
                date: self.clocks.get_lamport().clone(),
            },
        );

        let msg = Message {
            sender_id: self.site_id.clone(),
            sender_addr: self.site_addr,
            message_initiator_id: self.site_id.clone(),
            message_initiator_addr: self.site_addr,
            clock: self.clocks.clone(),
            command: None,
            info: MessageInfo::AcquireMutex(crate::message::AcquireMutexPayload),
            code: NetworkMessageCode::AcquireMutex,
        };

        let should_diffuse = {
            // initialisation des paramètres avant la diffusion d'un message
            self.set_parent_addr(self.site_id.to_string(), self.site_addr);
            self.set_nb_nei_for_wave(self.site_id.to_string(), self.get_nb_connected_neighbours());
            self.get_nb_connected_neighbours() > 0
        };

        if should_diffuse {
            self.notify_sc.notify_waiters();
            self.in_sc = false;
            self.waiting_sc = true;
            tracing::info!(
                node = self.site_id.as_str(),
                operation = "acquire_mutex",
                "Start of mutex acquisition broadcasting"
            );
            diffuse_message_without_lock(
                &msg,
                self.get_site_addr(),
                self.get_site_id().as_str(),
                self.get_connected_nei_addr(),
                self.get_parent_addr_for_wave(msg.message_initiator_id.clone()),
            )
            .await?;
        } else {
            tracing::info!(
                node = self.site_id.as_str(),
                operation = "acquire_mutex",
                "No neighbors available, entering critical section"
            );
            self.in_sc = true;
            self.waiting_sc = false;
            self.notify_sc.notify_waiters();
        }

        Ok(())
    }

    pub async fn release_mutex(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        use crate::message::{Message, MessageInfo, NetworkMessageCode};
        use crate::network::diffuse_message_without_lock;

        self.update_clock(None).await;

        let msg = Message {
            sender_id: self.site_id.clone(),
            sender_addr: self.site_addr,
            message_initiator_id: self.site_id.clone(),
            message_initiator_addr: self.site_addr,
            clock: self.clocks.clone(),
            command: None,
            info: MessageInfo::ReleaseMutex(crate::message::ReleaseMutexPayload),
            code: NetworkMessageCode::ReleaseGlobalMutex,
        };

        self.global_mutex_fifo.remove(&self.site_id);
        self.in_sc = false;
        self.waiting_sc = false;

        let should_diffuse = {
            self.set_parent_addr(self.site_id.to_string(), self.site_addr);
            self.set_nb_nei_for_wave(self.site_id.to_string(), self.get_nb_connected_neighbours());
            self.get_nb_connected_neighbours() > 0
        };

        if should_diffuse {
            tracing::info!(
                node = self.site_id.as_str(),
                operation = "release_mutex",
                "Start of a mutex release broadcast"
            );
            diffuse_message_without_lock(
                &msg,
                self.get_site_addr(),
                self.get_site_id().as_str(),
                self.get_connected_nei_addr(),
                self.get_parent_addr_for_wave(msg.message_initiator_id.clone()),
            )
            .await?;
        }
        Ok(())
    }

    pub fn try_enter_sc(&mut self) {
        // MUST BE CALLED ONLY AFTER A SUCCESSFUL WAVE AFTER ACQUIRE MUTEX
        // This function checks if the site can enter the critical section
        // It checks if the site is waiting for the critical section and if it can enter
        // based on the FIFO order of requests in the global mutex FIFO.

        // Pour respecter l'algo du poly il faut que la vague soit complete
        // c'est à dire que tout le monde ait répondu ACK pour appeller cette fonction
        // sinon on va entrer en section critique à un moment sans qu'un des peers ait noté notre demande
        let my_stamp = match self.global_mutex_fifo.get(&self.site_id) {
            Some(s) => *s,
            None => return, // No local request found
        };
        let me = (my_stamp.date, self.site_id.clone());

        // ici on compara les stamps des autres demandes, est-ce qu'on est le suivant dans la FIFO ?
        // si oui on peut entrer en section critique
        let ok = self.global_mutex_fifo.iter().all(|(id, stamp)| {
            if id == &self.site_id {
                true
            } else {
                match stamp.tag {
                    MutexTag::Request => me <= (stamp.date, id.clone()),
                    _ => true,
                }
            }
        });

        if ok {
            self.waiting_sc = false;
            self.in_sc = true;
            // All other sites are notified that we are in critical section
            self.notify_sc.notify_waiters(); // notifies worker to execute pending commands
            // We remove obsolete Releases
            self.global_mutex_fifo
                .retain(|_, s| s.tag != MutexTag::Release);
        } else {
            println!("\x1b[1;31mSECTION CRITIQUE REFUSEE !\x1b[0m");
            // print fifo order
            println!("FIFO order:");
            for (id, stamp) in &self.global_mutex_fifo {
                println!("{}: {:?} - {:?}", id, stamp.tag, stamp.date);
            }
            // print my stamp
            println!("{}: {:?} - {:?}", self.site_id, my_stamp.tag, my_stamp.date);
        }
    }

    /// Returns the local address as a string
    pub fn get_site_addr_as_string(&self) -> String {
        self.site_addr.to_string()
    }

    /// Returns a reference to the local address as &str
    pub fn get_site_id(&self) -> String {
        self.site_id.clone()
    }

    /// Returns a list of all peer addresses
    pub fn get_cli_peers_addrs(&self) -> Vec<std::net::SocketAddr> {
        self.cli_peer_addrs.clone()
    }

    /// Returns a list of all peer addresses as strings
    pub fn get_cli_peers_addrs_as_string(&self) -> Vec<String> {
        self.cli_peer_addrs.iter().map(|x| x.to_string()).collect()
    }

    /// Returns a list of conncted neibhours
    pub fn get_connected_nei_addr(&self) -> Vec<std::net::SocketAddr> {
        self.connected_neighbours_addrs.clone()
    }

    /// Returns a list of conncted neibhours as strings
    pub fn get_connected_nei_addr_string(&self) -> Vec<String> {
        self.connected_neighbours_addrs
            .iter()
            .map(|x| x.to_string())
            .collect()
    }

    /// Add a connected neighbour to the list of connected neighbours
    pub fn add_connected_neighbour(&mut self, addr: std::net::SocketAddr) {
        self.connected_neighbours_addrs.push(addr);
    }

    /// Returns the clock of the site
    pub fn get_clock(&self) -> crate::clock::Clock {
        self.clocks.clone()
    }

    /// Set the number of attended neighbors for the wave from initiator_id
    pub fn set_nb_nei_for_wave(&mut self, initiator_id: String, n: i64) {
        self.attended_neighbours_nb_for_transaction_wave
            .insert(initiator_id, n);
    }

    /// Get the list of attended neighbors for the wave from initiator_id
    pub fn get_parent_for_wave_map(
        &self,
    ) -> std::collections::HashMap<String, std::net::SocketAddr> {
        self.parent_addr_for_transaction_wave.clone()
    }

    /// Get the list of attended neighbors for the wave from initiator_id
    pub fn get_nb_nei_for_wave(&self) -> std::collections::HashMap<String, i64> {
        self.attended_neighbours_nb_for_transaction_wave.clone()
    }

    /// Get the parent (neighbour deg(1)) address for the wave from initiator_id
    pub fn get_parent_addr_for_wave(&self, initiator_id: String) -> std::net::SocketAddr {
        self.parent_addr_for_transaction_wave
            .get(&initiator_id)
            .copied()
            .unwrap_or("0.0.0.0:0".parse().unwrap())
    }

    /// Set the parent (neighbour deg(1)) address for the wave from initiator_id
    pub fn set_parent_addr(&mut self, initiator_id: String, peer_adr: std::net::SocketAddr) {
        self.parent_addr_for_transaction_wave
            .insert(initiator_id, peer_adr);
    }

    /// Returns the number of deg(1) neighbors connected
    pub fn get_nb_connected_neighbours(&self) -> i64 {
        self.connected_neighbours_addrs.len() as i64
    }

    /// Update the clock of the site
    pub async fn update_clock(&mut self, received_vc: Option<&crate::clock::Clock>) {
        // this wrapper is needed to ensure that the clock is saved
        // each time it is updated
        // please DO NOT call the `update_clock` method directly from the clock
        self.clocks.update_clock(&self.site_id, received_vc);
        self.save_local_state().await;
    }

    pub async fn save_local_state(&self) {
        // this is likely to be called whenever the clocks are updated
        let _ = crate::db::update_local_state(&self.site_id, self.clocks.clone());
    }

    /// For tokyo test, set manually the number of connected neighbours
    /// DO NOT USE IN PRODUCTION
    #[cfg(test)]
    pub fn set_nb_connected_neighbours(&mut self, nb: i64) {
        self.connected_neighbours_addrs.clear();
        for _ in 0..nb {
            self.connected_neighbours_addrs
                .push("127.0.0.1:8081".parse().unwrap());
        }
    }
}

// Singleton
#[cfg(feature = "server")]
lazy_static::lazy_static! {
    pub static ref LOCAL_APP_STATE: std::sync::Arc<tokio::sync::Mutex<AppState>> =
        std::sync::Arc::new(tokio::sync::Mutex::new(AppState::new(
            "".to_string(), // empty site id at start
            Vec::new(),
            "0.0.0.0:0".parse().unwrap(),
        )));
}

#[cfg(test)]
#[cfg(feature = "server")]
mod tests {
    use super::*;

    #[test]
    fn test_new_state() {
        let cli_site_id = "A".to_string();
        let num_sites = 2;
        let peer_addrs = vec![
            "127.0.0.1:8081".parse().unwrap(),
            "127.0.0.1:8082".parse().unwrap(),
        ];
        let local_addr: std::net::SocketAddr = format!("127.0.0.1:{}", 8080).parse().unwrap();
        let shared_state = AppState::new(cli_site_id.clone(), peer_addrs.clone(), local_addr);

        assert_eq!(shared_state.site_id, cli_site_id);
        assert_eq!(shared_state.cli_peer_addrs.len() as i64, num_sites);
        assert_eq!(shared_state.cli_peer_addrs, peer_addrs);
        assert_eq!(shared_state.clocks.get_vector_clock_map().len(), 0); // Initially empty
    }
}
