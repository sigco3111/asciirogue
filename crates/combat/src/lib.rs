//! Combat system for asciirogue: attack resolution, energy queues, AI tick.
//!
//! Two pure-function entry points:
//! - `attack::resolve_attack` — given `attacker_stats + defender_stats`, returns
//!   `AttackOutcome { hit, dmg, crit }` deterministically given an RNG seed.
//! - `ai::take_turn` — given an enemy's position, the player's position, and
//!   the dungeon (passability), tries to step toward or flee from the player.

#![deny(rust_2018_idioms)]

pub mod attack;
pub mod ai;

pub use attack::{resolve_attack, AttackOutcome};
pub use ai::{take_turn, AiAction};
