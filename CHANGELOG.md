# Changelog

All notable changes to asciirogue are documented here. Versions follow
[SemVer](https://semver.org/) loosely — pre-1.0 we bump on milestone.

## [Unreleased]

### v0.5.27 — Death screen overlay

The v0.5.25 death handler pushed summary messages but they were
buried in the footer log (which only shows the last 2 messages).
The user reported that the death screen looked empty: header
still said "asciirogue" (not "YOU DIED"), the body was blank,
and the footer was truncated to two log lines. v0.5.27 renders
a proper death overlay:

- New `draw_death_screen` method renders the death summary in
  the body area so the player sees the result of their run at
  a glance. Red bold title (`═══ ☠ YOU DIED ☠ ═══`), centered
  alignment, summary lines in white, restart hotkey highlighted.
- The map chunk is wiped with `Clear` when game_over so the
  death summary stands out cleanly against any stale dungeon.
- Header title already switched to ` ☠ YOU DIED ☠ ` in v0.5.25;
  v0.5.27 makes sure the body matches.
- Footer controls hint switches to `R = 새 게임, q = 종료` on
  death so the restart hotkey is visible without scrolling.

Test totals: 105 pass / 0 fail (was 104 pass / 0 fail; added 1
death-screen regression test).

## v0.5.26 — `!` dev cheat: drop a potion

Manual QA for the v0.5.25 death summary required walking the
player into an enemy at low HP — fragile because (a) enemy
positions vary by seed, (b) the player had to survive long enough
to *reach* the enemy, (c) the 1 HP setup was awkward. v0.5.26
adds a one-shot cheat key so we can pre-populate inventory
without depending on loot RNG.

Press `!` → drops a `치유 물약 (+15)` into the next free
inventory slot. Logs `[치트] 물약 추가: 치유 물약 (+15)` on
success, or `[치트] 인벤토리가 가득 차서 추가 실패` when
the inventory is full. Lives next to the other debug keys
(`r` re-roll seed, `c` clear viewshed, `~` stairs warp).

Test totals: 104 pass / 0 fail (was 102 pass / 0 fail; added
2 regression tests for the cheat's add / full-refusal paths).

## v0.5.25 — Player death event handling

Player death was previously a fire-and-forget event: `game_over =
true`, two log lines, no meta-progression saved. A death on floor
4 wiped out everything the player had done up to that point —
no marks of the departed for bosses killed, no last-wager gold
deposit. v0.5.25 treats death as a real end-of-run event:

- New `run_bosses_killed` counter on `App`, incremented when a
  floor boss dies.
- New `on_player_death()` method (idempotent — repeated calls
  don't double-deposit). Triggered by `enemy_take_turns` when
  the player's HP reaches zero.
- On death, calls `remembrance.on_run_end(floor, gold,
  run_bosses_killed)`. Even non-clearing runs now earn marks of
  the departed for every boss killed, and 10% of remaining gold
  (capped at 200) is deposited to the last-wager pool. The
  remembrance is persisted to disk.
- The log is replaced with a death summary so the player can
  read what they accomplished in this run:

  ```
  ═══ 쓰러졌습니다… ═══
  도달: 4F (4층) — 보스 2명 처치
  남긴 Gold: 130 — 영혼 +2 표시 / +13 골드 (마지막 노림전)
  R = 새 게임, q = 종료
  ```

Implementation notes: `descend_internal` still uses `on_run_end` for
the WIN path (clearing floor 8). `on_player_death` uses the same
method with the floor-at-time-of-death. The two paths share the
clearing-bonus logic via `on_run_end`'s `final_floor >= 8` check
(champions + clears only unlock on a full clear).

Test totals: 102 pass / 0 fail (was 98 pass / 0 fail; added 4
death-event regression tests in `crates/app/src/main.rs`).

## v0.5.24 — Boss-floor modal hint

The v0.5.23 boss gate keeps the modal open when descent is rejected,
but the popup itself still doesn't explain WHY the gate is firing
— the player has to read the buried log line. v0.5.24 adds an
in-popup hint on the boss floor:

```
┌▼ 내려가기 확인───────────────────────────────────────────┐
│                                                          │
│▼ 다음 층으로 내려갈까?  [Y]                              │
│                                                          │
│▶ [y] 예 — 다음 층으로                                    │
│  [n] 취소 — 머무름                                       │
│                                                          │
│※ 보스(D) 처치 후 이동 가능                               │
└──────────────────────────────────────────────────────────┘
```

The `D` glyph is rendered red+BOLD so the player knows which entity
gates the descent. English locale shows the equivalent
`※ Defeat boss (D) to descend`. Non-boss floors keep the original
5-row popup — the hint only appears where it's relevant.

Implementation notes: the popup height bumps from 7 to 9 rows on
the boss floor (2 borders + 7 inner rows including the hint).
Paragraph renders overflow silently, so the height calculation
must include the hint line or it gets cropped.

Test totals: 98 pass / 0 fail (was 96 pass / 0 fail; added 2 boss
hint regression tests in `crates/app/src/main.rs`).

## v0.5.23 — Boss-gate UX: modal stays open on rejection

The v0.5.10 boss gate in `descend_internal` already blocked
descent on the boss floor without defeating the boss — but the
modal closed anyway after `y` / Enter, leaving only a buried
log line. Players saw the popup vanish and assumed descent
succeeded. v0.5.23 fixes the feedback:

- `descend_internal` now returns `bool` (true = descended, false
  = blocked). Both the boss-gate path and the end-of-run path
  return false.
- The modal handler only closes the popup when descent succeeded.
  On boss-gate rejection, the popup stays open so the player sees
  the constraint and must explicitly cancel with `n` / `Esc`.
- Same UX for `Enter` with the "yes" choice.

`crates/app/src/main.rs` and `crates/app/src/lib.rs` both updated
to keep them in sync (the v0.5.3 lib+bin split is still the
underlying architectural drift).

Test totals: 96 pass / 0 fail (was 92 pass / 0 fail; added 2
boss-gate UX regression tests in `crates/app/src/main.rs`).

## v0.5.22 — Stairs modal: vertical navigation (Up/Down)

v0.5.21 added horizontal navigation (Left/Right) but the modal lays
out its two options vertically — `[y] 예` on row 1, `[n] 취소` on
row 2. Vertical arrows match the layout. Hotkeys `k`/`j` mirror the
game's existing `hjkl` movement convention.

| Key | Action |
|---|---|
| `↓` / `j` | Move cursor DOWN to "no / cancel" |
| `↑` / `k` | Move cursor UP to "yes / descend" |
| `Tab` | Toggle selection |
| `Enter` | Confirm current selection |
| `y` / `Y` | Direct hotkey: descend |
| `n` / `N` / `Esc` | Direct hotkey: cancel |

No test count change — the existing selection-state tests were
retargeted at the new keys (`arrow_up_moves_selection_to_yes`,
`arrow_down_moves_selection_to_no`) instead of being added.

## v0.5.21 — Stairs modal: visual selection state + arrow navigation

The v0.5.20 modal worked but had no visual feedback for which option
would be confirmed. Players had to guess which key to press. This
release adds a real selection UX:

- **Visual cursor `▶`** on the currently-selected option.
- **REVERSED style** on the selected option for unmistakable
  emphasis.
- **Arrow Left/Right** (and `h`/`l`) move the cursor between options.
- **Tab** toggles the selection.
- **Enter** confirms the currently-selected option.
- **y/Y** still descend directly; **n/N/Esc** still cancel directly.
- **Default selection is "yes / descend"** — the player walked to
  the stairs on purpose, descending is the natural choice; they
  navigate left to back out.

**Implementation.** `ModalMode::ConfirmStairs` is now
`ModalMode::ConfirmStairs { choice: bool }`. The `choice` field
carries the current selection. Modal triggers set it to `true`;
the handler rewrites it on navigation; the draw function reads it
to render the cursor + REVERSED style. Direct hotkeys (`y`/`n`)
close the modal without touching `choice`; Enter acts on the
current `choice`.

**Modal layout** (Korean locale):

```
┌▼ 내려가기 확인───────────────────────────────────────────┐
│                                                          │
│▼ 다음 층으로 내려갈까?  [Y]                              │
│                                                          │
│  ▶ [y] 예 — 다음 층으로                                  │   ← selected: REVERSED + ▶
│    [n] 취소 — 머무름                                     │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

Test totals: 92 pass / 0 fail (was 84 pass / 0 fail after v0.5.20;
added 8 selection-state regression tests in `crates/app/src/main.rs`).

## v0.5.20 — Stairs arrival modal: actual root-cause fix

**The bug.** Walking onto a `▼` tile never triggered the descent
confirmation modal. Nine patch versions (v0.5.11 → v0.5.19) tried to
fix this in `try_player_act`, the modal handler, the redraw counter,
recovery UX, the loot-on-stairs filter — every fix landed in the wrong
file. The bug was one layer up: `is_passable` in the binary's copy
of the helper did not include `Tile::StairsDown`, so `try_player_act`
returned at the wall-check before reaching the modal trigger. The
`~` debug warp bypasses `is_passable`, which is why the modal
handler looked like it worked.

**Root cause.** `crates/app/src/main.rs` is the binary (`cargo run`
uses `path = "src/main.rs"` per `Cargo.toml`). All v0.5.11–v0.5.19
patches landed in `crates/app/src/lib.rs`. The two files were forked
during the v0.5.3 `lib + bin` refactor and drifted apart. `lib.rs`
correctly matches `Floor | StairsDown`; `main.rs` only matched
`Floor`. The StairsDown variant was added in v0.5.10, and main.rs
never got the update.

**Fix.** One line in `crates/app/src/main.rs`:

```rust
fn is_passable(d: &Dungeon, x: i32, y: i32) -> bool {
    matches!(d.at(x, y), Tile::Floor | Tile::StairsDown) // was: Tile::Floor
}
```

A comment in `main.rs` now points to the lib.rs twin at line ~1668
so the next maintainer doesn't re-introduce the drift.

**Bonus.** The same session uncovered 7 pre-existing lib.rs test
failures ("attempt to multiply with overflow" at `new_at_with`'s
seed computation). The literal `0x9E37_79B9_7F4A_7C15` was being
type-inferred as `i64` because of multiplication-context precedence,
and the high bit set made it negative — `u64 * i64` overflowed.
Fixed both `main.rs` and `lib.rs` with explicit `.wrapping_mul`
and a `_u64` suffix on the literal.

**Tests added.** Four regression tests in `crates/app/src/main.rs`
under the new `v0.5.20: stairs arrival modal regression suite`
banner. Three document the architectural drift (so the next
maintainer doesn't delete them as redundant `lib.rs` coverage);
one drives `App::draw` through `ratatui::backend::TestBackend` to
prove the modal actually renders to screen, not just opens in state.

**Architectural follow-up (not done in this PR).**
`main.rs` and `lib.rs` are ~2150-line near-duplicates. The right
fix is to make `main.rs` a thin shim that calls
`asciirogue::run_main()` — then the next "patch landed in the wrong
file" bug can't happen.

Test totals: 84 pass / 0 fail (was 80 pass / 7 fail pre-fix).

## v0.5.19 — Stairs trigger survives a loot on the stairs tile

### v0.5.19 — Stairs trigger survives a loot on the stairs tile

User walkthrough exposed the real bug. When the player walked onto the
stairs tile, a ground item (loot) was frequently spawned there by
v0.5.x's loot logic. The stairs trigger code at the bottom of
try_player_act was unreachable because entity_at(next) returned the
loot entity, and player_attack returned early on `already_dead` (loot
has no Health component) before the function ever reached the modal
trigger.

Flow that was broken:
1. Player at (54, 20), stairs at (55, 20), loot at (55, 20)
2. `l` pressed → next = (55, 20)
3. entity_at((55, 20)) = Some(loot_entity)
4. player_attack(loot_entity): already_dead = true (no Health) → return
5. try_player_act returns; modal trigger never runs

v0.5.19 fix: filter entity_at to only enemies (entities with Health).
Ground items (no Health) fall through to the walking + pickup path,
which then executes the modal trigger at the bottom.

```rust
let target_entity = self.entity_at(next).filter(|&e| {
    self.world.get::<&Health>(e).is_ok()
});
```

After fix:
1. same setup
2. l pressed → next = (55, 20)
3. entity_at = Some(loot_entity), filter rejects (no Health)
4. Walking path: pick up loot, then modal trigger fires
5. Modal opens, player can confirm y/n

Added 2 tests in `loot_on_stairs_tests`:
- `stairs_trigger_fires_with_loot_on_stairs`
- `stairs_trigger_fires_without_loot`

Total: **80 passed / 0 failed**.

### v0.5.18 — Drop v0.5.16 recovery UX (infinite-modal trap)

User feedback: "▼에 접근해도 모달은 호출되지 않음. (이전과 같은 생태)"
and walkthrough shows the modal is reachable only via the `~` debug
warp, then `n` cancels but the modal comes back on the next frame
because v0.5.16's recovery UX re-triggers the modal when the player
is still on the stairs tile. The player then can't move at all because
the modal handler swallows every non-y/n key.

Diagnosis:
- `~` works → modal handler is fine
- `y` / `Enter` works → descend_internal works
- `n` works → modal closes (one frame)
- v0.5.16's "n on stairs → re-trigger" then reopens the modal
- After n, modal = ConfirmStairs → all other keys are ignored
- Player is stuck on the stairs tile with no way to move off
- They walk off? They can't: the modal swallows the move key first.

v0.5.18 removes the recovery UX entirely. After n/Esc:
- modal = Closed
- at_stairs = false
- The player can move freely. If they want to descend again, they
  walk off and back on (or press `~`).

Updated 2 tests in modal_recovery_tests and at_stairs_tests to
expect the new v0.5.18 behavior (modal closes, at_stairs cleared).
Added 2 new tests in v0_5_18_no_recovery_tests:
  - n_closes_modal_even_when_on_stairs
  - nondescend_keys_are_not_swallowed_after_n
- Total: **78 passed / 0 failed**.

### v0.5.17 — Modal handler debug log

User still reports modal interaction failing. Compared to $ (pickup)
and ? (help) which work: those handlers are simple `match k.code` blocks
without nested let-bindings. The modal handler had nested borrowing
(Ref + dungeon.at) which may have been making the borrow checker
complain in a way that surfaced as silent non-match.

v0.5.17 simplifies the modal handler:
- Single `match k.code` block with explicit y/n/Esc/Enter arms.
- Always log `[modal] key={:?}` so the user can verify the branch
  is reached. If the log appears, the modal handler is firing. If it
  doesn't, the modal state is being cleared before the key arrives.
- The recovery UX (n → re-trigger if still on stairs) is preserved.
- Total: **76 passed / 0 failed**.

### v0.5.16 — n-cancel recovery: re-show modal when still on stairs

User scenario: arrived at stairs, modal opened, pressed `n` to cancel,
then pressed `l` to step forward — but the modal never came back even
though the player was still on the stairs tile (the next step took
them OFF the stairs, so the natural arrival trigger didn't fire).

v0.5.16 adds **recovery UX**: after `n`/`Esc`, if the player is still
standing on the stairs tile, the modal re-opens immediately so they
can answer again. This is the "I pressed n by mistake" path.

- Modal handler checks `Tile::StairsDown` at the player's current
  position after cancel. If still on stairs, re-trigger
  `ModalMode::ConfirmStairs` with `modal_redraws = 4` and `at_stairs = true`.
- Updated 4 existing tests to move the player off stairs before
  testing the cancel path (otherwise the new behavior would falsely
  pass).
- Added 2 new tests in `modal_recovery_tests`:
  - `n_while_on_stairs_reopens_modal`
  - `y_answers_descend_and_clears`
- Total: **76 passed / 0 failed**.

### v0.5.15 — Stairs "▼ 도착!" badge + 4-frame visibility

- User verified (`~` debug warp) modal handler works correctly. The
  modal popup itself is fine — the real issue is that the player
  didn't notice they were 1 tile away from stairs (header showed
  `stairs: → ▼ 1`). v0.5.15 makes the arrival hard to miss.
- **New `at_stairs: bool` flag.** Set when the player steps onto a
  StairsDown tile (or teleports via `~`). Cleared on y/n answer or
  when the player walks off the stairs. Drives the header badge.
- **Header badge "▼ 도착! (y/n)"** appears whenever `at_stairs` is
  true. The player can never miss the prompt again.
- **modal_redraws 2 → 4 frames.** The popup is visible for ~33ms
  even at a typical 120 Hz render rate, well above the human
  subliminal ~5ms threshold.
- **4 new tests** in `at_stairs_tests`:
  - `debug_warp_sets_at_stairs`
  - `answering_y_clears_at_stairs`
  - `answering_n_clears_at_stairs`
  - `modal_redraws_is_four_frames`
- Total: **73 passed / 0 failed**.

### v0.5.14 — Modal 1-frame guarantee + debug stairs warp

- **Modal popup is now visible for at least one full frame.** The
  v0.5.11 modal could appear and disappear between two render calls
  without ever being visible to the player (a single-frame flash). We
  add an `App::modal_redraws: u8` counter that is set to 2 on stairs
  arrival and cleared on y/n. The DRAW function renders the modal
  whenever `modal_redraws > 0` OR `modal == ConfirmStairs`, so the
  popup is guaranteed one tick of visibility.
- **Walking onto stairs also logs a hint.** New log line
  *"▼ — 내려갈까? (y/n)"* appears in the message log so the player
  sees the prompt even if the centered popup is somehow occluded.
- **Debug warp key (`~` or backtick).** Teleports the player onto the
  current floor's stairs tile without walking the whole dungeon. This
  lets us exercise the same modal code path that a natural arrival
  uses, and gives the user a way to verify the popup is rendering.
- **3 new tests** in `modal_redraws_tests`:
  - `stairs_arrival_sets_modal_redraws_via_debug_warp`
  - `answering_y_clears_modal_redraws`
  - `answering_n_clears_modal_redraws`
- Total: **69 passed / 0 failed**.

### v0.5.13 — Stairs modal restyled + help text uniformity

- **Modal popup is now unmistakable.** The v0.5.11 popup was a 40×5
  plain box that was easy to miss when the dungeon behind it was busy.
  v0.5.13 widens it to 60% of the screen width, raises it to 7 rows,
  colors the border and title yellow, and renders **two `Clear` passes**
  (full screen + popup region) so the popup stands out cleanly even on
  top of bright terrain.
- **Help text consistency.** v0.5.11 removed the `>` shortcut but left
  `> descend` strings in several places (startup message, status bar
  controls hint, `?` help burst, `--help` CLI output). All of these
  now say `stairs: y/n` (ko) / `stairs: y/n` (en) to match the modal
  keymap.
- Total: **66 passed / 0 failed**.

### v0.5.12 — Stairs tile is never blocked on spawn

- **Boss repositioned off the stairs.** On BOSS_FLOOR the boss used to
  spawn at `rooms.last().center()` — exactly the same tile as the
  descent. The player could not walk onto ▼ until the boss was killed,
  and the layout felt like the stairs symbol was "under" the boss. The
  boss now spawns on a tile adjacent to the stairs inside the last room,
  so the descent is visible from the doorway.
- **Rat/goblin/bear no longer placed on the last room.** Small dungeons
  (rooms ≤ 3) used to land rats and goblins on the stairs tile via
  `idx.min(room_count-1)`. We now back off to the penultimate room when
  the chosen index would otherwise equal the last room index.
- **spawn_enemy nudges off the stairs.** If a room's center is the
  stairs tile for any other reason (e.g. a future 2-room minimum), the
  spawn walks the 8 neighbours until it finds a non-stairs tile inside
  the room.
- **Loot nudge.** Loot spawn also avoids the stairs tile centre.
- **Dungeon.stairs: Option<TilePos>** now published by `bsp::generate`
  so callers can avoid the descent without re-scanning tiles.
- **2 new regression tests** (`stairs_unblocked_tests`): sweep 30
  seeds on floor 1 and a fixed seed on BOSS_FLOOR, both asserting that
  no entity occupies the stairs tile.
- Total: **66 passed / 0 failed** (was 64 in v0.5.11).

### v0.5.11 — Stairs confirmation popup (no `>` shortcut)

- **Descent is now a popup, not a hotkey.** Walking onto a `▼` StairsDown
  tile triggers a centered modal: *"▼ 다음 층으로 내려갈까?"* (KO) /
  *"▼ Descend to the next floor?"* (EN). Press `y` or `Enter` to descend,
  `n` or `Esc` to stay on the floor. The previous `>` shortcut is
  removed — accidentally descending on the wrong tile is no longer
  possible.
- **Modal is a true mode.** While `ConfirmStairs` is open, movement keys
  (`hjkl`, `yubn`, arrows) are inert: the player must answer before any
  further action. This matches the existing inventory modal policy
  (v0.5.7) and prevents the enemy turn from interrupting the prompt.
- **New i18n keys.** `MsgStairConfirm`, `MsgStairDescendOk` (with `{}`
  floor placeholder), `MsgStairCancel` — every line of the prompt and
  the resulting log message is translated.
- **Code structure.** Descend logic was extracted into `descend_internal`
  so the modal handler and the (re-introducible) test fixture share
  one path. Boss gate and end-of-run rewards are unchanged from v0.5.10.
- **8 new tests** in `confirm_stairs_tests`:
  - `y_in_confirm_descends_to_next_floor`
  - `enter_in_confirm_descends_to_next_floor`
  - `n_in_confirm_cancels_without_descending`
  - `esc_in_confirm_cancels`
  - `other_keys_in_confirm_are_ignored`
  - `gt_key_no_longer_descends`
  - `walking_onto_stairs_opens_confirm_modal`
  - `walking_onto_stairs_triggers_confirm`
- Total: **64 passed / 0 failed** (was 56 in v0.5.10).

### v0.5.10 — StairsDown visible & gated + gold surfaced in both header & inventory

- **`Tile::StairsDown`** added to procgen. BSP generator now places exactly
  one `▼` tile in the last (farthest) room per floor — visible when discovered
  and **passable** (`is_passable` returns true). Previously there was no
  descent indicator; the player could advance floors only via the boss gate
  on F8. Now every floor has a visible, walkable descent point.
- **`>` key gated by location.** Descent now requires the player to be
  standing on a `StairsDown` tile. From any other tile, `>` warns
  *"주위에 계단이 없습니다."* (KO) / *"There are no stairs here."* (EN)
  via the new `i18n::Key::MsgStairs` key. Boss-first and already-at-bottom
  messages also moved to i18n keys (`MsgBossFirst`, `MsgAlreadyAtBottom`).
- **`stairs: ↘ ▼ 30` header hint.** Header now shows the direction and
  distance to the nearest stairs in addition to the pickup hint. BFS cap is
  intentionally generous (999) — there is exactly one stairs tile per floor,
  so always showing it is more useful than hiding it past 24 tiles.
- **Gold surfaced in header + inventory modal.** Following user feedback
  that bypass resources (gold, XP, keys) should be visible in both surfaces,
  the header line now reads `HP x/y  MP x/y  G N` and the inventory modal
  shows `보유 골드: N` next to the stat block. Previously the player could
  lose track of their wallet because it lived outside the inventory.
- **5 new tests** (3 for `nearest_stairs_info`, 2 for the descend gate).
  Total: **56 passed / 0 failed**.

## v0.5.0 (Week 5) — Meta progression + i18n + polish

5주 로드맵 (SPEC §14) 마지막 단계. 외부 사람이 플레이 가능한 빌드 마감.

핵심 산출물:
- **메타 진행 — Soul Remembrance** (`core/src/meta.rs`): 챔피언 영혼, 강령
  파편, 순교자 표시, 말년 재물, 증언 5종 데이터 모델. 캡은 SPEC §18 그대로
  (챔피언 30, 파편 8장, 표시 50, 재물 200 골드). 디스크 저장 = RON 직렬화,
  경로 `~/.local/share/asciirogue/remembrance.ron`. 키 `S`로 수동 저장,
  8층 클리어 시 `on_run_end` 자동 호출.
- **i18n** (`core/src/i18n.rs`): 한국어/영어 양방향. 한-글자 키(`ko`, `en`)와
  풀네임(`korean`, `english`) 모두 인식. `t_format()` 헬퍼로 `{}` 0개
  positional substitution (소수의 메시지만 placeholder). 키 `L`로 게임 중
  토글, `ASCIITROGUE_LANG` 환경변수로 시작 시 오버라이드.
- **v0.1 → v0.5 점진적 발전**: SPEC §14 모든 마일스톤 — BSP→ECS→전투→아이템
  & 인벤토리→메타+i18n 한 사이클 완성.
- **40 tests** 통과 (이전 32 + i18n 3 + meta 5). 0 warnings.

이제 외부 사람이 `cargo install --path .` 또는 로컬 빌드로 게임을 받아서
플레이할 수 있습니다. 첫 8층을 클리어하면 챔피언 영혼 1장 + 클리어 카운트
+ 1이 디스크에 저장되고, 다음 런의 몬스터 HP/공격에 1% 약화 보너스 (순교자 표시).

## v0.4.0 (Week 4) — Items + inventory + 8-floor themes + boss clear gate

8개 층 변주(1-4 동굴, 5-8 카타콤), 보스(`D` 황녀 🐲, 120+ HP) 8층에 등장,
처치 후 `>` 키로 내려가기. 아이템 시스템(골드/물약/무기), 인벤토리(8 슬롯),
자동 loot 드롭(짝수 룸마다). 보스 처치 감지는 `Health::max > 100` 휴리스틱.

## v0.3.0 (Week 3) — Combat + enemy AI + HP/MP + name + status log

hecs 컴포넌트(Health/Mana/Stats/Ai/Name/StatusEffects), 공격 굴림·치명타 공식,
BFS 적 AI(Chase/Coward), 자동 적 행동, 사망 메시지, 처치 카운트.

## v0.2.0 (Week 2) — ECS + keys + FOV

hecs 통합, doryen-fov shadowcasting 8-way, vim+yubn 키바인딩, 방향키 매핑
도 사용자 요청으로 추가(4-way). 첫 프레임에 player/viewshed/visible 헤더
표시.

## v0.1.0 (Week 1) — initial bootstrap

워크스페이스 5-crate, BSP 절차 생성, half-block wall_glyph, ratatui 첫
bin 컴파일. `cargo build --release` 성공, `cargo test` 통과.
