use crossterm::event::{KeyCode, KeyEvent};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders};
use tui_textarea::{CursorMove, TextArea};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimMode {
    Normal,
    Insert,
}

pub enum FieldResult {
    None,
    Submit,
}

pub struct VimField {
    pub area: TextArea<'static>,
    pub mode: VimMode,
    pending_g: bool,
    pending_delete: bool,
}

impl VimField {
    pub fn new(value: impl Into<String>) -> Self {
        let mut field = Self {
            area: TextArea::from(vec![value.into()]),
            mode: VimMode::Insert,
            pending_g: false,
            pending_delete: false,
        };
        field.update_cursor_style();
        field
    }

    pub fn text(&self) -> String {
        self.area.lines().join("\n")
    }
    pub fn set_normal(&mut self) {
        self.mode = VimMode::Normal;
        self.pending_g = false;
        self.pending_delete = false;
        self.update_cursor_style();
    }
    pub fn set_insert(&mut self) {
        self.mode = VimMode::Insert;
        self.pending_g = false;
        self.pending_delete = false;
        self.update_cursor_style();
    }

    fn update_cursor_style(&mut self) {
        let style = match self.mode {
            VimMode::Normal => Style::default().add_modifier(Modifier::REVERSED),
            VimMode::Insert => Style::default().add_modifier(Modifier::UNDERLINED),
        };
        self.area.set_cursor_style(style);
        self.area.set_cursor_line_style(Style::default());
    }
    pub fn title(&self, label: impl Into<String>) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .title(format!("{} · {}", label.into(), self.mode))
    }

    pub fn handle(&mut self, key: KeyEvent) -> FieldResult {
        if self.mode == VimMode::Insert {
            if key.code == KeyCode::Esc {
                self.set_normal();
                return FieldResult::None;
            }
            if key.code == KeyCode::Enter {
                return FieldResult::Submit;
            }
            self.area
                .input(ratatui::crossterm::event::KeyEvent::from(key));
            return FieldResult::None;
        }
        if key.code == KeyCode::Esc {
            self.pending_g = false;
            self.pending_delete = false;
            return FieldResult::None;
        }
        if self.pending_delete {
            self.pending_delete = false;
            match key.code {
                KeyCode::Char('w') => {
                    self.area.delete_next_word();
                }
                KeyCode::Char('b') => {
                    self.area.delete_word();
                }
                KeyCode::Char('d') => {
                    let had_content = self
                        .area
                        .lines()
                        .get(self.area.cursor().0)
                        .is_some_and(|line| !line.is_empty());
                    self.area.move_cursor(CursorMove::Head);
                    self.area.delete_line_by_end();
                    if had_content {
                        self.area.delete_next_char();
                    }
                }
                _ => {}
            }
            return FieldResult::None;
        }
        if self.pending_g {
            self.pending_g = false;
            if key.code == KeyCode::Char('g') {
                self.area.move_cursor(CursorMove::Top);
                return FieldResult::None;
            }
        }
        match key.code {
            KeyCode::Char('g') => self.pending_g = true,
            KeyCode::Char('G') => self.area.move_cursor(CursorMove::Bottom),
            KeyCode::Char('h') => self.area.move_cursor(CursorMove::Back),
            KeyCode::Char('j') => self.area.move_cursor(CursorMove::Down),
            KeyCode::Char('k') => self.area.move_cursor(CursorMove::Up),
            KeyCode::Char('l') => self.area.move_cursor(CursorMove::Forward),
            KeyCode::Char('0') => self.area.move_cursor(CursorMove::Head),
            KeyCode::Char('$') => self.area.move_cursor(CursorMove::End),
            KeyCode::Char('w') => self.area.move_cursor(CursorMove::WordForward),
            KeyCode::Char('b') => self.area.move_cursor(CursorMove::WordBack),
            KeyCode::Char('x') => {
                self.area.delete_next_char();
            }
            KeyCode::Char('D') => {
                self.area.delete_line_by_end();
            }
            KeyCode::Char('d') => self.pending_delete = true,
            KeyCode::Char('u') => {
                self.area.undo();
            }
            KeyCode::Char('i') => self.set_insert(),
            KeyCode::Char('a') => {
                self.area.move_cursor(CursorMove::Forward);
                self.set_insert();
            }
            KeyCode::Char('A') => {
                self.area.move_cursor(CursorMove::End);
                self.set_insert();
            }
            KeyCode::Char('I') => {
                self.area.move_cursor(CursorMove::Head);
                self.set_insert();
            }
            KeyCode::Char('o') => {
                self.area.move_cursor(CursorMove::End);
                self.area.insert_newline();
                self.set_insert();
            }
            KeyCode::Char('O') => {
                self.area.move_cursor(CursorMove::Head);
                self.area.insert_newline();
                self.area.move_cursor(CursorMove::Up);
                self.set_insert();
            }
            KeyCode::Enter => return FieldResult::Submit,
            _ => {}
        }
        FieldResult::None
    }
}

impl std::fmt::Display for VimMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
        })
    }
}
