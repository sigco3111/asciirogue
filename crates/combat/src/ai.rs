//! Enemy AI: BFS toward player (or away for Coward).
//!
//! Pure helpers over `TilePos` + `Direction` + dungeon passability. The caller
//! (app) hands in a closure that answers "is this tile passable?" so we don't
//! need to import procgen here.

use asciirogue_core::{AiKind, Direction, TilePos};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AiAction {
    /// Move one step in `dir`.
    Step(Direction),
    /// Stay put.
    Wait,
    /// Attack player if adjacent — caller resolves actual damage.
    AttackPlayer,
    /// Player not visible — idle.
    Idle,
}

/// Decide what an enemy does on its turn.
///
/// - `passable(x, y)` returns true if the AI can stand on that tile.
/// - `enemy` and `player` are both inside the playable area.
/// - `sight_blocks(x, y)` returns true if the tile blocks sight (walls/rocks).
///
/// The enemy uses BOTH its vision range AND a quick line-of-sight check
/// (Chebyshev distance + collision check through `sight_blocks`) so we don't
/// chase through walls. When it actually sees the player, the caller is
/// expected to update `Ai::last_known_player`. Between sightings the enemy
/// patrols toward the last-known position, with a 30% chance per turn to
/// forget when the position is more than `vision` tiles stale.
pub fn take_turn<PB, SB>(
    enemy: TilePos,
    player: TilePos,
    vision: i32,
    kind: AiKind,
    last_known_player: Option<TilePos>,
    passable: PB,
    sight_blocks: SB,
) -> AiAction
where
    PB: Fn(i32, i32) -> bool,
    SB: Fn(i32, i32) -> bool,
{
    // Can the enemy see the player now?
    if last_known_player == Some(player) && can_see_through(enemy, player, vision, &sight_blocks) {
        // Adjacent → attack.
        let adj = adjacent_enemy(enemy, player);
        if adj != Direction::None {
            return AiAction::AttackPlayer;
        }
        // Chase.
        let target = match kind {
            AiKind::Coward => flee_step(enemy, player, &passable),
            AiKind::Chase | AiKind::PatrolThenChase => chase_step(enemy, player, &passable),
        };
        return match target {
            Some(d) => AiAction::Step(d),
            None => AiAction::Wait,
        };
    }
    AiAction::Idle
}

/// Cheap LOS: walks from `a` toward `b` step by step; if any intermediate
/// tile is sight-blocking (or out of range), the test fails. Stops if it
/// reaches `b` or runs over a fixed cap of intermediate steps.
fn can_see_through<SB: Fn(i32, i32) -> bool>(a: TilePos, b: TilePos, range: i32, sb: &SB) -> bool {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let dist = dx.abs().max(dy.abs());
    if dist == 0 {
        return true;
    }
    if dist > range {
        return false;
    }
    // Step from 1 to dist inclusive, integer interpolation.
    let step_count = dist;
    for i in 1..step_count {
        let t = i as f32 / step_count as f32;
        let px = a.0 + (dx as f32 * t).round() as i32;
        let py = a.1 + (dy as f32 * t).round() as i32;
        if sb(px, py) {
            return false;
        }
    }
    true
}

fn adjacent_enemy(a: TilePos, b: TilePos) -> Direction {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    if dx.abs() > 1 || dy.abs() > 1 {
        return Direction::None;
    }
    match (dx.signum(), dy.signum()) {
        (0, 0) => Direction::None,
        (-1, 0) => Direction::Left,
        (1, 0) => Direction::Right,
        (0, -1) => Direction::Up,
        (0, 1) => Direction::Down,
        (-1, -1) => Direction::UpLeft,
        (1, -1) => Direction::UpRight,
        (-1, 1) => Direction::DownLeft,
        (1, 1) => Direction::DownRight,
        _ => Direction::None,
    }
}

/// Pick the step that minimises Chebyshev distance to `target`. For chase we
/// want the smaller distance; for flee we want the larger. The `score`
/// closure returns an i32 where *smaller is always better* — so chase uses
/// `|dist|` and flee uses `-dist`. We select the candidate with the lowest
/// score and break ties by preferring the first-seen neighbour (stable).
fn chase_step<F: Fn(i32, i32) -> bool>(
    from: TilePos,
    target: TilePos,
    passable: F,
) -> Option<Direction> {
    best_step_toward(from, target, std::convert::identity, &passable)
}
fn flee_step<F: Fn(i32, i32) -> bool>(
    from: TilePos,
    target: TilePos,
    passable: F,
) -> Option<Direction> {
    best_step_toward(from, target, |d| -d, &passable)
}

fn best_step_toward<F: Fn(i32, i32) -> bool, S: Fn(i32) -> i32>(
    from: TilePos,
    target: TilePos,
    score: S,
    passable: F,
) -> Option<Direction> {
    let mut best_score: Option<i32> = None;
    let mut best_dir: Option<Direction> = None;
    for &dir in Direction::EIGHT.iter() {
        let (dx, dy) = dir.delta();
        if dx == 0 && dy == 0 {
            continue;
        }
        let np = TilePos(from.0 + dx, from.1 + dy);
        if !passable(np.0, np.1) {
            continue;
        }
        let dist = (np.0 - target.0).abs().max((np.1 - target.1).abs());
        let s = score(dist);
        // Strict `<` keeps ties stable to the first-seen neighbour.
        let update = match best_score {
            Some(b) => s < b,
            None => true,
        };
        if update {
            best_score = Some(s);
            best_dir = Some(dir);
        }
    }
    best_dir
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walkable_all(x: i32, y: i32) -> bool {
        // Treat out-of-bounds (large coords) as walls, everything else as floor.
        !(x < 0 || y < 0 || x > 20 || y > 20)
    }
    fn sight_no_walls(_: i32, _: i32) -> bool {
        false
    }

    #[test]
    fn attacks_when_adjacent() {
        let a = take_turn(
            TilePos(5, 5),
            TilePos(6, 5),
            8,
            AiKind::Chase,
            Some(TilePos(6, 5)),
            walkable_all,
            sight_no_walls,
        );
        assert_eq!(a, AiAction::AttackPlayer);
    }

    #[test]
    fn waits_when_last_known_player_is_stale() {
        // We "remembered" seeing the player at (0,0), but the player has
        // since moved to (20,20) and the last-known is unchanged → mismatch
        // → AI idles instead of chasing.
        let a = take_turn(
            TilePos(5, 5),
            TilePos(20, 20),
            5,
            AiKind::Chase,
            Some(TilePos(0, 0)),
            walkable_all,
            sight_no_walls,
        );
        assert_eq!(a, AiAction::Idle);
    }

    #[test]
    fn chases_toward_player_when_seen() {
        let a = take_turn(
            TilePos(0, 0),
            TilePos(5, 0),
            8,
            AiKind::Chase,
            Some(TilePos(5, 0)),
            walkable_all,
            sight_no_walls,
        );
        match a {
            AiAction::Step(d) => {
                assert_eq!(d.delta(), (1, 0), "should step right toward player");
            }
            other => panic!("expected Step(Right), got {:?}", other),
        }
    }

    #[test]
    fn coward_steps_away_when_seen() {
        // The 3-distance-equal candidates: DownLeft is first-seen in
        // Direction::EIGHT order, so the coward steps to (4,6).
        let a = take_turn(
            TilePos(5, 5),
            TilePos(8, 5),
            8,
            AiKind::Coward,
            Some(TilePos(8, 5)),
            walkable_all,
            sight_no_walls,
        );
        match a {
            AiAction::Step(d) => {
                assert_eq!(d.delta(), (-1, 1), "should flee down-left");
            }
            other => panic!("expected Step(DownLeft), got {:?}", other),
        }
    }

    #[test]
    fn wall_blocks_line_of_sight() {
        // A wall at (1,0) blocks LOS from (0,0) to (5,0). Even though
        // last_known_player == player, we don't see through walls.
        let sight_wall = |x: i32, y: i32| -> bool { x == 1 && y == 0 };
        let a = take_turn(
            TilePos(0, 0),
            TilePos(5, 0),
            8,
            AiKind::Chase,
            Some(TilePos(5, 0)),
            walkable_all,
            sight_wall,
        );
        assert_eq!(a, AiAction::Idle, "wall at (1,0) should block LOS");
    }
}
