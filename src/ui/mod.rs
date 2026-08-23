mod render;

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, layout::Rect, widgets::ListState, Terminal};
use std::{io, time::Duration};

use crate::{
    model::{Conversation, Engine, Mode},
    storage::{
        create_conversation, load_conversations, rename_conversation, trash_conversation,
        update_engine, update_icon, update_pin,
    },
    vim::{FieldResult, VimField, VimMode},
};

/// Rendered height in rows of one conversation "slot" in the search list:
/// its own 2-line ListItem plus a separate 1-line spacer ListItem after it.
/// Used to hit-test mouse clicks. Keep in sync with ui/render.rs.
const LIST_ITEM_HEIGHT: u16 = 3;
/// Number of rows in the Space-actions modal (keep in sync with the labels
/// array in ui/render.rs).
const ACTION_COUNT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NewFocus {
    Engine,
    Title,
    Icon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Rename,
    Icon,
}

enum Modal {
    Actions { selected: usize },
    Edit { kind: EditKind, field: VimField },
    ConfirmTrash,
}

enum AppAction {
    None,
    Quit,
    Open(Conversation),
}

pub(crate) struct App {
    page: Mode,
    search: VimField,
    title: VimField,
    icon: VimField,
    search_focused: bool,
    new_focus: NewFocus,
    engine: Engine,
    conversations: Vec<Conversation>,
    selected: usize,
    list_pending_g: bool,
    modal: Option<Modal>,
    status: String,
    search_rect: Rect,
    list_rect: Rect,
    new_engine_rect: Rect,
    new_title_rect: Rect,
    new_icon_rect: Rect,
    modal_rect: Rect,
    list_state: ListState,
}

impl App {
    fn new(page: Mode, conversations: Vec<Conversation>) -> Self {
        let search_focused = matches!(&page, Mode::Search);
        let mut search = VimField::new("");
        search.set_normal();
        Self {
            page,
            search,
            title: VimField::new(""),
            icon: VimField::new(""),
            search_focused,
            new_focus: NewFocus::Engine,
            engine: Engine::Codex,
            conversations,
            selected: 0,
            list_pending_g: false,
            modal: None,
            status: String::new(),
            search_rect: Rect::default(),
            list_rect: Rect::default(),
            new_engine_rect: Rect::default(),
            new_title_rect: Rect::default(),
            new_icon_rect: Rect::default(),
            modal_rect: Rect::default(),
            list_state: ListState::default(),
        }
    }

    fn visible(&self) -> Vec<Conversation> {
        let query = self.search.text().to_lowercase();
        self.conversations
            .iter()
            .filter(|item| query.is_empty() || item.title.to_lowercase().contains(&query))
            .cloned()
            .collect()
    }

    fn selected_conversation(&self) -> Option<Conversation> {
        self.visible().get(self.selected).cloned()
    }

    fn clamp_selection(&mut self) {
        self.selected = self.selected.min(self.visible().len().saturating_sub(1));
    }

    fn reload(&mut self) {
        match load_conversations() {
            Ok(conversations) => {
                self.conversations = conversations;
                self.clamp_selection();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<AppAction> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
            return Ok(AppAction::Quit);
        }
        if self.modal.is_some() {
            return self.handle_modal_key(key);
        }
        match &self.page {
            Mode::Search => self.handle_search_key(key),
            Mode::New => self.handle_new_key(key),
            Mode::Chat(_) => Ok(AppAction::Quit),
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) -> Result<AppAction> {
        if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
            if self.search_focused {
                self.search_focused = false;
                self.search.set_normal();
            } else {
                self.search_focused = true;
                self.search.set_insert();
            }
            return Ok(AppAction::None);
        }
        if key.code == KeyCode::Char('/') && self.search.mode == VimMode::Normal {
            self.search_focused = true;
            self.search.set_insert();
            return Ok(AppAction::None);
        }
        if self.search_focused {
            if self.search.mode == VimMode::Insert {
                if matches!(self.search.handle(key), FieldResult::Submit) {
                    self.search.set_normal();
                }
                self.clamp_selection();
                return Ok(AppAction::None);
            }
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    self.selected = self
                        .selected
                        .saturating_add(1)
                        .min(self.visible().len().saturating_sub(1))
                }
                KeyCode::Char('k') | KeyCode::Up => self.selected = self.selected.saturating_sub(1),
                KeyCode::Char('g') => {
                    if self.list_pending_g {
                        self.selected = 0;
                        self.list_pending_g = false;
                    } else {
                        self.list_pending_g = true;
                    }
                }
                KeyCode::Char('G') | KeyCode::End => {
                    self.selected = self.visible().len().saturating_sub(1);
                    self.list_pending_g = false;
                }
                KeyCode::Char(' ') if !self.visible().is_empty() => {
                    self.modal = Some(Modal::Actions { selected: 0 })
                }
                KeyCode::Char('n') => {
                    self.page = Mode::New;
                    self.search_focused = false;
                    self.new_focus = NewFocus::Engine;
                }
                KeyCode::Enter => {
                    return Ok(self
                        .selected_conversation()
                        .map_or(AppAction::None, AppAction::Open))
                }
                KeyCode::Esc => self.search_focused = false,
                _ => {
                    let _ = self.search.handle(key);
                }
            }
        } else {
            match key.code {
                KeyCode::Char('/') => {
                    self.search_focused = true;
                    self.search.set_insert();
                }
                KeyCode::Char('n') => {
                    self.page = Mode::New;
                    self.new_focus = NewFocus::Engine;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    self.selected = self
                        .selected
                        .saturating_add(1)
                        .min(self.visible().len().saturating_sub(1))
                }
                KeyCode::Char('k') | KeyCode::Up => self.selected = self.selected.saturating_sub(1),
                KeyCode::Enter => {
                    return Ok(self
                        .selected_conversation()
                        .map_or(AppAction::None, AppAction::Open))
                }
                KeyCode::Char(' ') if !self.visible().is_empty() => {
                    self.modal = Some(Modal::Actions { selected: 0 })
                }
                KeyCode::Char('q') | KeyCode::Esc => {}
                _ => {}
            }
        }
        Ok(AppAction::None)
    }

    fn handle_new_key(&mut self, key: KeyEvent) -> Result<AppAction> {
        let can_search = match self.new_focus {
            NewFocus::Engine => true,
            NewFocus::Title => self.title.mode == VimMode::Normal,
            NewFocus::Icon => self.icon.mode == VimMode::Normal,
        };
        if key.code == KeyCode::Char('s') && can_search {
            self.page = Mode::Search;
            self.search_focused = true;
            self.search.set_normal();
            return Ok(AppAction::None);
        }
        if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
            // Terminals report Shift+Tab as its own BackTab code, not Tab
            // with a Shift modifier — check both so backwards navigation
            // actually works.
            let backwards = key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT);
            self.new_focus = match (self.new_focus, backwards) {
                (NewFocus::Engine, false) => NewFocus::Title,
                (NewFocus::Title, false) => NewFocus::Icon,
                (NewFocus::Icon, false) => NewFocus::Engine,
                (NewFocus::Engine, true) => NewFocus::Icon,
                (NewFocus::Title, true) => NewFocus::Engine,
                (NewFocus::Icon, true) => NewFocus::Title,
            };
            match self.new_focus {
                NewFocus::Engine => {}
                NewFocus::Title => self.title.set_insert(),
                NewFocus::Icon => self.icon.set_insert(),
            }
            return Ok(AppAction::None);
        }
        if key.code == KeyCode::Char('/') && self.new_focus == NewFocus::Engine {
            self.page = Mode::Search;
            self.search_focused = true;
            self.search.set_normal();
            return Ok(AppAction::None);
        }
        if self.new_focus == NewFocus::Engine {
            match key.code {
                KeyCode::Char('j') | KeyCode::Char('l') | KeyCode::Down | KeyCode::Right => {
                    self.engine = Engine::Codex
                }
                KeyCode::Char('k') | KeyCode::Char('h') | KeyCode::Up | KeyCode::Left => {
                    self.engine = Engine::Claude
                }
                KeyCode::Char('c') => self.engine = Engine::Claude,
                KeyCode::Char('d') => self.engine = Engine::Codex,
                KeyCode::Tab | KeyCode::Char(' ') => {
                    self.engine = match self.engine {
                        Engine::Claude => Engine::Codex,
                        Engine::Codex => Engine::Claude,
                    }
                }
                KeyCode::Enter => {
                    self.new_focus = NewFocus::Title;
                    self.title.set_insert();
                }
                KeyCode::Esc | KeyCode::Char('q') => {}
                _ => {}
            }
            return Ok(AppAction::None);
        }
        if self.new_focus == NewFocus::Title {
            if key.code == KeyCode::Char('/') && self.title.mode == VimMode::Normal {
                self.page = Mode::Search;
                self.search_focused = true;
                self.search.set_normal();
                return Ok(AppAction::None);
            }
            if key.code == KeyCode::Esc && self.title.mode == VimMode::Normal {
                return Ok(AppAction::None);
            }
            if matches!(self.title.handle(key), FieldResult::Submit) {
                self.new_focus = NewFocus::Icon;
                self.icon.set_insert();
            }
            return Ok(AppAction::None);
        }
        if key.code == KeyCode::Char('/') && self.icon.mode == VimMode::Normal {
            self.page = Mode::Search;
            self.search_focused = true;
            self.search.set_normal();
            return Ok(AppAction::None);
        }
        if key.code == KeyCode::Esc && self.icon.mode == VimMode::Normal {
            return Ok(AppAction::None);
        }
        if matches!(self.icon.handle(key), FieldResult::Submit) {
            let mut conversation = create_conversation(self.engine, &self.title.text())?;
            let icon = self.icon.text();
            update_icon(&conversation, &icon)?;
            conversation.icon = if icon.trim().is_empty() {
                "⬜".into()
            } else {
                icon.trim().chars().take(2).collect()
            };
            return Ok(AppAction::Open(conversation));
        }
        Ok(AppAction::None)
    }

    fn handle_modal_key(&mut self, key: KeyEvent) -> Result<AppAction> {
        let Some(modal) = self.modal.take() else {
            return Ok(AppAction::None);
        };
        match modal {
            Modal::Actions { mut selected } => match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    selected = (selected + 1).min(ACTION_COUNT - 1);
                    self.modal = Some(Modal::Actions { selected });
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                    self.modal = Some(Modal::Actions { selected });
                }
                KeyCode::Char('g') => {
                    selected = 0;
                    self.modal = Some(Modal::Actions { selected });
                }
                KeyCode::Char('G') => {
                    selected = ACTION_COUNT - 1;
                    self.modal = Some(Modal::Actions { selected });
                }
                KeyCode::Esc | KeyCode::Char('q') => return Ok(AppAction::None),
                KeyCode::Enter => {
                    if selected == ACTION_COUNT - 1 {
                        return Ok(AppAction::None);
                    }
                    if let Some(conversation) = self.selected_conversation() {
                        match selected {
                            0 => {
                                self.modal = Some(Modal::Edit {
                                    kind: EditKind::Rename,
                                    field: VimField::new(conversation.title),
                                })
                            }
                            1 => {
                                self.modal = Some(Modal::Edit {
                                    kind: EditKind::Icon,
                                    field: VimField::new(conversation.icon),
                                })
                            }
                            2 => {
                                let next = match conversation.engine {
                                    Engine::Claude => Engine::Codex,
                                    Engine::Codex => Engine::Claude,
                                };
                                update_engine(&conversation, next)?;
                                self.status = format!("Coding agent set to {}", next.label());
                                self.reload();
                            }
                            3 => {
                                update_pin(&conversation, !conversation.pinned)?;
                                self.status = "Conversation pin updated".into();
                                self.reload();
                            }
                            4 => self.modal = Some(Modal::ConfirmTrash),
                            _ => {}
                        }
                    }
                }
                _ => self.modal = Some(Modal::Actions { selected }),
            },
            Modal::Edit { kind, mut field } => {
                if key.code == KeyCode::Esc && field.mode == VimMode::Normal {
                    return Ok(AppAction::None);
                }
                if matches!(field.handle(key), FieldResult::Submit) {
                    if let Some(conversation) = self.selected_conversation() {
                        match kind {
                            EditKind::Rename => rename_conversation(&conversation, &field.text())?,
                            EditKind::Icon => update_icon(&conversation, &field.text())?,
                        }
                        self.status = "Conversation updated".into();
                        self.reload();
                    }
                } else {
                    self.modal = Some(Modal::Edit { kind, field });
                }
            }
            Modal::ConfirmTrash => match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    if let Some(conversation) = self.selected_conversation() {
                        trash_conversation(&conversation)?;
                        self.status = "Conversation moved to trash".into();
                        self.reload();
                    }
                }
                KeyCode::Char('n') | KeyCode::Esc | KeyCode::Char('q') => {}
                _ => self.modal = Some(Modal::ConfirmTrash),
            },
        }
        Ok(AppAction::None)
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<AppAction> {
        if self.modal.is_some() {
            return self.handle_modal_mouse(mouse);
        }
        match &self.page {
            Mode::Search => self.handle_search_mouse(mouse),
            Mode::New => self.handle_new_mouse(mouse),
            Mode::Chat(_) => Ok(AppAction::None),
        }
    }

    fn handle_search_mouse(&mut self, mouse: MouseEvent) -> Result<AppAction> {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if point_in(self.search_rect, mouse.column, mouse.row) {
                    self.search_focused = true;
                    self.search.set_insert();
                } else if point_in(self.list_rect, mouse.column, mouse.row) {
                    if let Some(index) = self.list_item_at(mouse.row) {
                        let reopen = index == self.selected;
                        self.selected = index;
                        self.search_focused = false;
                        self.search.set_normal();
                        if reopen {
                            return Ok(self
                                .selected_conversation()
                                .map_or(AppAction::None, AppAction::Open));
                        }
                    }
                }
            }
            MouseEventKind::ScrollDown if point_in(self.list_rect, mouse.column, mouse.row) => {
                self.selected = self
                    .selected
                    .saturating_add(1)
                    .min(self.visible().len().saturating_sub(1));
            }
            MouseEventKind::ScrollUp if point_in(self.list_rect, mouse.column, mouse.row) => {
                self.selected = self.selected.saturating_sub(1);
            }
            _ => {}
        }
        Ok(AppAction::None)
    }

    fn handle_new_mouse(&mut self, mouse: MouseEvent) -> Result<AppAction> {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return Ok(AppAction::None);
        }
        if point_in(self.new_engine_rect, mouse.column, mouse.row) {
            let midpoint = self.new_engine_rect.x + self.new_engine_rect.width / 2;
            self.engine = if mouse.column < midpoint {
                Engine::Claude
            } else {
                Engine::Codex
            };
            self.new_focus = NewFocus::Engine;
        } else if point_in(self.new_title_rect, mouse.column, mouse.row) {
            self.new_focus = NewFocus::Title;
            self.title.set_insert();
        } else if point_in(self.new_icon_rect, mouse.column, mouse.row) {
            self.new_focus = NewFocus::Icon;
            self.icon.set_insert();
        }
        Ok(AppAction::None)
    }

    fn handle_modal_mouse(&mut self, mouse: MouseEvent) -> Result<AppAction> {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return Ok(AppAction::None);
        }
        if !point_in(self.modal_rect, mouse.column, mouse.row)
            || !matches!(self.modal, Some(Modal::Actions { .. }))
        {
            return Ok(AppAction::None);
        }
        let index = mouse.row.saturating_sub(self.modal_rect.y + 1) as usize;
        if index >= ACTION_COUNT {
            return Ok(AppAction::None);
        }
        self.modal = Some(Modal::Actions { selected: index });
        self.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    }

    fn list_item_at(&self, row: u16) -> Option<usize> {
        let inner_top = self.list_rect.y + 1;
        if row < inner_top {
            return None;
        }
        let offset_row = row - inner_top;
        // Each conversation renders as its own ListItem followed by a
        // separate 1-line spacer ListItem, so `list_state.offset()` is an
        // index into that interleaved list — halve it to get a conversation
        // index — and a click landing on the spacer row (the last row of
        // each 3-row slot) doesn't select anything.
        if offset_row % LIST_ITEM_HEIGHT >= LIST_ITEM_HEIGHT - 1 {
            return None;
        }
        let index = self.list_state.offset() / 2 + (offset_row / LIST_ITEM_HEIGHT) as usize;
        (index < self.visible().len()).then_some(index)
    }
}

fn point_in(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
}

pub fn run_tui(page: Mode, conversations: Vec<Conversation>) -> Result<Option<Conversation>> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(page, conversations);
    let result = loop {
        terminal.draw(|frame| render::draw(frame, &mut app))?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        // A failed storage operation (bad rename, trash error, ...) must not
        // tear down the TUI via `?` here — that would skip the terminal
        // restoration below and leave the real terminal stuck in raw/
        // alternate-screen mode. Surface it in the status line instead.
        let handled = match event::read()? {
            Event::Key(key) => app.handle_key(key),
            Event::Mouse(mouse) => app.handle_mouse(mouse),
            _ => Ok(AppAction::None),
        };
        let action = match handled {
            Ok(action) => action,
            Err(error) => {
                app.status = error.to_string();
                AppAction::None
            }
        };
        match action {
            AppAction::None => {}
            AppAction::Quit => break Ok(None),
            AppAction::Open(conversation) => break Ok(Some(conversation)),
        }
    };
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    io::Write::flush(&mut io::stdout())?;
    if result.as_ref().is_ok_and(Option::is_some) {
        // Give the terminal a moment to fully process leaving the
        // alternate screen / disabling mouse capture before zellij floods
        // it with its own screen setup right behind us — handing off two
        // full-screen terminal apps back-to-back with zero gap has been
        // observed to crash ghostty's IO thread.
        std::thread::sleep(Duration::from_millis(60));
    }
    result
}
