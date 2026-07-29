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
/// `passable(x, y)` returns true if the AI can stand on that tile.
/// `enemy` and `player` are both inside the playable area.
pub fn take_turn<F: Fn(i32, i32) -> bool>(
    enemy: TilePos,
    player: TilePos,
    vision: i32,
    kind: AiKind,
    passable: F,
) -> AiAction {
    if !can_see(enemy, player, vision) {
        return AiAction::Idle;
    }

    let adj = adjacent_enemy(enemy, player);
    if adj != Direction::None {
        return AiAction::AttackPlayer;
    }

    // BFS pathfind one step toward (or away from) player.
    let target = match kind {
        AiKind::Coward => flee_step(enemy, player, &passable),
        AiKind::Chase | AiKind::PatrolThenChase => chase_step(enemy, player, &passable),
    };

    match target {
        Some(d) => AiAction::Step(d),
        None => AiAction::Wait,
    }
}

fn can_see(a: TilePos, b: TilePos, range: i32) -> bool {
    let dx = (a.0 - b.0).abs();
    let dy = (a.1 - b.1).abs();
    dx.max(dy) <= range
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

    #[test]
    fn attacks_when_adjacent() {
        let a = take_turn(
            TilePos(5, 5),
            TilePos(6, 5),
            8,
            AiKind::Chase,
            walkable_all,
        );
        assert_eq!(a, AiAction::AttackPlayer);
    }

    #[test]
    fn waits_when_player_outside_vision() {
        let a = take_turn(
            TilePos(5, 5),
            TilePos(20, 20),
            5,
            AiKind::Chase,
            walkable_all,
        );
        assert_eq!(a, AiAction::Idle);
    }

    #[test]
    fn chases_toward_player_when_visible() {
        let a = take_turn(
            TilePos(0, 0),
            TilePos(5, 0),
            8,
            AiKind::Chase,
            walkable_all,
        );
        match a {
            AiAction::Step(d) => {
                assert_eq!(d.delta(), (1, 0), "should step right toward player");
            }
            other => panic!("expected Step(Right), got {:?}", other),
        }
    }

    #[test]
    fn coward_steps_away_from_player() {
        // Distance to (8,5) from (5,5):
        //  - Left(-1,0) → (4,5), Chebyshev 4
        //  - UpLeft(-1,-1) → (4,4), Chebyshev 4
        //  - DownLeft(-1,1) → (4,6), Chebyshev 4
        // All three tie at the maximum distance; the AI picks the first one
        // encountered in `Direction::EIGHT` order, which is DownLeft.
        let a = take_turn(
            TilePos(5, 5),
            TilePos(8, 5),
            8,
            AiKind::Coward,
            walkable_all,
        );
        match a {
            AiAction::Step(d) => {
                assert_eq!(d.delta(), (-1, 1), "should flee down-left away from player");
            }
            other => panic!("expected Step(DownLeft), got {:?}", other),
        }
    }
}
