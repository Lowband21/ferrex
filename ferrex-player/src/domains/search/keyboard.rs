//! Minimal 10-foot on-screen keyboard model for remote text entry.
//!
//! The model is intentionally UI-agnostic: it tracks which key is focused,
//! exposes a stable key layout for rendering, and maps key activation to a
//! small set of search actions. The UI/update layer decides how to dispatch
//! those actions into the existing search query/update path.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenFootKeyboardDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenFootKeyboardKey {
    Character(char),
    Space,
    Backspace,
    Clear,
    Search,
    Done,
}

impl TenFootKeyboardKey {
    pub fn label(self) -> String {
        match self {
            Self::Character(ch) => ch.to_string(),
            Self::Space => "Space".to_string(),
            Self::Backspace => "⌫ Back".to_string(),
            Self::Clear => "Clear".to_string(),
            Self::Search => "Search".to_string(),
            Self::Done => "Done".to_string(),
        }
    }

    pub fn is_action(self) -> bool {
        !matches!(self, Self::Character(_) | Self::Space)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenFootKeyboardAction {
    UpdateQuery(String),
    ExecuteSearch,
    CloseKeyboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TenFootKeyboardFocus {
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenFootKeyboardState {
    is_open: bool,
    focus: TenFootKeyboardFocus,
}

pub const TENFOOT_KEYBOARD_ROWS: &[&[TenFootKeyboardKey]] = &[
    &[
        TenFootKeyboardKey::Character('A'),
        TenFootKeyboardKey::Character('B'),
        TenFootKeyboardKey::Character('C'),
        TenFootKeyboardKey::Character('D'),
        TenFootKeyboardKey::Character('E'),
        TenFootKeyboardKey::Character('F'),
        TenFootKeyboardKey::Character('G'),
        TenFootKeyboardKey::Character('H'),
        TenFootKeyboardKey::Character('I'),
        TenFootKeyboardKey::Character('J'),
    ],
    &[
        TenFootKeyboardKey::Character('K'),
        TenFootKeyboardKey::Character('L'),
        TenFootKeyboardKey::Character('M'),
        TenFootKeyboardKey::Character('N'),
        TenFootKeyboardKey::Character('O'),
        TenFootKeyboardKey::Character('P'),
        TenFootKeyboardKey::Character('Q'),
        TenFootKeyboardKey::Character('R'),
        TenFootKeyboardKey::Character('S'),
        TenFootKeyboardKey::Character('T'),
    ],
    &[
        TenFootKeyboardKey::Character('U'),
        TenFootKeyboardKey::Character('V'),
        TenFootKeyboardKey::Character('W'),
        TenFootKeyboardKey::Character('X'),
        TenFootKeyboardKey::Character('Y'),
        TenFootKeyboardKey::Character('Z'),
    ],
    &[
        TenFootKeyboardKey::Character('0'),
        TenFootKeyboardKey::Character('1'),
        TenFootKeyboardKey::Character('2'),
        TenFootKeyboardKey::Character('3'),
        TenFootKeyboardKey::Character('4'),
        TenFootKeyboardKey::Character('5'),
        TenFootKeyboardKey::Character('6'),
        TenFootKeyboardKey::Character('7'),
        TenFootKeyboardKey::Character('8'),
        TenFootKeyboardKey::Character('9'),
    ],
    &[
        TenFootKeyboardKey::Space,
        TenFootKeyboardKey::Backspace,
        TenFootKeyboardKey::Clear,
        TenFootKeyboardKey::Search,
        TenFootKeyboardKey::Done,
    ],
];

impl Default for TenFootKeyboardState {
    fn default() -> Self {
        Self {
            is_open: false,
            focus: TenFootKeyboardFocus { row: 0, column: 0 },
        }
    }
}

impl TenFootKeyboardState {
    pub fn rows() -> &'static [&'static [TenFootKeyboardKey]] {
        TENFOOT_KEYBOARD_ROWS
    }

    pub fn open(&mut self) {
        self.is_open = true;
        self.clamp_focus();
    }

    pub fn close(&mut self) {
        self.is_open = false;
    }

    pub fn is_open(&self) -> bool {
        self.is_open
    }

    pub fn focus(&self) -> TenFootKeyboardFocus {
        self.focus
    }

    pub fn focused_key(&self) -> Option<TenFootKeyboardKey> {
        Self::key_at(self.focus)
    }

    pub fn is_focused(&self, key: TenFootKeyboardKey) -> bool {
        self.focused_key() == Some(key)
    }

    pub fn focus_key(&mut self, key: TenFootKeyboardKey) -> bool {
        for (row, keys) in Self::rows().iter().enumerate() {
            if let Some(column) =
                keys.iter().position(|candidate| *candidate == key)
            {
                self.focus = TenFootKeyboardFocus { row, column };
                return true;
            }
        }

        false
    }

    pub fn move_focus(&mut self, direction: TenFootKeyboardDirection) {
        self.clamp_focus();

        match direction {
            TenFootKeyboardDirection::Left => {
                let row_len = Self::row_len(self.focus.row);
                if row_len == 0 {
                    return;
                }
                self.focus.column = if self.focus.column == 0 {
                    row_len - 1
                } else {
                    self.focus.column - 1
                };
            }
            TenFootKeyboardDirection::Right => {
                let row_len = Self::row_len(self.focus.row);
                if row_len == 0 {
                    return;
                }
                self.focus.column = (self.focus.column + 1) % row_len;
            }
            TenFootKeyboardDirection::Up => {
                self.move_vertical(-1);
            }
            TenFootKeyboardDirection::Down => {
                self.move_vertical(1);
            }
        }
    }

    pub fn action_for_key(
        key: TenFootKeyboardKey,
        query: &str,
    ) -> TenFootKeyboardAction {
        match key {
            TenFootKeyboardKey::Character(ch) => {
                let mut next = query.to_string();
                next.push(ch.to_ascii_lowercase());
                TenFootKeyboardAction::UpdateQuery(next)
            }
            TenFootKeyboardKey::Space => {
                let mut next = query.to_string();
                next.push(' ');
                TenFootKeyboardAction::UpdateQuery(next)
            }
            TenFootKeyboardKey::Backspace => {
                let mut next = query.to_string();
                next.pop();
                TenFootKeyboardAction::UpdateQuery(next)
            }
            TenFootKeyboardKey::Clear => {
                TenFootKeyboardAction::UpdateQuery(String::new())
            }
            TenFootKeyboardKey::Search => TenFootKeyboardAction::ExecuteSearch,
            TenFootKeyboardKey::Done => TenFootKeyboardAction::CloseKeyboard,
        }
    }

    fn key_at(focus: TenFootKeyboardFocus) -> Option<TenFootKeyboardKey> {
        Self::rows()
            .get(focus.row)
            .and_then(|row| row.get(focus.column))
            .copied()
    }

    fn row_len(row: usize) -> usize {
        Self::rows().get(row).map(|keys| keys.len()).unwrap_or(0)
    }

    fn clamp_focus(&mut self) {
        if Self::rows().is_empty() {
            self.focus = TenFootKeyboardFocus { row: 0, column: 0 };
            return;
        }

        self.focus.row = self.focus.row.min(Self::rows().len() - 1);
        let row_len = Self::row_len(self.focus.row);
        self.focus.column = if row_len == 0 {
            0
        } else {
            self.focus.column.min(row_len - 1)
        };
    }

    fn move_vertical(&mut self, delta: isize) {
        let row_count = Self::rows().len();
        if row_count == 0 {
            return;
        }

        let next_row = if delta < 0 {
            if self.focus.row == 0 {
                row_count - 1
            } else {
                self.focus.row - 1
            }
        } else {
            (self.focus.row + 1) % row_count
        };

        self.focus.row = next_row;
        let row_len = Self::row_len(next_row);
        self.focus.column = if row_len == 0 {
            0
        } else {
            self.focus.column.min(row_len - 1)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_appends_characters_and_space() {
        assert_eq!(
            TenFootKeyboardState::action_for_key(
                TenFootKeyboardKey::Character('A'),
                ""
            ),
            TenFootKeyboardAction::UpdateQuery("a".to_string())
        );
        assert_eq!(
            TenFootKeyboardState::action_for_key(
                TenFootKeyboardKey::Character('Z'),
                "star"
            ),
            TenFootKeyboardAction::UpdateQuery("starz".to_string())
        );
        assert_eq!(
            TenFootKeyboardState::action_for_key(
                TenFootKeyboardKey::Character('7'),
                "se"
            ),
            TenFootKeyboardAction::UpdateQuery("se7".to_string())
        );
        assert_eq!(
            TenFootKeyboardState::action_for_key(
                TenFootKeyboardKey::Space,
                "star"
            ),
            TenFootKeyboardAction::UpdateQuery("star ".to_string())
        );
    }

    #[test]
    fn activation_backspaces_and_clears() {
        assert_eq!(
            TenFootKeyboardState::action_for_key(
                TenFootKeyboardKey::Backspace,
                "alien"
            ),
            TenFootKeyboardAction::UpdateQuery("alie".to_string())
        );
        assert_eq!(
            TenFootKeyboardState::action_for_key(
                TenFootKeyboardKey::Backspace,
                ""
            ),
            TenFootKeyboardAction::UpdateQuery(String::new())
        );
        assert_eq!(
            TenFootKeyboardState::action_for_key(
                TenFootKeyboardKey::Clear,
                "alien"
            ),
            TenFootKeyboardAction::UpdateQuery(String::new())
        );
    }

    #[test]
    fn focus_movement_wraps_within_and_across_rows() {
        let mut keyboard = TenFootKeyboardState::default();
        keyboard.open();

        assert_eq!(
            keyboard.focused_key(),
            Some(TenFootKeyboardKey::Character('A'))
        );

        keyboard.move_focus(TenFootKeyboardDirection::Left);
        assert_eq!(
            keyboard.focused_key(),
            Some(TenFootKeyboardKey::Character('J'))
        );

        keyboard.move_focus(TenFootKeyboardDirection::Right);
        assert_eq!(
            keyboard.focused_key(),
            Some(TenFootKeyboardKey::Character('A'))
        );

        keyboard.move_focus(TenFootKeyboardDirection::Up);
        assert_eq!(keyboard.focused_key(), Some(TenFootKeyboardKey::Space));

        keyboard.move_focus(TenFootKeyboardDirection::Down);
        assert_eq!(
            keyboard.focused_key(),
            Some(TenFootKeyboardKey::Character('A'))
        );
    }

    #[test]
    fn action_keys_map_to_done_and_search_actions() {
        assert_eq!(
            TenFootKeyboardState::action_for_key(
                TenFootKeyboardKey::Search,
                "alien"
            ),
            TenFootKeyboardAction::ExecuteSearch
        );
        assert_eq!(
            TenFootKeyboardState::action_for_key(
                TenFootKeyboardKey::Done,
                "alien"
            ),
            TenFootKeyboardAction::CloseKeyboard
        );
    }
}
