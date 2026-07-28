//! Status effects placeholder.

#![deny(rust_2018_idioms)]

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StatusKind {
    Poison,
    Burn,
    Slow,
    Silence,
    Bleed,
}

#[derive(Copy, Clone, Debug)]
pub struct StatusEffect {
    pub kind: StatusKind,
    pub damage_per_turn: i32,
    pub turns_remaining: u32,
}

impl StatusEffect {
    pub fn tick(&mut self) -> i32 {
        if self.turns_remaining == 0 {
            return 0;
        }
        self.turns_remaining -= 1;
        self.damage_per_turn
    }
}
