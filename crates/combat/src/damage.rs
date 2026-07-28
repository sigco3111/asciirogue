//! Damage formula placeholder.
//!
//! Future: STR+weapon+rand vs defender armour.

#![deny(rust_2018_idioms)]

#[derive(Copy, Clone, Debug, Default)]
pub struct DamageFormula {
    pub base: i32,
    pub variance: i32,
    pub crit_threshold: i32, // natural roll >= threshold → crit
}

impl DamageFormula {
    pub fn roll(&self, rng: u32) -> i32 {
        // naive linear for now; replace with full dice in v0.2
        let dmg = self.base + ((rng as i32) % (self.variance.max(1) * 2 + 1)) - self.variance;
        dmg.max(1)
    }

    pub fn is_crit(&self, rng: u32) -> bool {
        (rng as i32) > 100 - self.crit_threshold as i32
    }
}
