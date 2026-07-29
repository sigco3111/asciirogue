//! Headless gameplay probe — drives a deterministic AI bot and prints every
//! state/log for inspection. Used to debug the "치명타! 0 피해!" spam
//! without a TTY.
//!
//!   cargo run --release --example play_probe -p asciirogue

use asciirogue::App;
use asciirogue_core::{Ai, Direction, Health, Position, TilePos};

fn main() {
    let seed: u64 = 0xCAFE_BABE_DEAD_BEEF;
    let mut app = App::new(seed);
    println!("=== probe starting, seed={:x} ===", seed);
    show_init_state(&app);

    // Walk toward the nearest enemy. After each step, show messages if any.
    for turn in 1..=120 {
        let dir = dir_toward_enemy(&app);
        app.step(dir);
        let msgs = app.messages();
        // Show every turn — we want to debug.
        let hp = read_player_hp(&app);
        let pos = read_player_pos(&app);
        println!(
            "[t={:3}] player=({},{}) hp={}  msgs={:?}",
            turn, pos.0, pos.1, hp, msgs
        );

        if app_is_game_over(&app) {
            println!("=== game over detected at turn {} ===", turn);
            break;
        }
    }
    println!("\n=== done ===");
}

fn app_is_game_over(app: &App) -> bool {
    app.messages().iter().any(|m| m.contains("YOU DIED") || m.contains("═══ 쓰러졌습니다"))
}

fn dir_toward_enemy(app: &App) -> Direction {
    let mut player_pos = (0i32, 0i32);
    let mut first = true;
    for (_, p) in app.world_ref().query::<&Position>().iter() {
        if first {
            player_pos = (p.0.0, p.0.1);
            first = false;
        }
    }
    let mut enemy_pos: Option<(i32, i32)> = None;
    for (_, (p, _a)) in app.world_ref().query::<(&Position, &Ai)>().iter() {
        enemy_pos = Some((p.0.0, p.0.1));
    }
    let (ex, ey) = enemy_pos.unwrap_or((player_pos.0 + 1, player_pos.1));
    let dx = (ex - player_pos.0).signum();
    let dy = (ey - player_pos.1).signum();
    match (dx, dy) {
        (1, 0) => Direction::Right,
        (-1, 0) => Direction::Left,
        (0, 1) => Direction::Down,
        (0, -1) => Direction::Up,
        (1, 1) => Direction::DownRight,
        (-1, 1) => Direction::DownLeft,
        (1, -1) => Direction::UpRight,
        (-1, -1) => Direction::UpLeft,
        _ => Direction::None,
    }
}

fn read_player_pos(app: &App) -> TilePos {
    for (_, p) in app.world_ref().query::<&Position>().iter() {
        return p.0;
    }
    TilePos(0, 0)
}

fn read_player_hp(app: &App) -> i32 {
    let mut max_hp = 0i32;
    for (_, h) in app.world_ref().query::<&Health>().iter() {
        max_hp = max_hp.max(h.current);
        break;
    }
    max_hp
}

fn show_init_state(app: &App) {
    let hp = read_player_hp(app);
    let pos = read_player_pos(app);
    let enemies: Vec<(i32, i32)> = app
        .world_ref()
        .query::<(&Position, &Ai)>()
        .iter()
        .map(|(_, (p, _a))| (p.0.0, p.0.1))
        .collect();
    println!(
        "[init] player=({},{}) hp={}  enemies={:?}",
        pos.0, pos.1, hp, enemies
    );
}
