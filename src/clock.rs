//! Logical clock implementation for distributed synchronization
//!
//! This module provides both Lamport and Vector clock implementations for
//! maintaining causal ordering of events in the distributed system.

#[cfg(feature = "server")]
/// Implements logical clocks for distributed synchronization
///
/// The Clock struct maintains both a Lamport clock for total ordering
/// and a vector clock for causal ordering of events across the distributed system.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Clock {
    /// Lamport clock value for total ordering of events
    lamport_clock: i64,
    /// Vector clock mapping site IDs to their clock values
    ///
    /// Site_id -> clock value
    vector_clock: std::collections::HashMap<String, i64>,
}

#[cfg(feature = "server")]
impl Clock {
    /// Creates a new Clock instance with initialized clocks
    pub fn new() -> Self {
        Clock {
            lamport_clock: 0,
            vector_clock: std::collections::HashMap::new(),
        }
    }

    pub fn from_parts(
        lamport_clock: i64,
        vector_clock: std::collections::HashMap<String, i64>,
    ) -> Self {
        Clock {
            lamport_clock,
            vector_clock,
        }
    }

    ///Creates a new Clock instance with initialized clocks, used for testing
    #[cfg(test)]
    pub fn new_with_values(lamport: i64, vector: std::collections::HashMap<String, i64>) -> Self {
        Clock {
            lamport_clock: lamport,
            vector_clock: vector,
        }
    }

    /// Increments the Lamport clock and returns the new value
    fn increment_lamport(&mut self) {
        self.lamport_clock += 1;
    }

    /// Increments the vector clock for a specific site and returns the new value
    fn increment_vector(&mut self, site_id: &str) {
        let clock = self.vector_clock.entry(site_id.to_string()).or_insert(0);
        *clock += 1;
    }

    /// Returns a reference to the Lamport clock value
    pub fn get_lamport(&self) -> &i64 {
        &self.lamport_clock
    }

    /// Returns a reference to the vector clock
    pub fn get_vector_clock_map(&self) -> &std::collections::HashMap<String, i64> {
        &self.vector_clock
    }

    /// Returns the vector clock as a list of values
    pub fn get_vector_clock_values(&self) -> Vec<i64> {
        let mut vc: Vec<i64> = vec![0; self.vector_clock.len()];
        for (i, clock_value) in self.vector_clock.values().enumerate() {
            vc[i] = *clock_value;
        }
        vc
    }

    /// Updates the vector clock with received values, taking the maximum of local and received values
    fn update_vector(&mut self, received_vc: &std::collections::HashMap<String, i64>) {
        for (site_id, clock_value) in received_vc {
            let current_value = self.vector_clock.entry(site_id.clone()).or_insert(0);
            *current_value = (*current_value).max(*clock_value) + 1;
        }
    }

    /// Updates the lamport clock with received value, taking the maximum of local and received values
    fn update_lamport(&mut self, received_lc: &i64) {
        self.lamport_clock = (self.lamport_clock).max(*received_lc) + 1;
    }

    /// Update the current clock value with an optional clock
    ///
    /// Local lamport clock is incremented
    ///
    /// Element of local vector clock is incremented
    ///
    /// Then we call update methods to take the maximum of the received clocks if any
    #[tracing::instrument(name = "clock::update_clock", skip(self, received_clock), fields(clef = %local_site_id, has_remote = received_clock.is_some()))]
    pub fn update_clock(&mut self, local_site_id: &str, received_clock: Option<&Self>) {
        if let Some(rc) = received_clock {
            self.update_vector(rc.get_vector_clock_map());
            self.update_lamport(rc.get_lamport());
        } else {
            // If the received clock is None, we increment the local lamport clock and the local vector clock
            self.increment_lamport();
            self.increment_vector(local_site_id);
        }
    }
}

#[cfg(test)]
#[cfg(feature = "server")]
mod tests {
    use super::*;

    #[test]
    fn test_new_clock_initialization() {
        let clock = Clock::new();
        assert_eq!(*clock.get_lamport(), 0);
        assert!(clock.get_vector_clock_map().is_empty());
    }

    #[test]
    fn test_increment_lamport() {
        let mut clock = Clock::new();
        clock.increment_lamport();
        assert_eq!(*clock.get_lamport(), 1);
        clock.increment_lamport();
        assert_eq!(*clock.get_lamport(), 2);
    }

    #[test]
    fn test_increment_vector() {
        let mut clock = Clock::new();
        clock.increment_vector("A");
        clock.increment_vector("A");
        clock.increment_vector("B");

        let vc = clock.get_vector_clock_map();
        assert_eq!(vc.get("A"), Some(&2));
        assert_eq!(vc.get("B"), Some(&1));
    }

    #[test]
    fn test_get_vector_clock_values() {
        let mut clock = Clock::new();
        clock.increment_vector("A");
        clock.increment_vector("B");
        clock.increment_vector("B");

        let mut values = clock.get_vector_clock_values();
        values.sort(); // Order not guaranteed by HashMap
        assert_eq!(values, vec![1, 2]);
    }

    #[test]
    fn test_update_vector_clock() {
        let mut local = Clock::new();
        local.increment_vector("A"); // A:1

        let mut incoming = Clock::new();
        incoming.increment_vector("A"); // A:1
        incoming.increment_vector("A"); // A:2
        incoming.increment_vector("B"); // B:1

        local.update_vector(&incoming.get_vector_clock_map());

        let local_vc = local.get_vector_clock_map();
        assert_eq!(local_vc.get("A"), Some(&3));
        assert_eq!(local_vc.get("B"), Some(&2));
    }

    #[test]
    fn test_update_clock_with_none() {
        let mut clock = Clock::new();
        clock.update_clock("A", None);

        assert_eq!(*clock.get_lamport(), 1);
        assert_eq!(clock.get_vector_clock_map().get("A"), Some(&1));
    }

    #[test]
    fn test_update_clock_with_received_clock() {
        let mut local = Clock::new();
        local.increment_vector("A"); // A:1
        local.increment_vector("A"); // A:2
        local.increment_lamport(); // lamport: 1

        let mut received = Clock::new();
        received.increment_vector("A"); // A:1
        received.increment_vector("B"); // B:1
        received.increment_lamport(); // lamport: 1
        received.increment_lamport(); // lamport: 2

        // Here local is A:2 before
        // Incrementing local to A:3
        // Then updated with received (A:2)
        // So should be A:3 after
        // Lamport was 1 before, so should be 2 after
        local.update_clock("A", Some(&received));

        // Lamport clock should be max(received, local) + 1
        assert_eq!(*local.get_lamport(), 3);
        let vc = local.get_vector_clock_map();
        assert_eq!(vc.get("A"), Some(&3)); // Incremented locally + merged max
        assert_eq!(vc.get("B"), Some(&2));
    }
}
