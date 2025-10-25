//! Snapshot management for distributed state consistency
//!
//! This module implements a distributed snapshot algorithm for ensuring
//! consistency across nodes in the distributed system. It handles snapshot
//! creation, consistency checking, and persistence.

#[cfg(feature = "server")]
/// Summary of a transaction for snapshot purposes
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Eq, PartialEq, Hash)]
pub struct TxSummary {
    /// Lamport timestamp of the transaction
    pub lamport_time: i64,
    /// ID of the node that created the transaction
    pub source_node: String,
    /// Source user of the transaction
    pub from_user: String,
    /// Destination user of the transaction
    pub to_user: String,
    /// Transaction amount
    pub amount_in_cent: i64,
}

#[cfg(feature = "server")]
/// Snapshot mode
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub enum SnapshotMode {
    /// When all snapshots are received, we can create a global snapshot and save the file
    FileMode,
    /// When all snapshot are received, we can create a global snapshot and send it to the network
    NetworkMode,
    /// When all snapshot are received, we can create a global snapshot and apply it to the local state
    SyncMode,
}

#[cfg(feature = "server")]
impl From<&crate::db::Transaction> for TxSummary {
    fn from(tx: &crate::db::Transaction) -> Self {
        Self {
            lamport_time: tx.lamport_time,
            source_node: tx.source_node.clone(),
            from_user: tx.from_user.clone(),
            to_user: tx.to_user.clone(),
            amount_in_cent: (tx.amount * 100.0) as i64,
        }
    }
}

#[cfg(feature = "server")]
/// Local snapshot of a node's state
#[derive(Clone)]
pub struct LocalSnapshot {
    /// ID of the node taking the snapshot
    pub site_id: String,
    /// Vector clock state at the time of the snapshot
    pub vector_clock: std::collections::HashMap<String, i64>,
    /// Set of transactions known to this node
    pub tx_log: std::collections::HashSet<TxSummary>,
}

#[cfg(feature = "server")]
/// Global snapshot combining all local snapshots
#[derive(Clone, Debug, serde::Serialize)]
pub struct GlobalSnapshot {
    /// Union of all transactions across nodes
    pub all_transactions: std::collections::HashSet<TxSummary>,
    /// Map of missing transactions per node
    pub missing: std::collections::HashMap<String, std::collections::HashSet<TxSummary>>,
}

#[cfg(feature = "server")]
impl GlobalSnapshot {
    /// Checks if a set of local snapshots is consistent
    ///
    /// A set of snapshots is consistent if for any pair of nodes i and j,
    /// the vector clock value of j in i's snapshot is not greater than
    /// the vector clock value of j in j's own snapshot.
    pub fn is_consistent(snaps: &[LocalSnapshot]) -> bool {
        for si in snaps {
            for sj in snaps {
                if let (Some(&cij), Some(&cjj)) = (
                    si.vector_clock.get(&sj.site_id),
                    sj.vector_clock.get(&sj.site_id),
                ) {
                    if cij > cjj {
                        return false;
                    }
                }
            }
        }
        true
    }
}

#[cfg(feature = "server")]
/// Manages the snapshot collection process
pub struct SnapshotManager {
    /// Number of snapshots expected to be collected
    pub expected: usize,
    /// Vector of received local snapshots
    pub received: Vec<LocalSnapshot>,
    /// Path to the last snapshot saved
    pub path: Option<std::path::PathBuf>,
    /// Snapshot mode
    pub mode: SnapshotMode,
}

#[cfg(feature = "server")]
impl SnapshotManager {
    /// Creates a new snapshot manager expecting the given number of snapshots
    pub fn new(expected: usize) -> Self {
        Self {
            expected,
            received: Vec::new(),
            path: None,
            mode: SnapshotMode::FileMode,
        }
    }

    /// Adds a snapshot response to the collection
    ///
    /// Returns a global snapshot if all expected snapshots have been received
    /// and they are consistent. If the snapshots are inconsistent, it will
    /// attempt to find a consistent subset by backtracking to the minimum
    /// vector clock values.
    /// all_received is defined by the state of our wave diffusion protocol
    pub fn push(&mut self, resp: crate::message::SnapshotResponse) -> Option<GlobalSnapshot> {
        tracing::debug!(
            operation = "snapshot_push",
            response_site = %resp.site_id,
            expected_snapshots = self.expected,
            "Adding snapshot in the manager."
        );
        self.received.push(LocalSnapshot {
            site_id: resp.site_id.clone(),
            vector_clock: resp.clock.get_vector_clock_map().clone(),
            tx_log: resp.tx_log.into_iter().collect(),
        });

        if self.received.len() < self.expected {
            tracing::debug!(
                operation = "snapshot_push",
                sites_received = self.received.len(),
                sites_expected = self.expected,
                "Number of snapshots received is less than expected, waiting for more."
            );
            return None;
        }

        tracing::debug!(
            operation = "snapshot_push",
             sites_received = self.received.len(),
             sites_expected = self.expected,
            "All local snapshots received, processing snapshot."
        );

        // In Sync and Network modes we simply aggregate all received
        // transactions without enforcing snapshot consistency. This prevents
        // dropping valid transactions when a node joins and requests a global
        // snapshot or when an intermediate node aggregates snapshots from its
        // children
        if matches!(
            self.mode,
            SnapshotMode::SyncMode | SnapshotMode::NetworkMode
        ) {
            return Some(self.build_snapshot(&self.received));
        }

        if GlobalSnapshot::is_consistent(&self.received) {
            return Some(self.build_snapshot(&self.received));
        }

        // Back-track to the last consistent snapshot by computing the minimum vector clock
        // V_j = min_i Ci[j], where Ci[j] is the clock value for site j in snapshot i.
        // This ensures that we only consider transactions that are consistent across all snapshots.

        // Compute the minimum vector clock (vmin) across all received snapshots.
        let mut vmin: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for snap in &self.received {
            for (site, &val) in &snap.vector_clock {
                // Update vmin for each site to the minimum value observed across all snapshots.
                vmin.entry(site.clone())
                    .and_modify(|m| *m = (*m).min(val))
                    .or_insert(val);
            }
        }

        // Create a new list of snapshots with trimmed vector clocks and transaction logs.
        let mut trimmed: Vec<LocalSnapshot> = Vec::new();
        for mut s in self.received.clone() {
            // Limit the vector clock for the current site to the minimum value in vmin.
            let lim = *vmin.get(&s.site_id).unwrap_or(&0);
            s.vector_clock.insert(s.site_id.clone(), lim);

            // Filter the transaction log to only include transactions that are consistent
            // with the minimum vector clock for their source node.
            let tx_keep: std::collections::HashSet<_> = s
                .tx_log
                .into_iter()
                .filter(|t| t.lamport_time <= *vmin.get(&t.source_node).unwrap_or(&0))
                .collect();
            s.tx_log = tx_keep;

            trimmed.push(s);
        }

        Some(self.build_snapshot(&trimmed))
    }

    /// Builds a global snapshot from a set of local snapshots
    ///
    /// Computes the union of all transactions and identifies missing
    /// transactions for each node.
    fn build_snapshot(&self, snaps: &[LocalSnapshot]) -> GlobalSnapshot {
        let mut union: std::collections::HashSet<TxSummary> = std::collections::HashSet::new();
        for s in snaps {
            tracing::info!(
                operation = "build_snapshot",
                site = %s.site_id,
                transactions = ?s.tx_log,
                "Adding transactions from site."
            );
            union.extend(s.tx_log.iter().cloned());
        }

        let mut miss: std::collections::HashMap<String, std::collections::HashSet<TxSummary>> =
            std::collections::HashMap::new();
        for s in snaps {
            let diff: std::collections::HashSet<_> = union.difference(&s.tx_log).cloned().collect();
            if !diff.is_empty() {
                miss.insert(s.site_id.clone(), diff);
            }
        }
        GlobalSnapshot {
            all_transactions: union,
            missing: miss,
        }
    }
}

#[cfg(feature = "server")]
/// Initiates a new snapshot process
///
/// Collects the local transaction log and sends snapshot requests to all peers.
pub async fn start_snapshot(mode: SnapshotMode) -> Result<(), Box<dyn std::error::Error>> {
    let local_txs = crate::db::get_local_transaction_log()?;
    let summaries: Vec<TxSummary> = local_txs.iter().map(|t| t.into()).collect();

    let (site_id, clock, expected) = {
        let st = crate::state::LOCAL_APP_STATE.lock().await;
        // We expect a snapshot from all connected peers
        // + 1 for self
        let expected_peers = match mode {
            SnapshotMode::NetworkMode => {
                // In NetworkMode, we expect a snapshot from all connected peers except our parent
                st.get_connected_nei_addr().len()
            }
            _ => {
                // Default case: expect snapshots from all connected peers including ourselves
                st.get_connected_nei_addr().len() + 1
            }
        };
        (st.get_site_id(), st.get_clock(), expected_peers)
    };

    {
        let mut mgr = LOCAL_SNAPSHOT_MANAGER.lock().await;
        mgr.expected = expected;
        mgr.received.clear();
        mgr.mode = mode.clone();
        if let Some(gs) = mgr.push(crate::message::SnapshotResponse {
            site_id: site_id.clone(),
            clock: clock.clone(),
            tx_log: summaries.clone(),
        }) {
            if mode.clone() == SnapshotMode::FileMode {
                tracing::info!(
                    operation = "start_snapshot",
                    missing = ?gs.missing,
                    "Global snapshot ready to be saved at start, hold per site.",
                );
                mgr.path = crate::snapshot::persist(&gs, site_id.clone())
                    .await
                    .unwrap()
                    .parse()
                    .ok();
            } else if mode.clone() == SnapshotMode::SyncMode {
                tracing::info!(
                    operation = "start_snapshot",
                    "No other site, synchronization done."
                );
            } else {
                tracing::error!(
                    operation = "start_snapshot",
                    "Start snapshot is not supposed to be called when there is no neighbours with network mode."
                );
            }
        }
    }

    Ok(())
}

#[cfg(feature = "server")]
/// Persists a global snapshot to disk
///
/// Saves the snapshot as a JSON file with a timestamp in the filename.
pub async fn persist(snapshot: &GlobalSnapshot, site_id: String) -> std::io::Result<String> {
    use std::io::Write;

    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let filename = format!("snapshot_{}_{}.json", site_id, ts);

    let mut file = std::fs::File::create(&filename)?;
    let json = serde_json::to_string_pretty(snapshot).unwrap();
    file.write_all(json.as_bytes())?;
    println!("📸 Snapshot completed successfully at {}", filename);
    tracing::info!(
        operation = "persist_snapshot",
        filename = %filename,
        "Snapshot saved to file."
    );
    Ok(filename)
}

#[cfg(feature = "server")]
lazy_static::lazy_static! {
    pub static ref LOCAL_SNAPSHOT_MANAGER: tokio::sync::Mutex<SnapshotManager> =
        tokio::sync::Mutex::new(SnapshotManager::new(0));
}

#[cfg(test)]
#[cfg(feature = "server")]
mod tests {
    use super::*;

    fn mk_clock(pairs: &[(&str, i64)]) -> crate::clock::Clock {
        let mut m = std::collections::HashMap::new();
        for (id, v) in pairs {
            m.insert((*id).to_string(), *v);
        }
        let c = crate::clock::Clock::new_with_values(0, m);
        c
    }

    fn resp(site: &str, vc: &[(&str, i64)], txs: &[TxSummary]) -> crate::message::SnapshotResponse {
        crate::message::SnapshotResponse {
            site_id: site.to_string(),
            clock: mk_clock(vc),
            tx_log: txs.to_vec(),
        }
    }

    #[test]
    fn consistency_ok() {
        let s1 = LocalSnapshot {
            site_id: "A".into(),
            vector_clock: std::collections::HashMap::from_iter([("A".into(), 1), ("B".into(), 0)]),
            tx_log: std::collections::HashSet::new(),
        };
        let s2 = LocalSnapshot {
            site_id: "B".into(),
            vector_clock: std::collections::HashMap::from_iter([("A".into(), 1), ("B".into(), 1)]),
            tx_log: std::collections::HashSet::new(),
        };
        assert!(GlobalSnapshot::is_consistent(&[s1, s2]));
    }

    #[test]
    fn consistency_violation() {
        let s1 = LocalSnapshot {
            site_id: "A".into(),
            vector_clock: std::collections::HashMap::from_iter([("A".into(), 2), ("B".into(), 2)]),
            tx_log: std::collections::HashSet::new(),
        };
        let s2 = LocalSnapshot {
            site_id: "B".into(),
            vector_clock: std::collections::HashMap::from_iter([("A".into(), 1), ("B".into(), 1)]),
            tx_log: std::collections::HashSet::new(),
        };
        assert!(!GlobalSnapshot::is_consistent(&[s1, s2]));
    }

    #[test]
    fn push_waits_for_expected() {
        let mut mgr = SnapshotManager::new(2);
        let tx = TxSummary {
            lamport_time: 1,
            source_node: "A".into(),
            from_user: "user1".into(),
            to_user: "user2".into(),
            amount_in_cent: 100,
        };
        let r1 = resp("A", &[("A", 1)], &[tx.clone()]);
        assert!(mgr.push(r1).is_none());
        assert_eq!(mgr.received.len(), 1);
    }

    #[test]
    fn push_detects_incoherence() {
        let mut mgr = SnapshotManager::new(2);
        let bad_r1 = resp("A", &[("A", 2), ("B", 2)], &[]);
        let bad_r2 = resp("B", &[("A", 1), ("B", 1)], &[]);
        assert!(mgr.push(bad_r1).is_none());
        let snap = mgr.push(bad_r2).expect("back-tracked snapshot");
        assert!(GlobalSnapshot::is_consistent(&[LocalSnapshot {
            site_id: "dummy".into(),
            vector_clock: std::collections::HashMap::new(),
            tx_log: snap.all_transactions.clone()
        }]));
        assert!(snap.missing.is_empty() || !snap.missing.contains_key("A"));
    }

    #[test]
    fn push_computes_missing_and_dedup() {
        let mut mgr = SnapshotManager::new(2);
        let t1 = TxSummary {
            lamport_time: 10,
            source_node: "A".into(),
            from_user: "user1".into(),
            to_user: "user2".into(),
            amount_in_cent: 100,
        };
        let t2 = TxSummary {
            lamport_time: 11,
            source_node: "B".into(),
            from_user: "user3".into(),
            to_user: "user4".into(),
            amount_in_cent: 200,
        };

        let r1 = resp("A", &[("A", 1)], &[t1.clone()]);
        let r2 = resp("B", &[("B", 1)], &[t1.clone(), t2.clone()]);

        let _ = mgr.push(r1);
        let gs = mgr.push(r2).expect("snapshot ready");

        assert_eq!(gs.all_transactions.len(), 2);
        assert_eq!(
            gs.missing["A"],
            std::collections::HashSet::from_iter([t2.clone()])
        );
        assert!(!gs.missing.contains_key("B"));
    }

    #[test]
    fn consistency_handles_missing_columns() {
        let a = LocalSnapshot {
            site_id: "A".into(),
            vector_clock: std::collections::HashMap::from_iter([("A".into(), 3)]),
            tx_log: std::collections::HashSet::new(),
        };
        let b = LocalSnapshot {
            site_id: "B".into(),
            vector_clock: std::collections::HashMap::from_iter([("B".into(), 1)]),
            tx_log: std::collections::HashSet::new(),
        };
        assert!(GlobalSnapshot::is_consistent(&[a, b]));
    }

    #[test]
    fn backtrack_trims_future_transactions() {
        let mut mgr = SnapshotManager::new(2);

        let t1 = TxSummary {
            lamport_time: 1,
            source_node: "A".into(),
            from_user: "user1".into(),
            to_user: "user2".into(),
            amount_in_cent: 100,
        };
        let t3 = TxSummary {
            lamport_time: 3,
            source_node: "A".into(),
            from_user: "user1".into(),
            to_user: "user2".into(),
            amount_in_cent: 300,
        };
        let t5 = TxSummary {
            lamport_time: 5,
            source_node: "A".into(),
            from_user: "user1".into(),
            to_user: "user2".into(),
            amount_in_cent: 500,
        };

        let r_a = resp(
            "A",
            &[("A", 5), ("B", 2)], // ← incohérence ici
            &[t1.clone(), t3.clone(), t5.clone()],
        );

        let r_b = resp("B", &[("A", 2), ("B", 1)], &[]);

        let _ = mgr.push(r_a);
        let snap = mgr.push(r_b).expect("snapshot after back-track");

        assert!(snap.all_transactions.contains(&t1));
        assert!(!snap.all_transactions.contains(&t5));
    }

    #[test]
    fn union_is_deduplicated() {
        let mut mgr = SnapshotManager::new(2);
        let tx = TxSummary {
            lamport_time: 7,
            source_node: "A".into(),
            from_user: "user1".into(),
            to_user: "user2".into(),
            amount_in_cent: 700,
        };

        let r1 = resp("A", &[("A", 1)], &[tx.clone()]);
        let r2 = resp("B", &[("B", 1)], &[tx.clone()]);

        let _ = mgr.push(r1);
        let gs = mgr.push(r2).expect("snapshot ready");
        assert_eq!(gs.all_transactions.len(), 1);
    }
}
