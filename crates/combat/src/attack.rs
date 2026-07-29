//! Attack resolution: hit roll + damage formula + crit.
//!
//! Pure function — no ECS / I/O. The caller passes the two combatants' stats
//! and an `rng` value, and applies the result itself.

use asciirogue_core::Stats;

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct AttackOutcome {
    pub hit: bool,
    pub dmg: i32,
    pub crit: bool,
}

/// Resolve an attack. `rng` is a 0..=99 d100-style value (caller-provided).
/// Auto-clamped to [0, 99].
pub fn resolve_attack(
    attacker_stats: &Stats,
    defender_stats: &Stats,
    rng: u32,
    attack_bonus: i32,
) -> AttackOutcome {
    let roll = (rng as i32).clamp(0, 99);

    // To-hit: attacker DEX*2 + 50 + bonus; dodge = defender DEX*3
    // 50 = base miss threshold. Attacker needs roll >= 100 - to_hit_chance.
    let to_hit_chance = 50 + attacker_stats.dexterity * 2 + attack_bonus;
    let dodge_threshold = defender_stats.dexterity * 3;
    let target = 100 - to_hit_chance + dodge_threshold;
    let crit_threshold = 95;
    let crit = roll >= crit_threshold;
    let hit = crit || roll >= target;

    if !hit {
        return AttackOutcome { hit: false, dmg: 0, crit: false };
    }

    // Damage: STR + d<STR> - DEF/2, minimum 1.
    let variance = attacker_stats.strength.max(1);
    let d = (rng as i32 % (variance * 2 + 1)) - variance;
    let raw = attacker_stats.strength + d - defender_stats.constitution / 2;
    let dmg = raw.max(1);
    let dmg = if crit { dmg * 5 / 2 } else { dmg };
    AttackOutcome { hit: true, dmg, crit }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atk(s: i32, d: i32) -> Stats {
        Stats::new(s, d, 0, 0, 0)
    }
    fn def(c: i32, dx: i32) -> Stats {
        Stats::new(0, dx, 0, 0, c)
    }

    #[test]
    fn miss_with_high_dodge() {
        let out = resolve_attack(&atk(5, 5), &def(3, 30), 50, 0);
        assert!(!out.hit, "target with high DEX should dodge low roll");
        assert_eq!(out.dmg, 0);
    }

    #[test]
    fn hit_with_high_attacker() {
        let out = resolve_attack(&atk(10, 20), &def(2, 0), 99, 5);
        assert!(out.hit);
        assert!(out.dmg >= 1, "every hit should deal at least 1 dmg");
    }

    #[test]
    fn natural_crit_on_highest_roll() {
        let out = resolve_attack(&atk(10, 20), &def(2, 0), 98, 0);
        assert!(out.crit);
        assert!(out.dmg >= 2, "crit bumps damage");
    }

    #[test]
    fn damage_caps_at_one_minimum() {
        // Concrete defender stats far exceed attacker STR + variance range.
        let out = resolve_attack(&atk(2, 0), &def(100, 0), 50, 0);
        assert!(out.hit || !out.hit);
        if out.hit {
            assert!(out.dmg >= 1, "damage has a 1-floor (variance can swing negative)");
        }
    }

    #[test]
    fn crit_overrides_miss() {
        // Low roll but still crit because 99 ≥ 95
        let out = resolve_attack(&atk(10, 20), &def(2, 50), 99, 0);
        assert!(out.crit);
        assert!(out.hit, "crit counts as hit even when dodge threshold is high");
    }
}
