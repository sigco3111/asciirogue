//! asciirogue — Korean-first TUI roguelike with half-block visuals.

use anyhow::{Context, Result};
use asciirogue_combat::{attack, ai};
use asciirogue_core::{
    vision, Ai, AiKind, Direction, FloorTheme, Health, Inventory, Item, Mana, Name,
    Player, Position, Renderable, Stats, TilePos, Viewshed,
};
use asciirogue_procgen::{bsp, Dungeon, Tile};
use asciirogue_render::{draw_world, wall_glyph};
use crossterm::event::{Event, KeyCode, KeyEventKind};
use crossterm::terminal::disable_raw_mode;
use crossterm::execute;
use crossterm::terminal::LeaveAlternateScreen;
use hecs::World;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph};
use std::env;
use std::io;

const MAP_W: i32 = 60;
const MAP_H: i32 = 24;
const VIEW_RADIUS: i32 = 8;

/// Game state — BSP dungeon + ECS world + cached viewshed.
struct App {
    dungeon: Dungeon,
    seed: u64,
    running: bool,
    /// Current floor (1..=28).
    floor: u32,
    /// Blocked sight cells computed once per level (true = wall/rock).
    blocks: Vec<bool>,
    world: World,
    player: hecs::Entity,
    /// Combat / event log (newest at back).
    messages: Vec<String>,
    /// Increments each tick to seed deterministic RNG.
    tick: u64,
    /// True when the boss for this floor is defeated; used to enable descent.
    boss_defeated: bool,
}

/// Total floors in the run (SPEC §7 — keeping to 8 for v0.4 demo).
const MAX_FLOORS: u32 = 8;
/// Boss appears on this floor (and every `BOSS_EVERY` after; in v0.4 we just
/// put one yellow wyrm at the bottom).
const BOSS_FLOOR: u32 = MAX_FLOORS;
const INVENTORY_MAX: usize = 8;

impl App {
    fn new(base_seed: u64) -> Self {
        Self::new_at(base_seed, 1)
    }

    fn new_at(base_seed: u64, floor: u32) -> Self {
        let theme = FloorTheme::from_depth(floor);
        // Per-floor seed: combine base + floor so each descent is deterministic.
        let seed = base_seed
            .wrapping_add(floor as u64 * 0x9E37_79B9_7F4A_7C15);
        let dungeon = bsp::generate(MAP_W, MAP_H, seed, bsp::Config::default());
        let mut world = World::new();

        // Spawn player in the first room, with inventory + resources.
        let (px, py) = dungeon
            .rooms
            .first()
            .map(|r| r.center())
            .unwrap_or((MAP_W / 2, MAP_H / 2));
        let player_pos = TilePos(px, py);
        let player = world.spawn((
            Position(player_pos),
            Player,
            Renderable::new('@', 0xFF_D6_5A),
            Viewshed::new(VIEW_RADIUS, MAP_W, MAP_H),
            Health::new(40 + 5 * (floor as i32 - 1)),
            Mana::new(15 + 2 * (floor as i32 - 1)),
            Stats::new(10, 10, 8, 8, 10),
            Inventory::new(INVENTORY_MAX),
            Name("모험가".into()),
        ));

        // Populate monsters and loot scaled by floor.
        populate_floor(&mut world, &dungeon, floor, theme);

        let blocks = build_blocks(&dungeon);
        let boss_defeated = floor > BOSS_FLOOR; // past-boss floors count as cleared
        let mut app = Self {
            dungeon,
            seed,
            running: true,
            floor,
            blocks,
            world,
            player,
            messages: vec![format!(
                "v0.4 {}F {} — h/j/k/l, arrows, yubn, > 내려가기",
                floor,
                theme.label()
            )],
            tick: 0,
            boss_defeated,
        };
        app.recompute_fov();
        app
    }

    fn floor_label(&self) -> String {
        format!("{}F {}", self.floor, FloorTheme::from_depth(self.floor).label())
    }

    fn log(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        if self.messages.len() >= 5 {
            self.messages.remove(0);
        }
        self.messages.push(msg);
    }

    fn handle(&mut self, ev: &Event) {
        if let Event::Key(k) = ev {
            if k.kind == KeyEventKind::Press {
                let dir = key_to_direction(&k.code);
                match k.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                        self.running = false;
                    }
                    KeyCode::Char('R') => {
                        // Restart the run from floor 1.
                        *self = App::new_at(self.seed, 1);
                    }
                    KeyCode::Char('r') => {
                        // Re-roll current floor seed (dev cheat).
                        let new_base = self.seed.wrapping_add(0x12345);
                        *self = App::new_at(new_base, self.floor);
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        // Debug: clear viewshed memory.
                        if let Ok(mut v) = self.world.get::<&mut Viewshed>(self.player) {
                            v.revealed.iter_mut().for_each(|r| *r = false);
                        }
                    }
                    KeyCode::Char('g') | KeyCode::Char('G') => {
                        if try_pickup(&mut self.world, self.player) {
                            self.log("아이템을 주웠습니다.");
                        } else {
                            self.log("주울 아이템이 없습니다.");
                        }
                    }
                    KeyCode::Char('i') | KeyCode::Char('I') => {
                        if let Some(msg) = use_first_potion(&mut self.world, self.player) {
                            self.log(msg);
                        } else {
                            self.log("사용할 물약이 없습니다.");
                        }
                    }
                    KeyCode::Char('>') => {
                        let next_floor = self.floor + 1;
                        if self.floor == BOSS_FLOOR && !self.boss_defeated {
                            self.log("보스를 먼저 처치하세요.");
                        } else if next_floor > MAX_FLOORS {
                            self.log("이미 던전 끝에 도달했습니다.");
                        } else {
                            // Keep the same base seed; descend.
                            let seed_save = self.seed;
                            *self = App::new_at(seed_save, next_floor);
                            self.log(format!("{}층으로 내려갑니다.", next_floor));
                        }
                    }
                    d if dir.is_some() => {
                        if let Some(dir) = dir {
                            self.try_player_act(dir);
                        }
                        let _ = d;
                    }
                    _ => {}
                }
            }
        }
    }

    /// Player attempts to move or attack adjacent enemy.
    fn try_player_act(&mut self, dir: Direction) {
        let cur = match self.world.get::<&Position>(self.player) {
            Ok(p) => p.0,
            Err(_) => return,
        };
        let (dx, dy) = dir.delta();
        if dx == 0 && dy == 0 {
            // wait
            self.tick = self.tick.wrapping_add(1);
            self.enemy_take_turns();
            return;
        }
        let next = TilePos(cur.0 + dx, cur.1 + dy);

        // Enemy in next tile? Try to attack.
        let target_entity = self.entity_at(next);
        if let Some(entity) = target_entity {
            self.player_attack(entity);
            self.tick = self.tick.wrapping_add(1);
            self.enemy_take_turns();
            return;
        }

        // Else try walking.
        if !is_passable(&self.dungeon, next.0, next.1) {
            return; // bump into wall — no turn consumed
        }
        if let Ok(mut p) = self.world.get::<&mut Position>(self.player) {
            p.0 = next;
        }
        self.recompute_fov();
        self.tick = self.tick.wrapping_add(1);
        self.enemy_take_turns();
    }

    fn entity_at(&self, pos: TilePos) -> Option<hecs::Entity> {
        let mut found = None;
        for (e, p) in self.world.query::<&Position>().iter() {
            if p.0 == pos {
                found = Some(e);
                break;
            }
        }
        found
    }

    fn player_attack(&mut self, target: hecs::Entity) {
        // Need attacker stats, defender stats, and a pseudo-random d100.
        let attacker_stats = match self.world.get::<&Stats>(self.player) {
            Ok(s) => *s,
            Err(_) => Stats::default(),
        };
        let target_stats = match self.world.get::<&Stats>(target) {
            Ok(s) => *s,
            Err(_) => Stats::default(),
        };
        let rng = ((self.tick as u32).wrapping_mul(2654435761)) >> 0;
        let outcome = attack::resolve_attack(&attacker_stats, &target_stats, rng, 0);
        if !outcome.hit {
            self.log("빗나감!");
            return;
        }
        let dealt = if let Ok(mut h) = self.world.get::<&mut Health>(target) {
            h.take(outcome.dmg)
        } else {
            0
        };
        let crit_marker = if outcome.crit { "치명타! " } else { "" };
        self.log(format!("{}{} 피해!", crit_marker, dealt));
        // remove dead enemies
        self.despawn_dead();
    }

    fn despawn_dead(&mut self) {
        // Determine which dead entities exist and whether any is the floor boss
        // (we tag bosses as having max HP > 100 — a heuristic but reliable given
        // our difficulty scaling).
        let mut dead: Vec<hecs::Entity> = self
            .world
            .query::<&Health>()
            .iter()
            .filter_map(|(e, h)| {
                if h.is_dead() {
                    Some(e)
                } else {
                    None
                }
            })
            .collect();
        let mut boss: Option<hecs::Entity> = None;
        for &e in &dead {
            if let Ok(h) = self.world.get::<&Health>(e) {
                if h.max > 100 {
                    boss = Some(e);
                    break;
                }
            }
        }
        let count = dead.len();
        for e in dead.drain(..) {
            let _ = self.world.despawn(e);
        }
        if count > 0 {
            self.log(format!("{} 처치!", count));
        }
        if boss.is_some() && !self.boss_defeated {
            self.boss_defeated = true;
            self.log(format!("{}층 보스 격파! '>' 내려가기.", self.floor));
        }
    }

    fn enemy_take_turns(&mut self) {
        // Snapshot player position once.
        let player_pos = match self.world.get::<&Position>(self.player) {
            Ok(p) => p.0,
            Err(_) => return,
        };
        // Iterate over all entities with Ai; for each, decide and act.
        // We can't borrow world immutably while mutating, so collect (entity, pos, ai) tuples first.
        let plan: Vec<(hecs::Entity, TilePos, Ai)> = self
            .world
            .query::<(&Position, &Ai)>()
            .iter()
            .map(|(e, (p, a))| (e, p.0, *a))
            .collect();
        for (entity, enemy_pos, ai_behav) in plan {
            let action = ai::take_turn(
                enemy_pos,
                player_pos,
                ai_behav.vision,
                ai_behav.kind,
                |x, y| is_passable(&self.dungeon, x, y)
                    && self.entity_at(TilePos(x, y)).is_none(),
            );
            match action {
                ai::AiAction::AttackPlayer => self.enemy_attack(entity),
                ai::AiAction::Step(d) => {
                    let (dx, dy) = d.delta();
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let np = TilePos(enemy_pos.0 + dx, enemy_pos.1 + dy);
                    if is_passable(&self.dungeon, np.0, np.1)
                        && self.entity_at(np).is_none()
                    {
                        if let Ok(mut p) = self.world.get::<&mut Position>(entity) {
                            p.0 = np;
                        }
                    }
                }
                ai::AiAction::Wait | ai::AiAction::Idle => {}
            }
        }
    }

    fn enemy_attack(&mut self, attacker: hecs::Entity) {
        let atk_stats = match self.world.get::<&Stats>(attacker) {
            Ok(s) => *s,
            Err(_) => Stats::default(),
        };
        let def_stats = match self.world.get::<&Stats>(self.player) {
            Ok(s) => *s,
            Err(_) => Stats::default(),
        };
        let rng = ((self.tick as u32).wrapping_mul(2654435761)) ^ 0xDEAD_BEEF;
        let outcome = attack::resolve_attack(&atk_stats, &def_stats, rng, 0);
        if !outcome.hit {
            self.log("적이 빗나감.");
            return;
        }
        if let Ok(mut h) = self.world.get::<&mut Health>(self.player) {
            h.take(outcome.dmg);
        }
        let crit = if outcome.crit { " (치명타!)" } else { "" };
        self.log(format!("적 명중! {} 피해{}", outcome.dmg, crit));
    }

    fn recompute_fov(&mut self) {
        let pos = if let Ok(p) = self.world.get::<&Position>(self.player) {
            p.0
        } else {
            TilePos(MAP_W / 2, MAP_H / 2)
        };
        let mut visible_buf: Vec<TilePos> = Vec::new();
        vision::compute(
            pos,
            self.dungeon.width,
            self.dungeon.height,
            VIEW_RADIUS,
            &self.blocks,
            &mut visible_buf,
        );
        // Detach borrow scope: read what we need, then take a fresh mutable borrow.
        let visible_for_reveal: Vec<TilePos> = visible_buf.clone();
        if let Ok(mut v) = self.world.get::<&mut Viewshed>(self.player) {
            v.visible = visible_buf;
            let w = self.dungeon.width;
            for &TilePos(x, y) in &visible_for_reveal {
                if x >= 0 && y >= 0 && x < w && y < self.dungeon.height {
                    v.revealed[(y * w + x) as usize] = true;
                }
            }
        }
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(area);

        // Header.
        let player_pos = if let Ok(p) = self.world.get::<&Position>(self.player) {
            p.0
        } else {
            TilePos::default()
        };
        let visible_count = if let Ok(v) = self.world.get::<&Viewshed>(self.player) {
            v.visible.len()
        } else {
            0
        };
        let hp_mp = if let (Ok(h), Ok(m)) = (
            self.world.get::<&Health>(self.player),
            self.world.get::<&Mana>(self.player),
        ) {
            format!("HP {}/{}  MP {}/{}", h.current, h.max, m.current, m.max)
        } else {
            String::from("?")
        };
        let title = Paragraph::new(Span::from(format!(
            "asciirogue — seed {}  {}  player ({},{})  visible {}  {}",
            self.seed,
            self.floor_label(),
            player_pos.0,
            player_pos.1,
            visible_count,
            hp_mp
        )))
        .block(Block::default().borders(Borders::ALL).title(" asciirogue "));
        frame.render_widget(title, chunks[0]);

        // Map area, centred.
        let map_chunk = chunks[1];
        let inner_w = map_chunk.width.min(MAP_W as u16);
        let inner_h = map_chunk.height.min(MAP_H as u16);
        // Snapshot viewshed before passing to the renderer to avoid holding a borrow
        // while we also borrow the world for entity iteration.
        let viewshed_snapshot = if let Ok(v) = self.world.get::<&Viewshed>(self.player) {
            Viewshed {
                range: v.range,
                visible: v.visible.clone(),
                revealed: v.revealed.clone(),
            }
        } else {
            Viewshed::new(VIEW_RADIUS, MAP_W, MAP_H)
        };
        let centered = ratatui::layout::Rect {
            x: map_chunk.x + (map_chunk.width.saturating_sub(inner_w)) / 2,
            y: map_chunk.y + (map_chunk.height.saturating_sub(inner_h)) / 2,
            width: inner_w,
            height: inner_h,
        };
        draw_world(frame, centered, &self.dungeon, &self.world, &viewshed_snapshot);

        // Footer — last few messages + control hint.
        let recent = self
            .messages
            .iter()
            .rev()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join("  |  ");
        let controls_hint = if self.boss_defeated && self.floor == BOSS_FLOOR {
            "g pick  i potion  > descend  R restart"
        } else {
            "g pick  i potion  q quit  r new  R restart"
        };
        let status = Paragraph::new(Span::from(format!(
            "[{}]  rooms {}  {}\n{}",
            recent,
            self.dungeon.rooms.len(),
            self.floor_label(),
            controls_hint,
        )))
        .block(Block::default().borders(Borders::ALL));
        frame.render_widget(status, chunks[2]);
    }
}

fn spawn_enemy(
    world: &mut World,
    dungeon: &Dungeon,
    room_idx: usize,
    glyph: char,
    name: &str,
    hp: i32,
    stats: Stats,
    vision: i32,
    kind: AiKind,
) {
    if let Some(room) = dungeon.rooms.get(room_idx) {
        let (ex, ey) = room.center();
        let fg = match glyph {
            'r' => 0xE6_82_82u32, // pink rat
            'g' => 0xB4_5A_D2u32, // magenta goblin
            _ => 0xFF_FF_FFu32,
        };
        world.spawn((
            Position(TilePos(ex, ey)),
            Renderable::new(glyph, fg),
            Health::new(hp),
            stats,
            Ai::new(kind, 100, vision),
            Name(name.to_string()),
        ));
    }
}

/// Spawn monsters + loot scaled by floor number (1..=MAX_FLOORS).
fn populate_floor(world: &mut World, dungeon: &Dungeon, floor: u32, theme: FloorTheme) {
    // Difficulty scales linearly: HP + attack bonus per floor.
    let scale = floor as i32; // 1..=8

    // Spawn 2 standard enemies per floor (in non-boss floors) + 1 boss on BOSS_FLOOR.
    let room_count = dungeon.rooms.len();
    if room_count >= 3 {
        let mut idx = 1_usize;
        // Tier 1: a rat in room[1] (always available, weakest).
        spawn_enemy(
            world,
            dungeon,
            idx.min(room_count - 1),
            'r',
            "쥐",
            6 + scale,
            Stats::new(3, 8, 1, 1, 5),
            8,
            AiKind::Coward,
        );
        idx += 1;
        // Tier 2: a goblin (or skeleton for catacombs+).
        let glyph2 = match theme {
            FloorTheme::Cave | FloorTheme::Garden => 'g',
            FloorTheme::Catacomb | FloorTheme::Library => 'S',
            FloorTheme::Forge | FloorTheme::Temple => 'T',
            FloorTheme::FinalMaw => 'D',
            FloorTheme::Boss => 'D',
        };
        let name2 = match glyph2 {
            'g' => "고블린",
            'S' => "스켈레톤",
            'T' => "트롤",
            'D' => "드래곤",
            _ => "적",
        };
        spawn_enemy(
            world,
            dungeon,
            idx.min(room_count - 1),
            glyph2,
            name2,
            14 + scale * 2,
            Stats::new(5, 6, 2, 2, 7),
            9,
            AiKind::Chase,
        );
    }
    // Spawn a tougher prowler if floor is mid-tier or beyond (room 3+).
    if room_count >= 4 && floor >= 3 {
        spawn_enemy(
            world,
            dungeon,
            3_usize.min(room_count - 1),
            'B',
            "곰",
            28 + scale * 2,
            Stats::new(9, 4, 1, 1, 12),
            8,
            AiKind::Chase,
        );
    }

    // Boss on BOSS_FLOOR.
    if floor == BOSS_FLOOR {
        if let Some(room) = dungeon.rooms.last() {
            let (bx, by) = room.center();
            world.spawn((
                Position(TilePos(bx, by)),
                Renderable::new('D', 0xFF_FF_40),
                Health::new(120 + scale * 8),
                Stats::new(12, 8, 6, 6, 16),
                Ai::new(AiKind::Chase, 100, 10),
                Name("황녀 🐲".into()),
            ));
        }
    }

    // Loot: drop a few items per floor (every 2nd room has a chest).
    for (i, room) in dungeon.rooms.iter().enumerate() {
        if i == 0 {
            continue;
        }
        if i % 2 == 0 {
            continue;
        }
        // Pick a free tile within the room.
        let cx = ((room.x1 + room.x2) / 2).max(room.x1 + 1);
        let cy = ((room.y1 + room.y2) / 2).max(room.y1 + 1);
        let item = match (floor + i as u32) % 4 {
            0 => Item::gold(10 + scale * 4),
            1 => Item::potion_hp(),
            2 => Item::potion_mp(),
            _ => Item::weapon(1 + scale, if scale >= 3 { "장검" } else { "단검" }),
        };
        // Render on ground: store as an entity with Item component (no health/AI),
        // and a derived glyph/fg.
        let kind_idx = match item.kind {
            asciirogue_core::ItemKind::Coin => 0,
            asciirogue_core::ItemKind::HealthPotion => 1,
            asciirogue_core::ItemKind::ManaPotion => 2,
            asciirogue_core::ItemKind::Weapon => 3,
            asciirogue_core::ItemKind::Armor => 4,
        };
        let glyphs = ['$', '!', '?', ')', '['];
        let fgs = [
            0xFF_D2_46u32,
            0x5A_DC_A0u32,
            0x5A_9C_F0u32,
            0xC8_C8_C8u32,
            0xB8_B8_BCu32,
        ];
        let _ = kind_idx; // enum discriminant kept for downstream extension
        world.spawn((
            Position(TilePos(cx, cy)),
            Renderable::new(glyphs[item.kind as usize], fgs[item.kind as usize]),
            item,
        ));
    }
}

/// Pick up an item lying on the ground at the player's tile (key `g`).
fn try_pickup(world: &mut World, player: hecs::Entity) -> bool {
    let player_pos = match world.get::<&Position>(player) {
        Ok(p) => p.0,
        Err(_) => return false,
    };
    // Collect entity ids at the player's tile that carry an Item.
    let mut item_entities: Vec<hecs::Entity> = Vec::new();
    let to_check: Vec<hecs::Entity> = world
        .query::<(&Position, &Item)>()
        .iter()
        .filter_map(|(e, (p, _))| if p.0 == player_pos { Some(e) } else { None })
        .collect();
    for e in to_check {
        item_entities.push(e);
    }
    if item_entities.is_empty() {
        return false;
    }

    // Pull the items out one at a time so each `Ref` is released before the
    // next borrow. Inventory is mutated AFTER all items are cloned.
    let mut items_to_put: Vec<Item> = Vec::with_capacity(item_entities.len());
    let mut picked = Vec::new();
    for e in item_entities {
        // Borrow `&Item`, clone, drop the Ref, then push to despawn queue.
        let cloned = match world.get::<&Item>(e) {
            Ok(r) => Some((*r).clone()),
            Err(_) => None,
        };
        if let Some(item) = cloned {
            items_to_put.push(item);
            picked.push(e);
        }
    }
    // Free refs before mutating Inventory.
    drop(world.get::<&Health>(player)); // no-op placeholder that runs; ok
    // Now push into Inventory.
    let mut inv = world.get::<&mut Inventory>(player).ok().unwrap();
    for item in items_to_put {
        if inv.push(item).is_err() {
            // Inventory full — keep the corresponding entity on the ground.
            if let Some(e) = picked.pop() {
                // We over-counted by one; ignore — leaving the item where it is.
                let _ = e;
            }
            break;
        }
    }
    drop(inv);
    let count = picked.len();
    for e in picked {
        let _ = world.despawn(e);
    }
    count > 0
}

/// Use the first available potion in the inventory (key `i`).
fn use_first_potion(world: &mut World, player: hecs::Entity) -> Option<String> {
    let mut inv = world.get::<&mut Inventory>(player).ok()?;
    let mut idx: Option<usize> = None;
    for (i, slot) in inv.slots.iter().enumerate() {
        if let Some(it) = slot {
            if matches!(it.kind, asciirogue_core::ItemKind::HealthPotion
                              | asciirogue_core::ItemKind::ManaPotion)
            {
                idx = Some(i);
                break;
            }
        }
    }
    let i = idx?;
    let item = inv.take(i)?;
    let msg = match item.kind {
        asciirogue_core::ItemKind::HealthPotion => {
            drop(inv);
            if let Ok(mut h) = world.get::<&mut Health>(player) {
                h.current = (h.current + item.bonus).min(h.max);
                format!("치유 물약 사용! HP +{}", item.bonus)
            } else {
                String::from("치유 물약...")
            }
        }
        asciirogue_core::ItemKind::ManaPotion => {
            drop(inv);
            if let Ok(mut m) = world.get::<&mut Mana>(player) {
                m.current = (m.current + item.bonus).min(m.max);
                format!("마나 물약 사용! MP +{}", item.bonus)
            } else {
                String::from("마나 물약...")
            }
        }
        _ => String::from("(potion이 아님)"),
    };
    Some(msg)
}

/// v0.1 — descents and saves are scaffolded. We support `>` to descend if boss
/// defeated, and `R` to restart the run from floor 1.

fn key_to_direction(k: &KeyCode) -> Option<Direction> {
    match k {
        // vim cardinals
        KeyCode::Char('h') => Some(Direction::Left),
        KeyCode::Char('j') => Some(Direction::Down),
        KeyCode::Char('k') => Some(Direction::Up),
        KeyCode::Char('l') => Some(Direction::Right),
        // yubn diagonals
        KeyCode::Char('y') => Some(Direction::UpLeft),
        KeyCode::Char('u') => Some(Direction::UpRight),
        KeyCode::Char('b') => Some(Direction::DownLeft),
        KeyCode::Char('n') => Some(Direction::DownRight),
        // arrow keys — 4-way only (no diagonal arrow on standard keyboards)
        KeyCode::Left => Some(Direction::Left),
        KeyCode::Right => Some(Direction::Right),
        KeyCode::Up => Some(Direction::Up),
        KeyCode::Down => Some(Direction::Down),
        // wait
        KeyCode::Char('.') => Some(Direction::None),
        _ => None,
    }
}

fn build_blocks(d: &Dungeon) -> Vec<bool> {
    let w = d.width as usize;
    let h = d.height as usize;
    (0..w * h)
        .map(|i| !matches!(d.tiles[i], Tile::Floor))
        .collect()
}

fn is_passable(d: &Dungeon, x: i32, y: i32) -> bool {
    matches!(d.at(x, y), Tile::Floor)
}

fn run() -> Result<()> {
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        let mut app = App::new(0xCAFE_BABE_DEAD_BEEF);
        let tick = std::time::Duration::from_millis(100);
        while app.running {
            terminal.draw(|f| app.draw(f))?;
            if crossterm::event::poll(std::time::Duration::from_millis(50))? {
                let ev = crossterm::event::read()?;
                app.handle(&ev);
            } else {
                std::thread::sleep(tick);
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}

fn dump_to_stdout(seed: u64, count: u32) -> Result<()> {
    for i in 1..=count {
        // Use App::new_at so each floor gets its own seed, theme, monsters and loot.
        let app = App::new_at(seed, i);
        let dungeon = &app.dungeon;
        let world = &app.world;
        let player = app.player;

        // FOV once so the dump mirrors what a player would see on arrival.
        let player_pos = match world.get::<&Position>(player) {
            Ok(p) => p.0,
            Err(_) => continue,
        };
        let blocks = build_blocks(dungeon);
        let mut visible_buf: Vec<TilePos> = Vec::new();
        vision::compute(player_pos, MAP_W, MAP_H, VIEW_RADIUS, &blocks, &mut visible_buf);
        let visible_set: std::collections::HashSet<(i32, i32)> = visible_buf
            .iter()
            .map(|&TilePos(x, y)| (x, y))
            .collect();

        // Build an entity lookup including items (rendered as their item glyph).
        let entity_at: std::collections::HashMap<(i32, i32), char> = world
            .query::<(&Position, &Renderable)>()
            .iter()
            .map(|(_, (p, r))| ((p.0.0, p.0.1), r.glyph))
            .collect();

        let theme = FloorTheme::from_depth(i);
        let boss_marker = if i == BOSS_FLOOR { " [BOSS]" } else { "" };
        println!(
            "── seed 0x{:016X}  {}F {}{}  rooms {}  player@({},{})  visible {} ──",
            app.seed,
            i,
            theme.label(),
            boss_marker,
            dungeon.rooms.len(),
            player_pos.0,
            player_pos.1,
            visible_set.len()
        );
        for y in 0..MAP_H {
            let mut line = String::with_capacity(MAP_W as usize + 16);
            for x in 0..MAP_W {
                if let Some(&glyph) = entity_at.get(&(x, y)) {
                    line.push(glyph);
                } else if visible_set.contains(&(x, y)) {
                    line.push(match dungeon.at(x, y) {
                        Tile::Wall => wall_glyph(&dungeon, x, y),
                        Tile::Floor => '.',
                        Tile::Rock => ' ',
                    });
                } else if matches!(dungeon.at(x, y), Tile::Wall | Tile::Rock) {
                    line.push(' ');
                } else {
                    line.push('·');
                }
            }
            println!("{}", line);
        }
        println!();
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let dump_mode = args.iter().any(|a| a == "--dump");
    if dump_mode {
        let count: u32 = args
            .windows(2)
            .find(|w| w[0] == "--count")
            .and_then(|w| w[1].parse().ok())
            .unwrap_or(3);
        let seed: u64 = args
            .windows(2)
            .find(|w| w[0] == "--seed")
            .and_then(|w| parse_seed(&w[1]).ok())
            .unwrap_or(0xCAFE_BABE_DEAD_BEEF);
        return dump_to_stdout(seed, count);
    }

    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("asciirogue — Korean-first TUI roguelike");
        eprintln!();
        eprintln!("Keys (vim / yubn / arrows):");
        eprintln!("  h j k l  — cardinal movement (vim)");
        eprintln!("  y u b n  — diagonal movement (yubn)");
        eprintln!("  ← ↓ ↑ →  — cardinal movement (arrow keys)");
        eprintln!("  .        — wait one turn");
        eprintln!("  c        — clear vision memory");
        eprintln!("  r        — new random seed (regenerate dungeon)");
        eprintln!("  q / Esc  — quit");
        eprintln!();
        eprintln!("Usage:");
        eprintln!("  asciirogue              Launch the TUI");
        eprintln!("  asciirogue --dump       Dump floors (with FOV) to stdout (no TTY required)");
        eprintln!("  asciirogue --dump --count 5 --seed 1  Dump 5 floors with custom seed");
        eprintln!("  asciirogue --help       Show this message");
        return Ok(());
    }

    // panic hook — restore the terminal first, then print the panic.
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen);
        original(info);
    }));
    run().context("asciirogue runtime error")
}

fn parse_seed(s: &str) -> Result<u64> {
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u64::from_str_radix(rest, 16).with_context(|| format!("invalid hex seed: {s}"));
    }
    s.parse::<u64>().with_context(|| format!("invalid decimal seed: {s}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vim_keys_map_to_directions() {
        assert_eq!(key_to_direction(&KeyCode::Char('h')), Some(Direction::Left));
        assert_eq!(key_to_direction(&KeyCode::Char('j')), Some(Direction::Down));
        assert_eq!(key_to_direction(&KeyCode::Char('k')), Some(Direction::Up));
        assert_eq!(key_to_direction(&KeyCode::Char('l')), Some(Direction::Right));
    }

    #[test]
    fn yubn_diagonals() {
        assert_eq!(key_to_direction(&KeyCode::Char('y')), Some(Direction::UpLeft));
        assert_eq!(key_to_direction(&KeyCode::Char('u')), Some(Direction::UpRight));
        assert_eq!(key_to_direction(&KeyCode::Char('b')), Some(Direction::DownLeft));
        assert_eq!(key_to_direction(&KeyCode::Char('n')), Some(Direction::DownRight));
    }

    #[test]
    fn arrow_keys_map_to_cardinals() {
        assert_eq!(key_to_direction(&KeyCode::Left), Some(Direction::Left));
        assert_eq!(key_to_direction(&KeyCode::Right), Some(Direction::Right));
        assert_eq!(key_to_direction(&KeyCode::Up), Some(Direction::Up));
        assert_eq!(key_to_direction(&KeyCode::Down), Some(Direction::Down));
    }

    #[test]
    fn wait_and_others() {
        assert_eq!(key_to_direction(&KeyCode::Char('.')), Some(Direction::None));
        // q / r / c / Esc are handled in App::handle directly — they don't map
        // to a direction (key_to_direction returns None).
        assert_eq!(key_to_direction(&KeyCode::Char('q')), None);
        assert_eq!(key_to_direction(&KeyCode::Char('r')), None);
        assert_eq!(key_to_direction(&KeyCode::Esc), None);
        assert_eq!(key_to_direction(&KeyCode::Char(' ')), None);
    }
}
