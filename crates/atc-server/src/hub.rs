//! Pure-logic frequency hub: rooms per frequency, PTT locking, tuning,
//! unique callsigns. No I/O.

use std::collections::HashMap;

pub type ClientId = u64;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HubError {
    #[error("callsign already in use")]
    DuplicateCallsign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PttOutcome {
    Granted,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubEvent {
    PttReleased { freq_khz: u32, by: ClientId },
}

#[derive(Debug)]
struct Client {
    callsign: String,
    /// Frequencies heard (the transmit radio first; empty until the first `tune`).
    tuned: Vec<u32>,
    /// Frequency on which this client currently holds PTT.
    ptt: Option<u32>,
}

/// Frequency hub. One transmission per frequency at a time, one per client.
#[derive(Debug, Default)]
pub struct Hub {
    clients: HashMap<ClientId, Client>,
    /// freq -> transmitting client.
    locks: HashMap<u32, ClientId>,
}

impl Hub {
    pub fn join(&mut self, id: ClientId, callsign: String) -> Result<(), HubError> {
        if self.clients.values().any(|c| c.callsign == callsign) {
            return Err(HubError::DuplicateCallsign);
        }
        self.clients.insert(
            id,
            Client {
                callsign,
                tuned: Vec::new(),
                ptt: None,
            },
        );
        Ok(())
    }

    /// Removes the client; releases its PTT lock if held. Unknown id: no-op.
    pub fn leave(&mut self, id: ClientId) -> Vec<HubEvent> {
        let Some(c) = self.clients.remove(&id) else {
            return Vec::new();
        };
        match c.ptt {
            Some(freq_khz) => {
                self.locks.remove(&freq_khz);
                vec![HubEvent::PttReleased { freq_khz, by: id }]
            }
            None => Vec::new(),
        }
    }

    /// Retunes the client to the frequencies it hears. A held PTT stays on its original
    /// frequency. Unknown id: no-op.
    pub fn tune(&mut self, id: ClientId, heard_khz: &[u32]) {
        if let Some(c) = self.clients.get_mut(&id) {
            c.tuned = heard_khz.to_vec();
        }
    }

    /// Unknown id returns `Blocked`. Repeating on the held frequency is
    /// idempotent (`Granted`); a different frequency while holding is `Blocked`.
    pub fn ptt_start(&mut self, id: ClientId, freq_khz: u32) -> PttOutcome {
        let Some(c) = self.clients.get_mut(&id) else {
            return PttOutcome::Blocked;
        };
        if let Some(held) = c.ptt {
            return if held == freq_khz {
                PttOutcome::Granted
            } else {
                PttOutcome::Blocked
            };
        }
        if self.locks.contains_key(&freq_khz) {
            return PttOutcome::Blocked;
        }
        self.locks.insert(freq_khz, id);
        c.ptt = Some(freq_khz);
        PttOutcome::Granted
    }

    /// Returns the frequency PTT was held on (even after retune), if any.
    pub fn ptt_end(&mut self, id: ClientId) -> Option<u32> {
        let freq = self.clients.get_mut(&id)?.ptt.take()?;
        self.locks.remove(&freq);
        Some(freq)
    }

    /// Clients hearing `freq_khz`, sorted by id.
    pub fn listeners(&self, freq_khz: u32) -> Vec<ClientId> {
        let mut v: Vec<_> = self
            .clients
            .iter()
            .filter(|(_, c)| c.tuned.contains(&freq_khz))
            .map(|(&id, _)| id)
            .collect();
        v.sort_unstable();
        v
    }

    pub fn callsign(&self, id: ClientId) -> Option<&str> {
        self.clients.get(&id).map(|c| c.callsign.as_str())
    }

    /// The client flying `callsign` (as joined).
    pub fn client(&self, callsign: &str) -> Option<ClientId> {
        self.clients
            .iter()
            .find(|(_, c)| c.callsign == callsign)
            .map(|(&id, _)| id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hub() -> Hub {
        let mut h = Hub::default();
        h.join(1, "N1".into()).unwrap();
        h.join(2, "N2".into()).unwrap();
        h.tune(1, &[121_500]);
        h.tune(2, &[121_500]);
        h
    }

    #[test]
    fn second_ptt_on_same_freq_is_blocked() {
        let mut h = hub();
        assert_eq!(h.ptt_start(1, 121_500), PttOutcome::Granted);
        assert_eq!(h.ptt_start(2, 121_500), PttOutcome::Blocked);
        assert_eq!(h.ptt_end(1), Some(121_500));
        assert_eq!(h.ptt_start(2, 121_500), PttOutcome::Granted);
    }

    #[test]
    fn ptt_on_other_freq_is_granted() {
        let mut h = hub();
        assert_eq!(h.ptt_start(1, 121_500), PttOutcome::Granted);
        assert_eq!(h.ptt_start(2, 118_000), PttOutcome::Granted);
    }

    #[test]
    fn ptt_keeps_original_freq_after_retune() {
        let mut h = hub();
        assert_eq!(h.ptt_start(1, 121_500), PttOutcome::Granted);
        h.tune(1, &[118_000]);
        assert_eq!(h.ptt_start(2, 121_500), PttOutcome::Blocked);
        assert_eq!(h.ptt_end(1), Some(121_500));
    }

    #[test]
    fn disconnect_mid_ptt_releases_lock() {
        let mut h = hub();
        h.ptt_start(1, 121_500);
        assert_eq!(
            h.leave(1),
            vec![HubEvent::PttReleased {
                freq_khz: 121_500,
                by: 1
            }]
        );
        assert_eq!(h.ptt_start(2, 121_500), PttOutcome::Granted);
        assert!(h.leave(2).len() == 1);
        assert!(h.leave(2).is_empty());
    }

    #[test]
    fn listeners_follow_tuning() {
        let mut h = hub();
        assert_eq!(h.listeners(121_500), vec![1, 2]);
        h.tune(1, &[118_000]);
        assert_eq!(h.listeners(121_500), vec![2]);
        assert_eq!(h.listeners(118_000), vec![1]);
        h.leave(2);
        assert!(h.listeners(121_500).is_empty());
        // Two radios heard: both frequencies reach the client.
        h.tune(1, &[118_000, 126_130]);
        assert_eq!(h.listeners(126_130), vec![1]);
        assert_eq!(h.listeners(118_000), vec![1]);
    }

    #[test]
    fn duplicate_callsign_rejected() {
        let mut h = hub();
        assert_eq!(h.join(3, "N1".into()), Err(HubError::DuplicateCallsign));
        assert_eq!(h.callsign(3), None);
        assert_eq!(h.callsign(1), Some("N1"));
        h.leave(1);
        assert!(h.join(3, "N1".into()).is_ok());
    }

    #[test]
    fn unknown_and_repeat_ptt() {
        let mut h = hub();
        assert_eq!(h.ptt_start(99, 121_500), PttOutcome::Blocked);
        h.tune(99, &[1]);
        assert_eq!(h.ptt_end(99), None);
        assert!(h.leave(99).is_empty());
        assert_eq!(h.ptt_start(1, 121_500), PttOutcome::Granted);
        assert_eq!(h.ptt_start(1, 121_500), PttOutcome::Granted);
        assert_eq!(h.ptt_start(1, 118_000), PttOutcome::Blocked);
    }
}
