# asciirogue

한국어 우선 · TUI · half-block 비주얼 · 절차 생성 던전 로그라이크 (Rust)

> Korean-first TUI roguelike with Unicode half-block visuals, written in Rust.

---

## ✨ 컨셉

`asciirogue`는 텍스트 터미널에서 동작하는 로그라이키 던전 크롤러입니다.
**Unicode 절반블록**(`▀▄▌▐█░▒▓`)으로 벽을 그려 픽셀 2배 해상도를 만들고, **`crossterm` + `ratatui`** 위에 `hecs` ECS로 게임 로직을 구성합니다.

- **Trogue + Brogue + Slay-the-Spire**의 하이브리드 — 텍스트 표현, 절차 생성, 영구 메타 진행
- 한국어 우선 UX — `ko` 로케일 기본
- 멀티태스킹 친화 — **`Auto-Pilot`** (§19)로 알아서 진행, 필요한 순간만 멈춤
- **Knob 슬라이더 15종** (§20)으로 매 런 규칙을 미세 조정

시각 샘플과 비교:

| ASCII | Half-block | Braille |
|---|---|---|
| 옛날 roguelike 감성, 가장 단순 | **이 프로젝트의 채택 결** — 픽셀 2배 | 픽셀 4배, 데이터 시각화 영역 |

샘플 비교 PNG는 `crates/procgen/examples/dump.rs` 실행으로 직접 확인하거나 디자인 PNG는 [`/Users/mac/work/end_of_eden/visual_samples/`](../end_of_eden/visual_samples/) 참고.

---

## 📦 빌드

필요: **Rust 1.74+ (stable)**.

```bash
git clone https://github.com/sigco3111/asciirogue
cd asciirogue
cargo run                  # 메인 게임 (현재: BSP 던전 + 키바인딩 데모)
cargo test                 # 8개 테스트
cargo run --release        # 최적화 빌드 (게임 루프 매끄러움)
# TTY 없이 던전 미리보기:
cargo run --release -- --dump --count 3 --seed 0xCAFE
```

### 모듈 구조

```
asciirogue/
├── crates/
│   ├── core/       # ECS 컴포넌트, TilePos/Energy/Tick
│   ├── procgen/    # BSP 던전 생성, 결정성 RNG
│   ├── combat/     # DamageFormula, StatusEffect (5종)
│   ├── render/     # wall_glyph (4-bit 마스크), ratatui 프레젠터
│   └── app/        # 메인 binary + 키바인딩 (q=quit, r=재시드)
├── Cargo.toml      # 워크스페이스
├── README.md
├── LICENSE         # MIT
└── .gitignore
```

---

## 🎮 컨트롤

> **골드(`$`)** 는 인벤토리를 차지하지 않습니다 — 픽업 즉시 `App::gold`(지갑)에 합산되고,
> 인벤토리 슬롯은 비어 있습니다. 포션·무기·방어구만 8슬롯에 들어갑니다.

| 키 | 동작 |
|---|---|
| `q` / `Q` / `Esc` | 종료 |
| `r` | 현재 층 시드 재굴림 |
| `R` | 런 처음(1층)부터 다시 |
| `h` `j` `k` `l` | 좌/하/상/우 이동 (vim) |
| `y` `u` `b` `n` | 대각 이동 (yubn) |
| `←` `↓` `↑` `→` | 좌/하/상/우 이동 (방향키, 4-way) |
| `.` | 한 턴 대기 |
| `g` | 아이템 줍기 |
| `i` | 인벤토리 모달 열기 (장착/해제/사용/버리기) |
| `p` | 빠른 포션 사용 (HP/MP 회복) |
| `k` / `K` | **노브 슬라이더 모달** (SPEC §20 — 16개 슬라이더로 게임 룰 미세 조정; v0.6.0+) |
| `y` / `n` | **계단 팝업**: 내려가기 / 취소 (v0.5.11: `>` 단축키 제거) |
| `c` | 시야 기억 초기화 (FOV 리셋) |
| `S` | 메타 진행(영혼 기억) 디스크 저장 |
| `L` | 언어 토글 (ko ↔ en) |
| `?` | 인게임 도움말 |
| `ASCIITROGUE_LANG=ko\|en` | 환경변수 (시작 시 로케일) |

### 다국어

**한국어 우선**. `--help` 한 줄만 보면 알 수 있고, 게임 시작 메시지부터 모든 UI 문자열이 한국어입니다.

| 화면 | 한국어 | 영문 |
|---|---|---|
| 시작 메시지 | `{0}층 {1} 진입` | `Entering {0}F {1}` |
| 보스 격파 | `{0}층 보스 격파! '>' 내려가기.` | `Boss defeated on F{0}! ...` |
| 사망 | `쓰러졌습니다… (R 새 게임)` | `You died…` |
| 저장 | `영혼 기억 저장: N 회 클리어` | (현재 한국어 한 줄) |

---

## 🎛️ 노브 슬라이더 (SPEC §20)

게임 중 `k` / `K` 키로 16개 슬라이더 모달을 열어 게임 룰을 미세 조정할 수 있습니다.

| 키 (모달 내) | 동작 |
|---|---|
| `j` / `↓` | 다음 슬라이더 |
| `k` / `↑` | 이전 슬라이더 |
| `h` / `←` | 값 -스텝 |
| `l` / `→` | 값 +스텝 |
| `r` | 현재 슬라이더 기본값으로 리셋 |
| `R` | 전체 슬라이더 기본값으로 리셋 |
| `Esc` / `q` / `k` / `K` | 모달 닫기 |

v0.6.1에서 **10개 슬라이더**가 실제 게임 룰에 반영됩니다 (나머지 6개는 UI 표시만, v0.6.2 이후 시스템 도입 시 연동 예정):

- **`vision.range` (시야)** — 플레이어 Viewshed::new의 range에 적용
- **`enemy.hp_mul` (적 HP)** — 적 Health::new 값에 곱
- **`enemy.atk_mul` (적 공격)** — 적 Stats.attack_bonus 및 strength에 곱
- **`monster.density` (적 밀도)** — 값 `0` 일 때 중간 등급 적(곰) 스폰 생략
- **`max_floors` (최대 층)** — descend_internal 임계값을 기본 8에서 사용자가 늘릴 수 있게
- **`hp.start` (시작 HP)** — 플레이어 시작 HP에 적용
- **`mp.start` (시작 MP)** — 플레이어 시작 MP에 적용
- **`enemy.speed_mul` (적 속도)** — 적 AI 속도에 적용
- **`scaling.floor` (층 스케일링)** — 층별 적 스케일에 적용
- **`gold.density` (골드 밀도)** — 골드 드롭량에 적용

### 노브 카테고리 (v0.6.2+)

모달 안 16 개 슬라이더 각각에 카테고리 배지가 붙어 어느 영역에 영향을 주는지 한눈에 보입니다:

- **`[P]` (Green)** — 플레이어 (시작 HP/MP, 시야): `hp.start`, `mp.start`, `vision.range`
- **`[E]` (Red)** — 적 (속도/HP/공격/밀도): `enemy.speed_mul`, `enemy.hp_mul`, `enemy.atk_mul`, `monster.density`
- **`[W]` (Blue)** — 월드 (층 스케일/최대 층/드롭): `max_floors`, `scaling.floor`, `gold.density`
- **`[A]` (Magenta)** — 자동 (§19 Auto-Pilot, v0.6.2 기준 UI 만): `autopilot.mode`, `autopilot.speed`

나머지 6개 (`food.start`, `autopilot.mode`, `autopilot.speed`, `pacing.recovery`, `relic.density`, `trap.density`)는 슬라이더가 표시되고 조작도 가능하지만 v0.6.1에서는 게임에 반영되지 않으며, v0.6.2 이후 시스템 도입 시 연동 예정입니다.

설정은 `SoulRemembrance.knobs` (RON, `#[serde(default)]`)에 자동 저장되어 다음 런에도 유지됩니다. 자세한 내용은 [`docs/SPEC.md`](docs/SPEC.md) §20 참고.

---

## 🗺️ 5주 로드맵

| 주차 | 목표 | 검증 |
|---|---|---|
| 1 | BSP 던전 + half-block wall_glyph ✅ | `cargo run --release -- --dump --count 3` — 12룸 60×24 |
| 2 | **ECS + 키바인딩(vim/yubn) + FOV ✅** | `--dump`에서 `player@(...)` + `visible N` 헤더로 시야 확인 가능 |
| 3 | **적 AI + 전투 + HP/MP ✅** | 매턴 적 행동 결정(BFS/Chase/Coward), 명중·데미지·치명타 |
| 4 | **아이템 + 인벤 + 8층 변주 + 보스 ✅** | `>`로 보스 처치 후 내려가기 가능, 골드/물약/무기 순환 드롭 |
| 5 | **메타 진행 (§18) + i18n + 다듬기 ✅** | `S` 저장(ron), `L` ko/en 토글, `?` 인게임 도움말 |

자세한 명세는 [`docs/SPEC.md`](docs/SPEC.md) 참고 — 20개 섹션, 클래식한 TUI 로그라이크 디자인 + **Auto-Pilot** (§19) + **Knob 슬라이더** (§20).

> §19 Auto-Pilot: 게임이 알아서 진행, 플레이어는 필요한 순간만 개입 (§17 `Bolrun` 회차 누적 적 강화 + §18 `Soul Remembrance` 영혼 누적 보너스 + 결합).
>
> §20 Knob: 매 런 시작 시 15개 슬라이더로 게임 룰 미세 조정 — 사용자가 자기만의 게임 루프를 만듦.

---

## 📚 레퍼런스

핵심 학습 대상으로 본 프로젝트가 차용한 코드:

- [`thebracket/rustrogueliketutorial`](https://github.com/thebracket/rustrogueliketutorial) ⭐ 967 — chapter 16 `wall_glyph()` 4-bit 마스크 알고리즘
- [`kako-jun/nobiscuit`](https://github.com/kako-jun/nobiscuit) — TUI raycasting maze + half-block + 24bit RGB
- [`maria-rcks/mythos-adventure`](https://github.com/maria-rcks/mythos-adventure) — ratatui Widget half-block blit 패턴
- [`doryen-fov`](https://github.com/jice-nospam/doryen-fov) — FOV 알고리즘 (v2 사용 예정)

---

## 📄 라이선스

MIT — [`LICENSE`](LICENSE) 참고.
