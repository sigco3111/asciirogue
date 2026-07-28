//! Combat system placeholder. v0.1: types only.

#![deny(rust_2018_idioms)]

pub mod damage;
pub mod status;

pub use damage::DamageFormula;
pub use status::{StatusEffect, StatusKind};
