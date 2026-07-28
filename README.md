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
cargo run --example dump -p asciirogue-procgen   # 헤드리스 던전 텍스트 덤프
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

## 🎮 컨트롤 (현재 데모)

| 키 | 동작 |
|---|---|
| `q` / `Q` / `Esc` | 종료 |
| `r` / `R` | 새 시드로 던전 재생성 |
| `?` | 도움말 (예정) |

---

## 🗺️ 5주 로드맵

| 주차 | 목표 | 검증 |
|---|---|---|
| 1 | BSP 던전 + half-block wall_glyph ✅ | `cargo run --example dump` — 12룸 60×24 |
| 2 | ECS + 키바인딩 + FOV | 플레이어 이동 시 시야 토글 |
| 3 | 적 AI + 전투 + HP/MP | 적 처치 |
| 4 | 아이템/인벤 + 8층 + 보스 1 | 첫 보스 클리어 |
| 5 | 메타 진행 (§18) + i18n + 다듬기 | 외부 플레이 가능 |

자세한 명세는 [SPEC 섹션](docs/SPEC.md) 참고 (현재 사본은 작업 디렉토리 `asciirogue_spec.md`).

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
