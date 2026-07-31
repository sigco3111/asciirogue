/// 플레이어가 보유한 식량의 현재량과 시작 최대량입니다.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Food {
    pub current: i32,
    pub max: i32,
}

impl Food {
    pub const DEFAULT_MAX: i32 = 8;

    /// 음수 입력을 0으로 보정해 현재량과 최대량을 함께 초기화합니다.
    pub const fn new(value: i32) -> Self {
        let value = if value < 0 { 0 } else { value };
        Self {
            current: value,
            max: value,
        }
    }

    /// 현재 식량만 0 아래로 내려가지 않게 소모합니다.
    pub fn consume(&mut self, by: i32) {
        self.current = self.current.saturating_sub(by).max(0);
    }

    /// 현재 식량이 바닥났는지 반환합니다.
    pub const fn is_starving(&self) -> bool {
        self.current <= 0
    }
}
