//! Tiny built-in i18n: Korean and English strings packed into a single lookup
//! table. v0.5 keeps strings inside the binary (no on-disk .toml loading).
//! Future versions (v1.1+) may read assets/i18n/{ko,en}.toml at startup.

#![deny(rust_2018_idioms)]

/// Supported locales.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Locale {
    Korean,
    English,
}

impl Locale {
    pub fn from_code(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "en" | "english" => Self::English,
            _ => Self::Korean, // default for unknown; never silently degrade
        }
    }
}

/// String keys used throughout the game. Each maps to a (ko, en) pair below.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Key {
    UiGameTitle,
    UiPickUp,
    UiUsePotion,
    UiDescend,
    UiFloor,
    MsgFloorEntered,
    MsgBossDown,
    MsgNoItem,
    MsgNoPotion,
    MsgPickupOk,
    MsgPotionHp,
    MsgPotionMp,
    MsgAlreadyAtBottom,
    MsgBossFirst,
    MsgMenuNewGame,
    MsgMenuLoad,
    MsgMenuHelp,
    MsgStairs,
    MsgDeath,
}

const TABLE: &[(Key, &str, &str)] = &[
    (Key::UiGameTitle, "asciirogue — 한국어 우선 로그라이크", "asciirogue — Korean-first roguelike"),
    (Key::UiPickUp, "줍기", "Pick up"),
    (Key::UiUsePotion, "포션 사용", "Use potion"),
    (Key::UiDescend, "내려가기", "Descend"),
    (Key::UiFloor, "층", "F"),
    (Key::MsgFloorEntered, "{}층 {} 진입", "Entering {}F {}"),
    (Key::MsgBossDown, "{}층 보스 격파! '>' 내려가기.", "Boss defeated on F{}! Press \'>\' to descend."),
    (Key::MsgNoItem, "주울 아이템이 없습니다.", "Nothing to pick up."),
    (Key::MsgNoPotion, "사용할 물약이 없습니다.", "No potions available."),
    (Key::MsgPickupOk, "아이템을 주웠습니다.", "Items picked up."),
    (Key::MsgPotionHp, "치유 물약 사용! HP +{}", "Healing potion! +{} HP"),
    (Key::MsgPotionMp, "마나 물약 사용! MP +{}", "Mana potion! +{} MP"),
    (Key::MsgAlreadyAtBottom, "이미 던전 끝에 도달했습니다.", "Already at the bottom of the dungeon."),
    (Key::MsgBossFirst, "보스를 먼저 처치하세요.", "Defeat the boss first."),
    (Key::MsgMenuNewGame, "새 게임", "New game"),
    (Key::MsgMenuLoad, "이어하기", "Continue"),
    (Key::MsgMenuHelp, "도움말", "Help"),
    (Key::MsgStairs, "주위에 계단이 없습니다.", "There are no stairs here."),
    (Key::MsgDeath, "쓰러졌습니다…", "You died…"),
];

/// Translate a known key. Falls back to `"???" + key` when missing so we can
/// spot gaps in development.
pub fn t_for(key: Key, locale: Locale) -> &'static str {
    for (k, ko, en) in TABLE {
        if *k == key {
            return match locale {
                Locale::Korean => ko,
                Locale::English => en,
            };
        }
    }
    "???"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_code_round_trip() {
        assert_eq!(Locale::from_code("ko"), Locale::Korean);
        assert_eq!(Locale::from_code("en"), Locale::English);
        assert_eq!(Locale::from_code("xyz"), Locale::Korean, "unknown falls back to ko");
    }

    #[test]
    fn strings_pair_exist() {
        for (_k, ko, en) in TABLE {
            assert!(!ko.is_empty(), "ko missing");
            assert!(!en.is_empty(), "en missing");
        }
    }

    #[test]
    fn translate_returns_non_empty() {
        assert_eq!(t_for(Key::MsgDeath, Locale::Korean), "쓰러졌습니다…");
        assert_eq!(t_for(Key::MsgDeath, Locale::English), "You died…");
    }
}
