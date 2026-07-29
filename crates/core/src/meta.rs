//! Meta-progression state (SPEC §18 — Soul Remembrance).
//!
//! v0.5 minimal slice:
//! - Champion class+job pair (5 jobs x 5 races = 25 slots, but we keep the
//!   struct open so future work can add race/starting bonuses)
//! - Wisp (강령 파편) count
//! - Mark of the Departed (순교자 표시) count
//! - Last wager gold carried
//!
//! This struct serialises to RON and is what the save file stores.

#![deny(rust_2018_idioms)]

use serde::{Deserialize, Serialize};

/// Permanent bonuses carried between runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SoulRemembrance {
    /// Total champion souls unlocked (at most 30 — see SPEC §18).
    pub champion_count: u32,
    /// Equipment slots added (0..=8). Each wisp permanently adds +1 slot.
    pub wisps: u8,
    /// Bosses defeated across all runs. Each mark → next run's boss HP -1%.
    pub marks_of_departed: u32,
    /// Gold carried over from the last run, capped at 200 (SPEC §18 cap).
    pub last_wager_gold: u32,
    /// Total clears of the 8-floor run.
    pub clears: u32,
}

impl SoulRemembrance {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply end-of-run rewards to this remembrance.
    /// `final_floor` is the deepest floor reached (1..=8).
    /// `gold_remaining` is the gold left in inventory when the run ended.
    pub fn on_run_end(&mut self, final_floor: u32, gold_remaining: u32, bosses_killed_this_run: u32) {
        if final_floor >= 8 {
            self.clears = self.clears.saturating_add(1);
            // Clearing grants a champion soul slot (cap 30).
            self.champion_count = (self.champion_count + 1).min(30);
        }
        // Last-wager deposit: 10% of remaining gold, capped at 200.
        let deposit = ((gold_remaining as u32) / 10).min(200);
        self.last_wager_gold = self.last_wager_gold.saturating_add(deposit).min(200);
        // Marks of the Departed scale 1 per boss kill.
        self.marks_of_departed = self
            .marks_of_departed
            .saturating_add(bosses_killed_this_run)
            .min(50);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_is_empty() {
        let m = SoulRemembrance::new();
        assert_eq!(m.champion_count, 0);
        assert_eq!(m.wisps, 0);
    }

    #[test]
    fn clearing_grants_champion_soul() {
        let mut m = SoulRemembrance::new();
        m.on_run_end(8, 100, 1);
        assert_eq!(m.champion_count, 1);
        assert_eq!(m.clears, 1);
        assert!(m.last_wager_gold >= 10, "10% of 100 = 10 gold");
    }

    #[test]
    fn dying_floor_4_grants_no_champion() {
        let mut m = SoulRemembrance::new();
        m.on_run_end(4, 50, 0);
        assert_eq!(m.champion_count, 0);
        assert!(m.last_wager_gold >= 5, "10% of 50 = 5 gold");
    }

    #[test]
    fn capstone_respects_max_caps() {
        let mut m = SoulRemembrance::new();
        for _ in 0..100 {
            m.on_run_end(8, 99999, 8);
        }
        assert_eq!(m.champion_count, 30);
        assert_eq!(m.last_wager_gold, 200);
        assert_eq!(m.marks_of_departed, 50);
    }

    #[test]
    fn ron_round_trip() {
        let m = SoulRemembrance {
            champion_count: 3,
            wisps: 1,
            marks_of_departed: 2,
            last_wager_gold: 84,
            clears: 1,
        };
        let ron_str = ron::to_string(&m).unwrap();
        let back: SoulRemembrance = ron::from_str(&ron_str).unwrap();
        assert_eq!(m, back);
    }
}
