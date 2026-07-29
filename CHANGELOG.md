# Changelog

All notable changes to asciirogue are documented here. Versions follow
[SemVer](https://semver.org/) loosely — pre-1.0 we bump on milestone.

## [Unreleased]

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
