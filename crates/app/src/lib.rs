//! asciirogue — Korean-first TUI roguelike with half-block visuals.

use anyhow::{Context, Result};
use asciirogue_combat::{attack, ai};
pub use asciirogue_core::EquipSlot;
use asciirogue_core::{
    i18n::{self, Key as I18nKey, Locale},
    meta::SoulRemembrance,
    vision, Ai, AiKind, Direction, Equipment, FloorTheme, Health, Inventory, Item,
    ItemKind, Mana, Name, Player, Position, Renderable, Stats, TilePos, Viewshed,
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
/// Sight radius in tiles. Lower values mean the player has to actively seek
/// encounters instead of being sniped from across a corridor. Tightened
/// from 8 → 5 in v0.5.1 after play-testing revealed enemies chased the
/// player before they were ever visible.
const VIEW_RADIUS: i32 = 5;

/// Game state — BSP dungeon + ECS world + cached viewshed.
pub struct App {
    dungeon: Dungeon,
    seed: u64,
    running: bool,
    /// Current floor (1..=8).
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
    /// Active locale (toggle with `L`).
    locale: Locale,
    /// Persistent meta state (Soul Remembrance).
    remembrance: SoulRemembrance,
    /// Gold the player is carrying right now (collected from coin drops).
    gold: u32,
    /// True when the player has hit 0 HP. The game loop refuses all input
    /// except `R` (restart) and `q` (quit) until the user starts a new run.
    /// Saved-state dumps via `--dump` can still inspect the floor.
    game_over: bool,
    /// v0.5.7: top-level UI mode. While Closed, keys go to gameplay. While
    /// Open, keys go to the inventory modal and the world is frozen.
    modal: ModalMode,
}

/// v0.5.7: Modal sub-modes. Currently only Inventory, but the enum keeps
/// the door open for future screens (spellbook, character sheet, …).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ModalMode {
    Closed,
    Inventory { cursor: usize },
}

/// Total floors in the run (SPEC §7 — keeping to 8 for v0.4 demo).
const MAX_FLOORS: u32 = 8;
/// Boss appears on this floor (and every `BOSS_EVERY` after; in v0.4 we just
/// put one yellow wyrm at the bottom).
const BOSS_FLOOR: u32 = MAX_FLOORS;
const INVENTORY_MAX: usize = 8;

impl App {
    pub fn new(base_seed: u64) -> Self {
        let loc = Locale::from_code(&std::env::var("ASCIITROGUE_LANG").unwrap_or_default());
        Self::new_at_with(
            base_seed,
            1,
            load_remembrance(),
            loc,
            0,
        )
    }

    /// Convenience for tests: a fresh App with no saved meta.
    #[allow(dead_code)]
    pub fn new_at(base_seed: u64, floor: u32) -> Self {
        Self::new_at_with(base_seed, floor, SoulRemembrance::new(), Locale::Korean, 0)
    }

    pub fn new_at_with(
        base_seed: u64,
        floor: u32,
        remembrance: SoulRemembrance,
        locale: Locale,
        carried_gold: u32,
    ) -> Self {
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
            Equipment::new(),
            Name("모험가".into()),
        ));

        // Populate monsters and loot scaled by floor.
        populate_floor(&mut world, &dungeon, floor, theme);

        let blocks = build_blocks(&dungeon);
        let boss_defeated = floor > BOSS_FLOOR; // past-boss floors count as cleared

        // Compose an opening log line using the active locale.
        let startup_args: [&dyn std::fmt::Display; 2] =
            [&floor as &dyn std::fmt::Display, &theme.label() as &dyn std::fmt::Display];
        let startup_msg = format!(
            "{} — {}",
            t_format_with_locale(I18nKey::MsgFloorEntered, locale, &startup_args),
            match locale {
                Locale::Korean => "h/j/k/l, 화살표, yubn, i 인벤토리, p 포션, > 내려가기",
                Locale::English => "h/j/k/l, arrows, yubn, i inv, p potion, > to descend",
            }
        );

        let mut app = Self {
            dungeon,
            seed,
            running: true,
            floor,
            blocks,
            world,
            player,
            messages: vec![startup_msg],
            tick: 0,
            boss_defeated,
            locale,
            remembrance,
            gold: carried_gold,
            game_over: false,
            modal: ModalMode::Closed,
        };
        app.recompute_fov();
        app
    }

    pub fn floor_label(&self) -> String {
        format!("{}F {}", self.floor, FloorTheme::from_depth(self.floor).label())
    }

    pub fn log(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        if self.messages.len() >= 5 {
            self.messages.remove(0);
        }
        self.messages.push(msg);
    }

    pub fn handle(&mut self, ev: &Event) {
        if let Event::Key(k) = ev {
            if k.kind == KeyEventKind::Press {
                let dir = key_to_direction(&k.code);
                // When dead, only R (restart) or q (quit) do anything.
                if self.game_over {
                    match k.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                            self.running = false;
                        }
                        KeyCode::Char('R') => {
                            let rem = self.remembrance.clone();
                            let locale = self.locale;
                            *self = App::new_at_with(self.seed, 1, rem, locale, 0);
                        }
                        _ => {}
                    }
                    return;
                }
                match k.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                        self.running = false;
                    }
                    KeyCode::Char('R') => {
                        // Restart the run from floor 1.
                        let rem = self.remembrance.clone();
                        let locale = self.locale;
                        *self = App::new_at_with(self.seed, 1, rem, locale, 0);
                    }
                    KeyCode::Char('S') => {
                        // Save: persist current meta state to disk.
                        let _ = save_remembrance(&self.remembrance);
                        self.log(format!(
                            "영혼 기억 저장: {} 회 클리어, {} 챔피언 영혼",
                            self.remembrance.clears, self.remembrance.champion_count
                        ));
                    }
                    KeyCode::Char('L') => {
                        // Locale toggle.
                        self.locale = match self.locale {
                            Locale::Korean => Locale::English,
                            Locale::English => Locale::Korean,
                        };
                        self.log(match self.locale {
                            Locale::Korean => "언어: 한국어",
                            Locale::English => "Locale: English",
                        });
                    }
                    KeyCode::Char('?') => {
                        // In-screen help (one message burst).
                        self.log("h/j/k/l 이동 | yubn 대각 | 화살표 이동".to_string());
                        self.log("g/$ 줍기 | i 인벤토리 | p 포션 | > 내려가기 | R 새 게임 | q 종료".to_string());
                        self.log("S 영혼 저장 | L 언어 토글 | ? 도움말".to_string());
                    }
                    KeyCode::Char('r') => {
                        // Re-roll current floor seed (dev cheat).
                        let new_base = self.seed.wrapping_add(0x12345);
                        let rem = self.remembrance.clone();
                        let locale = self.locale;
                        let gold = self.gold;
                        *self = App::new_at_with(new_base, self.floor, rem, locale, gold);
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        // Debug: clear viewshed memory.
                        if let Ok(mut v) = self.world.get::<&mut Viewshed>(self.player) {
                            v.revealed.iter_mut().for_each(|r| *r = false);
                        }
                    }
                    KeyCode::Char('g') | KeyCode::Char('G') | KeyCode::Char('$') | KeyCode::Char(',') => {
                        // `g` is the documented pickup key; `$` (gold sign) and
                        // `,` are intuitive aliases the player will reach
                        // for when they see a $ glyph.
                        match pick_up_outcome(&mut self.world, self.player) {
                            PickupOutcome::Picked => {
                                self.log("아이템을 주웠습니다.");
                            }
                            PickupOutcome::NoItem => {
                                self.log("주울 아이템이 없습니다.");
                            }
                            PickupOutcome::InventoryFull => {
                                self.log("인벤토리가 가득 찼습니다.");
                            }
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
                            // Final-floor cleared: end-of-run rewards.
                            self.remembrance.on_run_end(MAX_FLOORS, self.gold, 1);
                            let _ = save_remembrance(&self.remembrance);
                            self.log(format!(
                                "쏠쏠리의 방 클리어! clears={}, soul_wisps={}",
                                self.remembrance.clears,
                                self.remembrance.wisps,
                            ));
                        } else {
                            // Mid-run descent — keep gold + remembrance.
                            let rem = self.remembrance.clone();
                            let locale = self.locale;
                            let gold = self.gold;
                            let seed_save = self.seed;
                            *self = App::new_at_with(seed_save, next_floor, rem, locale, gold);
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
    pub fn try_player_act(&mut self, dir: Direction) {
        // Player-turn boundary: clear all enemy cooldowns so any enemy that
        // attacked *two turns ago* is again eligible for one action this
        // turn. (We do NOT reset here — see end of function.)
        let cur = match self.world.get::<&Position>(self.player) {
            Ok(p) => p.0,
            Err(_) => return,
        };
        let (dx, dy) = dir.delta();
        if dx == 0 && dy == 0 {
            // wait — explicit no-op for this turn, but still advance the
            // game clock so enemies can act.
            self.tick = self.tick.wrapping_add(1);
            self.enemy_take_turns();
            self.clear_enemy_cooldowns();
            return;
        }
        let next = TilePos(cur.0 + dx, cur.1 + dy);

        // Enemy in next tile? Try to attack.
        let target_entity = self.entity_at(next);
        if let Some(entity) = target_entity {
            self.player_attack(entity);
            self.tick = self.tick.wrapping_add(1);
            self.enemy_take_turns();
            self.clear_enemy_cooldowns();
            return;
        }

        // Else try walking.
        if !is_passable(&self.dungeon, next.0, next.1) {
            return; // bump into wall — no turn consumed, no enemy moves
        }
        if let Ok(mut p) = self.world.get::<&mut Position>(self.player) {
            p.0 = next;
        }
        self.recompute_fov();
        self.tick = self.tick.wrapping_add(1);
        self.enemy_take_turns();
        self.clear_enemy_cooldowns();
    }

    /// Clear all enemy cooldowns. Called after `enemy_take_turns` so the
    /// NEXT player action gets a fresh enemy phase.
    fn clear_enemy_cooldowns(&mut self) {
        let _ = self.world.query::<&mut Ai>().iter().for_each(|(_, ai)| ai.cooldown = 0);
    }

    pub fn entity_at(&self, pos: TilePos) -> Option<hecs::Entity> {
        let mut found = None;
        for (e, p) in self.world.query::<&Position>().iter() {
            if p.0 == pos {
                found = Some(e);
                break;
            }
        }
        found
    }

    pub fn player_attack(&mut self, target: hecs::Entity) {
        // Need attacker stats, defender stats, and a pseudo-random d100.
        let attacker_stats = match self.world.get::<&Stats>(self.player) {
            Ok(s) => *s,
            Err(_) => Stats::default(),
        };
        let target_stats = match self.world.get::<&Stats>(target) {
            Ok(s) => *s,
            Err(_) => Stats::default(),
        };
        // If the target is already dead, skip the message (despawn_dead will
        // emit the "x 처치!" announcement separately).
        let already_dead = self
            .world
            .get::<&Health>(target)
            .map(|h| h.is_dead())
            .unwrap_or(true);
        if already_dead {
            return;
        }
        let rng = ((self.tick as u32).wrapping_mul(2654435761)) >> 0;
        // v0.5.7: equipment bonuses apply on hit / damage.
        let outcome = attack::resolve_attack(
            &attacker_stats,
            &target_stats,
            rng,
            attacker_stats.attack_bonus,
            target_stats.defense_bonus,
        );
        if !outcome.hit {
            self.log("빗나감!");
            return;
        }
        let dealt = if let Ok(mut h) = self.world.get::<&mut Health>(target) {
            h.take(outcome.dmg)
        } else {
            0
        };
        if dealt == 0 {
            // Target had < 1 HP from a previous strike this turn; let
            // despawn_dead report the kill instead of printing "0 피해!".
            self.despawn_dead();
            return;
        }
        let crit_marker = if outcome.crit { "치명타! " } else { "" };
        self.log(format!("{}{} 피해!", crit_marker, dealt));
        // remove dead enemies
        self.despawn_dead();
    }

    pub fn despawn_dead(&mut self) {
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
        // Death check on player.
        let player_died = self
            .world
            .get::<&Health>(self.player)
            .map(|h| h.is_dead())
            .unwrap_or(false);
        if player_died && !self.game_over {
            self.game_over = true;
            // Clear queue, push a single announcement that is hard to miss.
            self.messages.clear();
            self.messages.push("═══ 쓰러졌습니다… ═══".to_string());
            self.messages.push("R = 새 게임, q = 종료".to_string());
        }
    }

    pub fn enemy_take_turns(&mut self) {
        // Snapshot player position once.
        let player_pos = match self.world.get::<&Position>(self.player) {
            Ok(p) => p.0,
            Err(_) => return,
        };
        // We do NOT reset cooldowns here. The cooldown flag was set to 1 by
        // an enemy that just attacked last turn; it must persist until the
        // next player action creates a new turn. So a single enemy's attack
        // settles the entire turn's enemy phase; cooldown is cleared at the
        // *start* of try_player_act (the player-turn boundary).
        // Iterate over all entities with Ai; for each, decide and act.
        // Snapshot every enemy with Ai. We can't borrow world immutably while
        // mutating, so collect tuples first.
        let plan: Vec<(hecs::Entity, TilePos, Ai)> = self
            .world
            .query::<(&Position, &Ai)>()
            .iter()
            .map(|(e, (p, a))| (e, p.0, a.clone()))
            .collect();
        // For each enemy, compute fresh sight info and decide an action.
        // The sight_blocks closure answers "does this tile block LOS for the
        // enemy?" using the same dungeon passability that the player's FOV
        // uses. We also refresh Ai.last_known_player based on what the enemy
        // sees *now* (Chebyshev ≤ vision AND no wall between).
        // Also: only ONE enemy may attack this turn (enforce a global "one
        // hit per player action" rule so combat doesn't feel like a stun
        // machine). The first attacker is picked in iteration order.
        let mut attack_used = false;
        for (entity, enemy_pos, ai_behav) in plan {
            // Skip enemies that just attacked last turn — their cooldown is
            // still 1 until the player advances the turn.
            if ai_behav.cooldown > 0 {
                continue;
            }
            // Only ONE attack per call to enemy_take_turns.
            if attack_used {
                break;
            }
            let blocks = &self.blocks; // borrow
            let sight_blocks = |x: i32, y: i32| {
                let idx = ((y * self.dungeon.width) + x) as usize;
                idx < blocks.len() && blocks[idx]
            };
            let can_see_player_now = {
                let dx = (player_pos.0 - enemy_pos.0).abs();
                let dy = (player_pos.1 - enemy_pos.1).abs();
                dx.max(dy) <= ai_behav.vision
                    && los_clear(enemy_pos, player_pos, sight_blocks)
            };
            // Update last_known_player whenever the enemy sees the player.
            if can_see_player_now {
                if let Ok(mut ai) = self.world.get::<&mut Ai>(entity) {
                    ai.last_known_player = Some(player_pos);
                }
            }
            let action = ai::take_turn(
                enemy_pos,
                player_pos,
                ai_behav.vision,
                ai_behav.kind,
                if can_see_player_now {
                    Some(player_pos)
                } else {
                    ai_behav.last_known_player
                },
                |x, y| is_passable(&self.dungeon, x, y)
                    && self.entity_at(TilePos(x, y)).is_none(),
                sight_blocks,
            );
            match action {
                ai::AiAction::AttackPlayer => {
                    self.enemy_attack(entity);
                    if let Ok(mut ai) = self.world.get::<&mut Ai>(entity) {
                        ai.cooldown = 1;
                    }
                    attack_used = true;
                }
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

    pub fn enemy_attack(&mut self, attacker: hecs::Entity) {
        let atk_stats = match self.world.get::<&Stats>(attacker) {
            Ok(s) => *s,
            Err(_) => Stats::default(),
        };
        let def_stats = match self.world.get::<&Stats>(self.player) {
            Ok(s) => *s,
            Err(_) => Stats::default(),
        };
        let rng = ((self.tick as u32).wrapping_mul(2654435761)) ^ 0xDEAD_BEEF;
        // v0.5.7: equipment bonuses apply here too.
        let outcome = attack::resolve_attack(
            &atk_stats,
            &def_stats,
            rng,
            atk_stats.attack_bonus,
            def_stats.defense_bonus,
        );
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

    pub fn recompute_fov(&mut self) {
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

    pub fn draw(&self, frame: &mut ratatui::Frame) {
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
        .block({
            let mut b = Block::default().borders(Borders::ALL);
            if self.game_over {
                b = b.title(" ☠ YOU DIED ☠ ");
            } else {
                b = b.title(" asciirogue ");
            }
            b
        });
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
        // v0.5.5: highlight adjacent pickupable ground items in bold so
        // the player can tell at a glance which tile they're standing on.
        let mut near_pickup_tiles: std::collections::HashSet<(i32, i32)> =
            std::collections::HashSet::new();
        if let Ok(ppos) = self.world.get::<&Position>(self.player) {
            let px = ppos.0.0;
            let py = ppos.0.1;
            let neighbours = [(px - 1, py), (px + 1, py), (px, py - 1), (px, py + 1)];
            for (_, (pp, _it)) in self.world.query::<(&Position, &Item)>().iter() {
                let k = (pp.0.0, pp.0.1);
                if k == (px, py) || neighbours.contains(&k) {
                    near_pickup_tiles.insert(k);
                }
            }
        }
        draw_world(frame, centered, &self.dungeon, &self.world, &viewshed_snapshot, &near_pickup_tiles);

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

// ─── i18n helpers ────────────────────────────────────────────────────────────

/// Look up a translation key in the app's current locale and apply simple
/// `{}`-style positional substitution. For the small set of formats we use,
/// `{}` is replaced in order with the variadic args converted via Display.
#[allow(dead_code)]
pub fn t_format(app: &App, key: I18nKey, args: &[&dyn std::fmt::Display]) -> String {
    t_format_with_locale(key, app.locale, args)
}

/// Same as `t_format`, but takes the locale directly. Useful when we need to
/// format a string before `App` exists (e.g. inside `new_at_with`).
pub fn t_format_with_locale(
    key: I18nKey,
    locale: Locale,
    args: &[&dyn std::fmt::Display],
) -> String {
    let raw = i18n::t_for(key, locale);
    let mut out = String::with_capacity(raw.len() + 16);
    let mut arg_idx = 0;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' && chars.peek() == Some(&'}') {
            chars.next(); // consume '}'
            if let Some(a) = args.get(arg_idx) {
                use std::fmt::Write;
                let _ = write!(&mut out, "{}", a);
                arg_idx += 1;
            } else {
                out.push_str("{}");
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[allow(dead_code)]
pub fn log_t(app: &mut App, key: I18nKey, args: &[&dyn std::fmt::Display]) {
    let s = t_format(app, key, args);
    app.log(s);
}

// ─── save / load ──────────────────────────────────────────────────────────────

pub fn save_root() -> std::path::PathBuf {
    let mut p = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    p.push(".local");
    p.push("share");
    p.push("asciirogue");
    let _ = std::fs::create_dir_all(&p);
    p.push("remembrance.ron");
    p
}

pub fn save_remembrance(r: &SoulRemembrance) -> std::io::Result<()> {
    let path = save_root();
    let s = ron::to_string(r).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, s)
}

pub fn load_remembrance() -> SoulRemembrance {
    let path = save_root();
    match std::fs::read_to_string(&path) {
        Ok(s) => ron::from_str(&s).unwrap_or_default(),
        Err(_) => SoulRemembrance::new(),
    }
}

pub fn spawn_enemy(
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
pub fn populate_floor(world: &mut World, dungeon: &Dungeon, floor: u32, theme: FloorTheme) {
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
pub fn try_pickup(world: &mut World, player: hecs::Entity) -> bool {
    matches!(pick_up_outcome(world, player), PickupOutcome::Picked)
}

/// Distinguishes "no item here" from "inventory full" so the caller
/// can show a useful message either way.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PickupOutcome {
    /// Nothing on the ground to pick up at the player's tile.
    NoItem,
    /// Picked up at least one item this call.
    Picked,
    /// There are items here but the inventory is full — ground untouched.
    InventoryFull,
}

/// Tri-state pickup. Caller should display a distinct message per arm.
///
/// v0.5.6: pickup now covers the player's tile **and** its 4 cardinal
/// neighbours. The previous version only looked at the player's tile, which
/// matched auto-pickup-on-step semantics but felt broken for the explicit
/// `g` key — players expected "I see gold right there, I'll pick it up".
pub fn pick_up_outcome(world: &mut World, player: hecs::Entity) -> PickupOutcome {
    let player_pos = match world.get::<&Position>(player) {
        Ok(p) => p.0,
        Err(_) => return PickupOutcome::NoItem,
    };
    // v0.5.6: collect ground items on the player's tile and the 4 cardinal
    // neighbours. The player's own tile is tried first so auto-pickup
    // behaviour (which fires when the player steps onto an item) remains
    // deterministic — if both "here" and "next to me" have items, the one
    // we're standing on is the one that gets grabbed first.
    let here = player_pos;
    let neighbours = [
        TilePos(player_pos.0 - 1, player_pos.1),
        TilePos(player_pos.0 + 1, player_pos.1),
        TilePos(player_pos.0, player_pos.1 - 1),
        TilePos(player_pos.0, player_pos.1 + 1),
    ];
    let wanted: std::collections::HashSet<TilePos> =
        std::iter::once(here).chain(neighbours.iter().copied()).collect();
    // Items in entity-id order so it's stable across runs.
    let item_entities: Vec<hecs::Entity> = world
        .query::<(&Position, &Item)>()
        .iter()
        .filter_map(|(e, (p, _))| if wanted.contains(&p.0) { Some(e) } else { None })
        .collect();
    if item_entities.is_empty() {
        return PickupOutcome::NoItem;
    }

    // Clone each ground item out (one borrow at a time), remember which
    // entity each came from. We then push into Inventory one at a time;
    // the first failure (Inventory full) breaks and we leave every
    // remaining ground entity on the floor.
    let mut items_to_put: Vec<(hecs::Entity, TilePos, Item)> = Vec::new();
    for e in item_entities {
        if let Ok(r) = world.get::<&Item>(e) {
            if let Ok(p) = world.get::<&Position>(e) {
                items_to_put.push((e, p.0, (*r).clone()));
            }
        }
    }
    drop(world.get::<&Health>(player)); // release any lingering refs
    let mut inv = world.get::<&mut Inventory>(player).ok().unwrap();
    // Walk through items; on the first Inventory full, stop. Either all
    // got in (Picked) or none did (InventoryFull).
    let mut inserted = 0usize;
    let mut inventory_full = false;
    for (e, _pos, item) in items_to_put.drain(..) {
        match inv.push(item) {
            Ok(_) => inserted += 1,
            Err(()) => {
                inventory_full = true;
                // The item that failed to insert stays in `e` on the ground;
                // we don't despawn it. Items we already inserted also
                // remain in the inventory.
                let _ = e;
                break;
            }
        }
    }
    drop(inv);
    // Now despawn the ground entities we successfully inserted. Use the
    // exact `inserted` ground-item entities from the original ordering by
    // re-querying the same set of tiles — this preserves "item the player
    // is standing on is consumed first, neighbours after".
    if inserted > 0 {
        let to_despawn: Vec<hecs::Entity> = world
            .query::<(&Position, &Item)>()
            .iter()
            .filter_map(|(e, (p, _))| if wanted.contains(&p.0) { Some(e) } else { None })
            .take(inserted)
            .collect();
        for e in to_despawn {
            let _ = world.despawn(e);
        }
    }
    if inventory_full {
        PickupOutcome::InventoryFull
    } else if inserted > 0 {
        PickupOutcome::Picked
    } else {
        PickupOutcome::NoItem
    }
}

/// v0.5.7 — apply the equipped items' bonuses into the entity's `Stats`.
/// Called whenever an item is equipped, unequipped, or the inventory changes
/// in a way that might shift which item is worn. Sets `attack_bonus` to the
/// sum of weapon bonuses and `defense_bonus` to the sum of armor bonuses
/// (which currently both map to `Item.bonus`).
pub fn recompute_stats_from_equipment(world: &mut World, entity: hecs::Entity) {
    // We have to fetch Equipment and Inventory under non-overlapping borrows.
    let (atk, def) = {
        let equip = match world.get::<&Equipment>(entity) {
            Ok(e) => *e,
            Err(_) => return,
        };
        let inv = match world.get::<&Inventory>(entity) {
            Ok(i) => i,
            Err(_) => return,
        };
        let mut atk = 0i32;
        let mut def = 0i32;
        if let Some(idx) = equip.weapon {
            if let Some(Some(it)) = inv.slots.get(idx) {
                atk += it.bonus;
            }
        }
        if let Some(idx) = equip.armor {
            if let Some(Some(it)) = inv.slots.get(idx) {
                def += it.bonus;
            }
        }
        (atk, def)
    };
    if let Ok(mut s) = world.get::<&mut Stats>(entity) {
        s.attack_bonus = atk;
        s.defense_bonus = def;
    }
}

/// v0.5.7: Use the inventory item at `idx`. Returns a user-facing message.
///
/// Behaviour by kind:
/// - HealthPotion / ManaPotion → consume and apply
/// - Weapon / Armor            → "장비는 (e) 키로 장착하세요."
/// - Coin                      → consumed silently into the player's gold stash
///   (note: ground coins auto-convert at pickup time, so a coin in inventory
///   is unusual; we keep the path for completeness)
pub fn use_inventory_at(world: &mut World, player: hecs::Entity, idx: usize) -> Option<String> {
    let item = {
        let mut inv = world.get::<&mut Inventory>(player).ok()?;
        inv.take(idx)?
    };
    let msg = match item.kind {
        ItemKind::HealthPotion => {
            let healed = item.bonus;
            if let Ok(mut h) = world.get::<&mut Health>(player) {
                h.current = (h.current + healed).min(h.max);
                format!("치유 물약 사용! HP +{}", healed)
            } else {
                String::from("치유 물약...")
            }
        }
        ItemKind::ManaPotion => {
            let restored = item.bonus;
            if let Ok(mut m) = world.get::<&mut Mana>(player) {
                m.current = (m.current + restored).min(m.max);
                format!("마나 물약 사용! MP +{}", restored)
            } else {
                String::from("마나 물약...")
            }
        }
        ItemKind::Weapon | ItemKind::Armor => format!("장비는 (e) 키로 장착하세요. ({}{})", item.glyph, item.name),
        ItemKind::Coin => {
            format!("{} 골드 획득!", item.bonus)
        }
    };
    Some(msg)
}

/// v0.5.7: Equip the inventory item at `idx` into its matching slot.
/// Returns a user-facing message. If the slot is already occupied, the old
/// item is **swapped out** into the inventory (its slot stays the same, the
/// previously equipped item takes its old inventory slot). If the item kind
/// doesn't match any equipment slot, returns "장착할 수 없는 아이템".
pub fn equip_inventory_at(world: &mut World, player: hecs::Entity, idx: usize) -> Option<String> {
    let item = {
        let mut inv = world.get::<&mut Inventory>(player).ok()?;
        inv.take(idx)?
    };
    let kind = item.kind;
    let name = item.name.clone();
    let glyph = item.glyph;

    let target_slot = match kind {
        ItemKind::Weapon => Some(EquipSlot::Weapon),
        ItemKind::Armor => Some(EquipSlot::Armor),
        _ => None,
    };
    let Some(slot) = target_slot else {
        // Put it back — can't equip this.
        let mut inv = world.get::<&mut Inventory>(player).ok()?;
        let _ = inv.put_at(idx, item);
        return Some("장착할 수 없는 아이템".to_string());
    };

    // If the chosen slot is currently occupied, move the old equipment back
    // into this same inventory slot.
    let prev_slot = {
        let equip = world.get::<&Equipment>(player).ok()?;
        match slot {
            EquipSlot::Weapon => equip.weapon,
            EquipSlot::Armor => equip.armor,
        }
    };
    if let Some(prev_idx) = prev_slot {
        if prev_idx != idx {
            let mut inv = world.get::<&mut Inventory>(player).ok()?;
            if let Some(prev_item) = inv.take(prev_idx) {
                // Slot `idx` is about to receive the new item, so put the
                // old equipment anywhere EXCEPT `idx` (which will be filled
                // a moment later). Try the first free slot first; fall back
                // to pushing anywhere.
                let mut placed = false;
                for (i, slot) in inv.slots.iter_mut().enumerate() {
                    if i != idx && slot.is_none() {
                        *slot = Some(prev_item.clone());
                        placed = true;
                        break;
                    }
                }
                if !placed {
                    // Drop on the floor instead of failing the equip — the
                    // player should never lose gear because they equipped
                    // something over it. Release the inventory borrow before
                    // touching `world` for the spawn.
                    drop(inv);
                    let pos = world.get::<&Position>(player).ok()?.0;
                    let occupied = world
                        .query::<(&Position, &Item)>()
                        .iter()
                        .any(|(_, (p, _))| p.0 == pos);
                    if !occupied {
                        world.spawn((
                            Position(pos),
                            Renderable::new(prev_item.glyph, prev_item.fg_rgb),
                            prev_item,
                        ));
                    } else {
                        // Last resort: shove into any free slot.
                        let mut inv2 = world.get::<&mut Inventory>(player).ok()?;
                        if inv2.push(prev_item.clone()).is_err() {
                            // Truly no room — overwrite `idx` (player loses the
                            // old item, but they keep the new one).
                            let _ = inv2.put_at(idx, prev_item);
                        }
                    }
                }
            }
        }
    }

    // Place the new item back at `idx`, then record it in Equipment.
    {
        let mut inv = world.get::<&mut Inventory>(player).ok()?;
        if inv.put_at(idx, item).is_err() {
            return Some("인벤토리가 가득 찼습니다.".to_string());
        }
    }
    {
        let mut equip = world.get::<&mut Equipment>(player).ok()?;
        match slot {
            EquipSlot::Weapon => equip.weapon = Some(idx),
            EquipSlot::Armor => equip.armor = Some(idx),
        }
    }
    recompute_stats_from_equipment(world, player);
    let slot_name = match slot {
        EquipSlot::Weapon => "무기",
        EquipSlot::Armor => "방어구",
    };
    Some(format!("{} ({}{}) 장착!", slot_name, glyph, name))
}

/// v0.5.7: Unequip whatever currently fills the given equipment slot.
/// Returns the unequipped item to the inventory (or drops it if full).
pub fn unequip_slot(world: &mut World, player: hecs::Entity, slot: EquipSlot) -> Option<String> {
    let idx = {
        let equip = world.get::<&Equipment>(player).ok()?;
        match slot {
            EquipSlot::Weapon => equip.weapon?,
            EquipSlot::Armor => equip.armor?,
        }
    };
    let item = {
        let mut inv = world.get::<&mut Inventory>(player).ok()?;
        inv.take(idx)?
    };
    {
        let mut inv = world.get::<&mut Inventory>(player).ok()?;
        if inv.put_at(idx, item.clone()).is_err() {
            // No room — push anywhere.
            if inv.push(item).is_err() {
                return Some("인벤토리가 가득 차 해제할 수 없습니다.".to_string());
            }
        }
    }
    {
        let mut equip = world.get::<&mut Equipment>(player).ok()?;
        match slot {
            EquipSlot::Weapon => equip.weapon = None,
            EquipSlot::Armor => equip.armor = None,
        }
    }
    recompute_stats_from_equipment(world, player);
    let slot_name = match slot {
        EquipSlot::Weapon => "무기",
        EquipSlot::Armor => "방어구",
    };
    Some(format!("{} 해제!", slot_name))
}

/// v0.5.7: Toggle the item at `idx`. If it's currently equipped, unequip it;
/// otherwise equip it.
pub fn toggle_equip_at(world: &mut World, player: hecs::Entity, idx: usize) -> Option<String> {
    let slot = {
        let equip = world.get::<&Equipment>(player).ok()?;
        equip.slot_of(idx)
    };
    match slot {
        Some(s) => unequip_slot(world, player, s),
        None => equip_inventory_at(world, player, idx),
    }
}

/// v0.5.7: Drop the item at `idx` onto the player's tile. If the tile is
/// already occupied by another ground item, the drop fails.
pub fn drop_inventory_at(
    world: &mut World,
    player: hecs::Entity,
    idx: usize,
) -> Option<String> {
    let item = {
        let mut inv = world.get::<&mut Inventory>(player).ok()?;
        inv.take(idx)?
    };
    let pos = world.get::<&Position>(player).ok()?.0;
    // Check the tile is free.
    let occupied = world
        .query::<(&Position, &Item)>()
        .iter()
        .any(|(_, (p, _))| p.0 == pos);
    if occupied {
        // Put it back.
        let mut inv = world.get::<&mut Inventory>(player).ok()?;
        let _ = inv.put_at(idx, item);
        return Some("이 타일에는 이미 다른 아이템이 있습니다.".to_string());
    }
    world.spawn((Position(pos), Renderable::new(item.glyph, item.fg_rgb), item));
    Some("아이템을 바닥에 내려놓았습니다.".to_string())
}

/// Use the first available potion in the inventory (key `i`).
pub fn use_first_potion(world: &mut World, player: hecs::Entity) -> Option<String> {
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

pub fn key_to_direction(k: &KeyCode) -> Option<Direction> {
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

pub fn build_blocks(d: &Dungeon) -> Vec<bool> {
    let w = d.width as usize;
    let h = d.height as usize;
    (0..w * h)
        .map(|i| !matches!(d.tiles[i], Tile::Floor))
        .collect()
}

/// Cheap line-of-sight: walks from `a` toward `b` step by step, returning
/// false if any intermediate tile is sight-blocking or out of bounds.
pub fn los_clear(a: TilePos, b: TilePos, sight_blocks: impl Fn(i32, i32) -> bool) -> bool {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let dist = dx.abs().max(dy.abs());
    if dist == 0 {
        return true;
    }
    for i in 1..dist {
        let t = i as f32 / dist as f32;
        let px = a.0 + (dx as f32 * t).round() as i32;
        let py = a.1 + (dy as f32 * t).round() as i32;
        if sight_blocks(px, py) {
            return false;
        }
    }
    true
}

pub fn is_passable(d: &Dungeon, x: i32, y: i32) -> bool {
    matches!(d.at(x, y), Tile::Floor)
}

pub fn run() -> Result<()> {
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

pub fn dump_to_stdout(seed: u64, count: u32) -> Result<()> {
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


/// Re-export parse_seed from old main.rs as a library function.
pub fn parse_seed(s: &str) -> anyhow::Result<u64> {
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u64::from_str_radix(rest, 16)
            .map_err(|e| anyhow::anyhow!("invalid hex seed: {}", e));
    }
    s.parse::<u64>().map_err(|e| anyhow::anyhow!("invalid decimal seed: {}", e))
}

/// Entry point used by both `main.rs` (the actual binary) and any probe
/// or test binary. We keep the same body as the original main() so the TUI
/// behavior matches.



/// Library entrypoint: parses CLI args, dispatches `--dump` / `--help` /
/// panic hook + `run()`. Used by both the binary main and any test/probe
/// binary.
pub fn run_main() -> anyhow::Result<()> {
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
        eprintln!("  r        — new random seed (regenerate current floor)");
        eprintln!("  R        — restart the run from floor 1");
        eprintln!("  g        — pick up item under player");
        eprintln!("  i        — use first available potion");
        eprintln!("  >        — descend (after defeating the floor's boss)");
        eprintln!("  ?        — show help lines");
        eprintln!("  S        — save meta (Soul Remembrance) to disk");
        eprintln!("  L        — toggle locale (ko / en)");
        eprintln!("  q / Esc  — quit");
        eprintln!();
        eprintln!("Usage:");
        eprintln!("  asciirogue              Launch the TUI");
        eprintln!("  asciirogue --dump       Dump floors (with FOV) to stdout (no TTY required)");
        eprintln!("  asciirogue --dump --count 5 --seed 1  Dump 5 floors with custom seed");
        eprintln!("  asciirogue --help       Show this message");
        eprintln!();
        eprintln!("Environment:");
        eprintln!("  ASCIITROGUE_LANG=ko|en  Override startup locale (default: ko)");
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


// ── probe-friendly API ───────────────────────────────────────────────────────
//
// These methods exist so headless tests / examples can drive the game
// without a TTY. They are not part of the user-facing surface.

impl App {
    /// Take one turn in `dir` (same as `App::handle` with a Direction key).
    /// Returns true if a player turn was consumed.
    pub fn step(&mut self, dir: Direction) -> bool {
        self.try_player_act(dir);
        true
    }

    /// Access the world's ECS state for probes.
    pub fn world_ref(&self) -> &hecs::World {
        &self.world
    }

    /// Mutable access for probe / test code that needs to teleport the
    /// player or otherwise alter world state directly.
    pub fn world_mut(&mut self) -> &mut hecs::World {
        &mut self.world
    }

    /// Read-only view of the message log.
    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    /// Probe handle to the underlying dungeon.
    pub fn dungeon_ref(&self) -> &asciirogue_procgen::Dungeon {
        &self.dungeon
    }

    /// Current floor number (1-based).
    pub fn floor(&self) -> u32 {
        self.floor
    }

    /// Short summary of the nearest ground item, if any is reachable from the
    /// player. Returns `(distance_in_steps, first_step_direction, glyph)`.
    ///
    /// BFS over passable (Floor) tiles. Distances cap at `MAX_PICKUP_SEARCH`
    /// so a probe / render call doesn't walk the entire 60×24 dungeon every
    /// frame for a missing item.
    pub fn nearest_pickup_info(&self) -> Option<(usize, Direction, char)> {
        nearest_pickup_info(&self.dungeon, &self.world, self.player)
    }
}

/// Cap on how far we'll BFS for a nearby pickup — kept tight on purpose.
/// The whole point is "if you can see it, we'll tell you how far" — items
/// out of sight shouldn't clutter the header.
const MAX_PICKUP_SEARCH: usize = 24;

/// Walk the dungeon from the player until we hit a ground item (Item + Position
/// entity). Returns the BFS distance in tiles, the cardinal direction of the
/// first step toward it (so the header can say "↑ 4 steps"), and the item's
/// glyph so the header can show what kind it is (e.g. "$ 4").
pub fn nearest_pickup_info(
    dungeon: &Dungeon,
    world: &hecs::World,
    player: hecs::Entity,
) -> Option<(usize, Direction, char)> {
    let start = world.get::<&Position>(player).ok()?.0;
    if !is_passable(dungeon, start.0, start.1) {
        return None;
    }
    let w = dungeon.width;
    let h = dungeon.height;

    // Precompute the set of (x,y) tiles that hold a ground item.
    let mut item_tiles: std::collections::HashMap<(i32, i32), char> =
        std::collections::HashMap::new();
    for (_, (p, it)) in world.query::<(&Position, &Item)>().iter() {
        item_tiles.insert((p.0.0, p.0.1), it.glyph);
    }
    if item_tiles.is_empty() {
        return None;
    }
    // Item on the player's tile — trivially pickupable.
    if let Some(&g) = item_tiles.get(&(start.0, start.1)) {
        return Some((0, Direction::None, g));
    }

    // BFS over 4-neighborhood (we report cardinal direction of the FIRST step
    // only, so the cardinal-direction search matches what the player will do).
    let mut visited = vec![false; (w * h) as usize];
    let mut prev_dir: Vec<Direction> = vec![Direction::None; (w * h) as usize];
    let idx = |x: i32, y: i32| -> usize { (y * w + x) as usize };

    let mut frontier: std::collections::VecDeque<(i32, i32, usize)> =
        std::collections::VecDeque::new();
    frontier.push_back((start.0, start.1, 0));
    visited[idx(start.0, start.1)] = true;

    // Cardinal-step directions: matches `Direction::{Up,Down,Left,Right}` deltas.
    const NEIGHBORS: [(i32, i32, Direction); 4] = [
        (0, -1, Direction::Up),
        (0, 1, Direction::Down),
        (-1, 0, Direction::Left),
        (1, 0, Direction::Right),
    ];

    while let Some((x, y, d)) = frontier.pop_front() {
        if d >= MAX_PICKUP_SEARCH {
            break;
        }
        for (dx, dy, dir) in NEIGHBORS.iter() {
            let nx = x + dx;
            let ny = y + dy;
            if nx < 0 || ny < 0 || nx >= w || ny >= h {
                continue;
            }
            if !is_passable(dungeon, nx, ny) {
                continue;
            }
            let ni = idx(nx, ny);
            if visited[ni] {
                continue;
            }
            visited[ni] = true;
            // The first-step direction from start to (nx,ny) is `dir` if
            // we're one step out, otherwise inherited.
            let first_dir = if d == 0 { *dir } else { prev_dir[idx(x, y)] };
            prev_dir[ni] = first_dir;
            if let Some(&g) = item_tiles.get(&(nx, ny)) {
                return Some((d + 1, first_dir, g));
            }
            frontier.push_back((nx, ny, d + 1));
        }
    }
    None
}
