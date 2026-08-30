mod render;

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle,
    },
};
use ratatui::{backend::CrosstermBackend, layout::Rect, widgets::ListState, Terminal};
use std::{fs, io, path::PathBuf, time::Duration};

use crate::{
    config::{self, Config},
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
    Template,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetupFocus {
    Agents,
    Dir,
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
    ConfirmHyprlandRule,
}

enum AppAction {
    None,
    Quit,
    Open(Conversation),
}

pub(crate) struct App {
    config: Config,
    page: Mode,
    /// Page to return to when leaving Setup (via `s`, Esc, or after a
    /// successful save) without quitting the app entirely.
    return_to: Mode,
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
    new_template_rect: Rect,
    modal_rect: Rect,
    list_state: ListState,
    // Setup page state. `setup_agents`/`setup_enabled` are parallel arrays;
    // position in `setup_agents` is the agent's launch-priority rank.
    setup_agents: Vec<Engine>,
    setup_enabled: Vec<bool>,
    setup_cursor: usize,
    setup_focus: SetupFocus,
    setup_dir: VimField,
    // New-conversation page's template picker. `templates` is `default`
    // first, then the rest alphabetically; `template_index` is an index
    // into `templates` followed by one synthetic "(none)" slot at the end
    // (`templates.len()` itself), so `templates.len()` always means "(none)".
    templates: Vec<String>,
    template_index: usize,
    /// Set whenever there's 0 or 1 real templates — not an error, just a
    /// nudge shown inline (`Some("Only 'default' — add more at <path>")` or
    /// similar). Missing/unreadable templates get a separate desktop
    /// notification + log warning from `templates::list`, not this.
    template_hint: Option<String>,
}

impl App {
    fn new(page: Mode, conversations: Vec<Conversation>, config: Config) -> Self {
        // An empty `enabled_agents` only ever happens on the placeholder
        // config main.rs builds when config.toml doesn't exist yet — a real
        // saved config always has at least one agent.
        let needs_setup = config.enabled_agents.is_empty();
        let return_to = if matches!(page, Mode::Setup) {
            Mode::Search
        } else {
            page.clone()
        };
        let initial_page = if needs_setup { Mode::Setup } else { page };
        let search_focused = matches!(&initial_page, Mode::Search);
        let mut search = VimField::new("");
        search.set_normal();
        let engine = config
            .enabled_agents
            .first()
            .copied()
            .unwrap_or(Engine::Claude);
        let (setup_agents, setup_enabled) = setup_state_from(&config);
        let setup_dir = VimField::new(config.chats_dir.display().to_string());
        let mut app = Self {
            config,
            page: initial_page,
            return_to,
            search,
            title: VimField::new(""),
            icon: VimField::new(""),
            search_focused,
            new_focus: NewFocus::Engine,
            engine,
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
            new_template_rect: Rect::default(),
            modal_rect: Rect::default(),
            list_state: ListState::default(),
            setup_agents,
            setup_enabled,
            setup_cursor: 0,
            setup_focus: SetupFocus::Agents,
            setup_dir,
            templates: Vec::new(),
            template_index: 0,
            template_hint: None,
        };
        // Only scan the templates directory (and possibly seed/warn) when
        // actually landing on the New page — no need for that I/O just to
        // open Search.
        if matches!(app.page, Mode::New) {
            app.refresh_templates();
        }
        app
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
        match load_conversations(&self.config) {
            Ok(conversations) => {
                self.conversations = conversations;
                self.clamp_selection();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// (Re)initializes the setup checklist/dir field from the current
    /// config and switches to the Setup page. Called both when jumping in
    /// mid-session (`c`) and — indirectly, via `App::new` — on first run.
    fn enter_setup(&mut self) {
        let (agents, enabled) = setup_state_from(&self.config);
        self.setup_agents = agents;
        self.setup_enabled = enabled;
        self.setup_cursor = 0;
        self.setup_focus = SetupFocus::Agents;
        self.setup_dir = VimField::new(self.config.chats_dir.display().to_string());
        self.page = Mode::Setup;
    }

    /// Switches to the New page and (re)scans the templates directory, so
    /// a template added since the app started shows up immediately.
    fn enter_new(&mut self) {
        self.page = Mode::New;
        self.new_focus = NewFocus::Engine;
        self.refresh_templates();
    }

    fn refresh_templates(&mut self) {
        self.templates = crate::templates::list();
        self.template_index = 0;
        self.template_hint = if self.templates.len() <= 1 {
            let root = crate::templates::root_display();
            Some(match self.templates.first() {
                Some(name) => format!("Only '{name}' — add more at {root}"),
                None => format!("No templates yet — add one at {root}"),
            })
        } else {
            None
        };
    }

    /// Toggles the agent at the cursor and repositions it so enabled agents
    /// always occupy the top of the list, in the order they were enabled:
    /// turning one on moves it to the bottom of the enabled block (so it
    /// picks up the next rank); turning one off moves it to the top of the
    /// disabled block, right below the last remaining enabled agent. The
    /// cursor follows the agent so repeated toggling doesn't lose your
    /// place. `J`/`K` can still reorder freely on top of this.
    fn setup_toggle(&mut self) {
        let index = self.setup_cursor;
        if index >= self.setup_agents.len() {
            return;
        }
        let engine = self.setup_agents.remove(index);
        let now_enabled = !self.setup_enabled.remove(index);
        let insert_at = self.setup_enabled.iter().filter(|&&enabled| enabled).count();
        self.setup_agents.insert(insert_at, engine);
        self.setup_enabled.insert(insert_at, now_enabled);
        self.setup_cursor = insert_at;
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
            Mode::Setup => self.handle_setup_key(key),
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
        if key.code == KeyCode::Char('c') && self.search.mode != VimMode::Insert {
            self.return_to = Mode::Search;
            self.enter_setup();
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
                    self.search_focused = false;
                    self.enter_new();
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
                KeyCode::Char('n') => self.enter_new(),
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
            NewFocus::Engine | NewFocus::Template => true,
            NewFocus::Title => self.title.mode == VimMode::Normal,
            NewFocus::Icon => self.icon.mode == VimMode::Normal,
        };
        if key.code == KeyCode::Char('s') && can_search {
            self.page = Mode::Search;
            self.search_focused = true;
            self.search.set_normal();
            return Ok(AppAction::None);
        }
        if key.code == KeyCode::Char('c') && can_search {
            self.return_to = Mode::New;
            self.enter_setup();
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
                (NewFocus::Icon, false) => NewFocus::Template,
                (NewFocus::Template, false) => NewFocus::Engine,
                (NewFocus::Engine, true) => NewFocus::Template,
                (NewFocus::Title, true) => NewFocus::Engine,
                (NewFocus::Icon, true) => NewFocus::Title,
                (NewFocus::Template, true) => NewFocus::Icon,
            };
            match self.new_focus {
                NewFocus::Engine | NewFocus::Template => {}
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
            let agents = self.config.enabled_agents.clone();
            match key.code {
                KeyCode::Char('j') | KeyCode::Char('l') | KeyCode::Down | KeyCode::Right => {
                    self.engine = next_engine(&agents, self.engine, 1)
                }
                KeyCode::Char('k') | KeyCode::Char('h') | KeyCode::Up | KeyCode::Left => {
                    self.engine = next_engine(&agents, self.engine, -1)
                }
                KeyCode::Char(' ') => self.engine = next_engine(&agents, self.engine, 1),
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
        if self.new_focus == NewFocus::Icon {
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
                self.new_focus = NewFocus::Template;
            }
            return Ok(AppAction::None);
        }
        // NewFocus::Template — a picker like Engine's, not a text field: the
        // last slot (index == self.templates.len()) is always the synthetic
        // "(none)" option.
        let last = self.templates.len();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.template_index = (self.template_index + 1).min(last)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.template_index = self.template_index.saturating_sub(1)
            }
            KeyCode::Char(' ') => self.template_index = (self.template_index + 1) % (last + 1),
            KeyCode::Enter => {
                let template = self.templates.get(self.template_index).cloned();
                let mut conversation = create_conversation(
                    &self.config,
                    self.engine,
                    &self.title.text(),
                    template.as_deref(),
                )?;
                let icon = self.icon.text();
                update_icon(&conversation, &icon)?;
                conversation.icon = if icon.trim().is_empty() {
                    "⬜".into()
                } else {
                    icon.trim().chars().take(2).collect()
                };
                return Ok(AppAction::Open(conversation));
            }
            KeyCode::Esc | KeyCode::Char('q') => {}
            _ => {}
        }
        Ok(AppAction::None)
    }

    fn handle_setup_key(&mut self, key: KeyEvent) -> Result<AppAction> {
        // Setup is mandatory (can't be cancelled away from) exactly when
        // there's no saved config yet to fall back to.
        let mandatory = self.config.enabled_agents.is_empty();
        let dir_normal = self.setup_dir.mode == VimMode::Normal;
        if !mandatory && self.setup_focus == SetupFocus::Agents {
            match key.code {
                KeyCode::Char('s') => {
                    self.page = Mode::Search;
                    self.search_focused = true;
                    self.search.set_normal();
                    return Ok(AppAction::None);
                }
                KeyCode::Char('n') => {
                    self.enter_new();
                    return Ok(AppAction::None);
                }
                KeyCode::Esc => {
                    self.page = self.return_to.clone();
                    return Ok(AppAction::None);
                }
                _ => {}
            }
        }
        if !mandatory && key.code == KeyCode::Esc && dir_normal {
            self.page = self.return_to.clone();
            return Ok(AppAction::None);
        }
        if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
            self.setup_focus = match self.setup_focus {
                SetupFocus::Agents => {
                    self.setup_dir.set_insert();
                    SetupFocus::Dir
                }
                SetupFocus::Dir => {
                    self.setup_dir.set_normal();
                    SetupFocus::Agents
                }
            };
            return Ok(AppAction::None);
        }
        match self.setup_focus {
            SetupFocus::Agents => {
                let last = self.setup_agents.len().saturating_sub(1);
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        self.setup_cursor = (self.setup_cursor + 1).min(last)
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        self.setup_cursor = self.setup_cursor.saturating_sub(1)
                    }
                    KeyCode::Char('J') => {
                        if self.setup_cursor < last {
                            self.setup_agents.swap(self.setup_cursor, self.setup_cursor + 1);
                            self.setup_enabled.swap(self.setup_cursor, self.setup_cursor + 1);
                            self.setup_cursor += 1;
                        }
                    }
                    KeyCode::Char('K') => {
                        if self.setup_cursor > 0 {
                            self.setup_agents.swap(self.setup_cursor, self.setup_cursor - 1);
                            self.setup_enabled.swap(self.setup_cursor, self.setup_cursor - 1);
                            self.setup_cursor -= 1;
                        }
                    }
                    KeyCode::Char(' ') | KeyCode::Enter => self.setup_toggle(),
                    _ => {}
                }
                Ok(AppAction::None)
            }
            SetupFocus::Dir => {
                if matches!(self.setup_dir.handle(key), FieldResult::Submit) {
                    self.submit_setup()?;
                }
                Ok(AppAction::None)
            }
        }
    }

    fn submit_setup(&mut self) -> Result<()> {
        let enabled_agents: Vec<Engine> = self
            .setup_agents
            .iter()
            .copied()
            .zip(self.setup_enabled.iter().copied())
            .filter_map(|(engine, on)| on.then_some(engine))
            .collect();
        if enabled_agents.is_empty() {
            self.status = "enable at least one coding agent".into();
            return Ok(());
        }
        let raw = self.setup_dir.text();
        let trimmed = raw.trim();
        let chats_dir = if trimmed.is_empty() {
            config::default_chats_dir()?
        } else {
            expand_home(trimmed)
        };
        if let Err(error) = fs::create_dir_all(&chats_dir) {
            self.status = format!("could not create {}: {error}", chats_dir.display());
            return Ok(());
        }
        let new_config = Config {
            enabled_agents,
            chats_dir,
        };
        new_config.save()?;
        self.config = new_config;
        self.engine = self
            .config
            .enabled_agents
            .first()
            .copied()
            .unwrap_or(Engine::Claude);
        self.status = "Settings saved".into();
        self.page = self.return_to.clone();
        self.reload();
        if crate::hyprland::needs_source_line() == Some(true) {
            self.modal = Some(Modal::ConfirmHyprlandRule);
        }
        Ok(())
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
                                let next =
                                    next_engine(&self.config.enabled_agents, conversation.engine, 1);
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
            Modal::ConfirmHyprlandRule => match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.status = match crate::hyprland::write_source_line() {
                        Ok(()) => "Hyprland window rule added".into(),
                        Err(error) => format!("could not add Hyprland window rule: {error}"),
                    };
                }
                KeyCode::Char('n') | KeyCode::Esc | KeyCode::Char('q') => {}
                _ => self.modal = Some(Modal::ConfirmHyprlandRule),
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
            Mode::Chat(_) | Mode::Setup => Ok(AppAction::None),
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
            // Agent buttons are laid out dynamically (one per enabled
            // agent), so a click just cycles forward rather than hit-testing
            // individual button rects.
            self.engine = next_engine(&self.config.enabled_agents, self.engine, 1);
            self.new_focus = NewFocus::Engine;
        } else if point_in(self.new_title_rect, mouse.column, mouse.row) {
            self.new_focus = NewFocus::Title;
            self.title.set_insert();
        } else if point_in(self.new_icon_rect, mouse.column, mouse.row) {
            self.new_focus = NewFocus::Icon;
            self.icon.set_insert();
        } else if point_in(self.new_template_rect, mouse.column, mouse.row) {
            // Same simplification as the agent buttons above: cycle rather
            // than hit-test individual list rows.
            let last = self.templates.len();
            self.template_index = (self.template_index + 1) % (last + 1);
            self.new_focus = NewFocus::Template;
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

/// Cycles to the next (delta=1) or previous (delta=-1) agent in `agents`,
/// wrapping around. Falls back to the first agent if `current` isn't in the
/// list (e.g. a conversation's engine was later disabled) and to `current`
/// unchanged if `agents` is empty.
fn next_engine(agents: &[Engine], current: Engine, delta: i32) -> Engine {
    if agents.is_empty() {
        return current;
    }
    let index = agents.iter().position(|&engine| engine == current).unwrap_or(0);
    let len = agents.len() as i32;
    let next = (index as i32 + delta).rem_euclid(len) as usize;
    agents[next]
}

/// Builds the setup checklist's (agents-in-rank-order, enabled-flags) pair
/// from a config. Agents not yet in `config.enabled_agents` are appended
/// (disabled) in `Engine::ALL` order — covers both a placeholder/first-run
/// config (nothing enabled yet, so everything starts unchecked — the user
/// opts in to what they actually have) and a newly-supported agent showing
/// up in an existing config (also unchecked, same reasoning).
fn setup_state_from(config: &Config) -> (Vec<Engine>, Vec<bool>) {
    let mut order = config.enabled_agents.clone();
    for engine in Engine::ALL {
        if !order.contains(&engine) {
            order.push(engine);
        }
    }
    let enabled = order
        .iter()
        .map(|engine| config.enabled_agents.contains(engine))
        .collect();
    (order, enabled)
}

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/").or_else(|| (path == "~").then_some("")) {
        Some(rest) => std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(rest))
            .unwrap_or_else(|| PathBuf::from(path)),
        None => PathBuf::from(path),
    }
}

pub fn run_tui(
    page: Mode,
    conversations: Vec<Conversation>,
    config: Config,
) -> Result<Option<Conversation>> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    enable_raw_mode()?;
    // Fixed, app-controlled title (independent of how the terminal/launcher
    // invoked us) so the Hyprland window rule lmchatty writes for itself
    // (crate::hyprland) can match on it reliably.
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        SetTitle(crate::hyprland::WINDOW_TITLE)
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(page, conversations, config);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> App {
        let config = Config {
            enabled_agents: Vec::new(),
            chats_dir: PathBuf::from("/tmp/lmchatty-test"),
        };
        App::new(Mode::Setup, Vec::new(), config)
    }

    fn toggle(app: &mut App, engine: Engine) {
        app.setup_cursor = app.setup_agents.iter().position(|&e| e == engine).unwrap();
        app.setup_toggle();
    }

    #[test]
    fn toggle_ranks_agents_in_selection_order_not_list_order() {
        let mut app = test_app();
        // Engine::ALL is [Claude, Codex, OpenCode]; select out of that
        // order — rank should follow selection order, not list position.
        toggle(&mut app, Engine::Claude);
        toggle(&mut app, Engine::OpenCode);
        toggle(&mut app, Engine::Codex);
        assert_eq!(
            app.setup_agents,
            vec![Engine::Claude, Engine::OpenCode, Engine::Codex]
        );
        assert_eq!(app.setup_enabled, vec![true, true, true]);
    }

    #[test]
    fn disabling_moves_to_top_of_disabled_block() {
        let mut app = test_app();
        for engine in [Engine::Claude, Engine::Codex, Engine::OpenCode] {
            toggle(&mut app, engine);
        }
        toggle(&mut app, Engine::Claude); // disable it again
        assert_eq!(
            app.setup_agents,
            vec![Engine::Codex, Engine::OpenCode, Engine::Claude]
        );
        assert_eq!(app.setup_enabled, vec![true, true, false]);
    }
}
