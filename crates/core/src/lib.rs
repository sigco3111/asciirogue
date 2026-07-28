//! Core ECS components and common types for asciirogue.
//! No rendering, no I/O — pure game data.

#![deny(rust_2018_idioms)]

use serde::{Deserialize, Serialize};

/// World coordinate (tile, not pixel).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct TilePos(pub i32, pub i32);

/// Game tick (turn counter).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct Tick(pub u64);

/// Energy unit — entities gain energy each tick and act when full.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct Energy(pub i32);

/// Constant: energy required to spend a turn.
pub const ENERGY_PER_TURN: i32 = 100;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_threshold_baseline() {
        assert!(ENERGY_PER_TURN == 100);
    }
}
