use super::{App, EditKind, Modal, NewFocus};
use crate::storage::created_label;
use crate::vim::VimMode;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

pub fn draw(frame: &mut Frame, app: &mut App) {
    match &app.page {
        crate::model::Mode::Search => draw_search(frame, app),
        crate::model::Mode::New => draw_new(frame, app),
        crate::model::Mode::Chat(_) => {}
    }
    let selected_title = app
        .selected_conversation()
        .map(|conversation| conversation.title);
    if let Some(modal) = &app.modal {
        app.modal_rect = if matches!(modal, Modal::Actions { .. }) {
            bottom_actions_rect(frame.area())
        } else {
            centered_rect(52, 44, frame.area())
        };
    }
    if let Some(modal) = &mut app.modal {
        draw_modal(frame, selected_title.as_deref(), modal);
    }
}

fn draw_search(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());
    app.search_rect = chunks[1];
    app.list_rect = chunks[2];
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " lmchatty ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  [n] new conversation"),
        ])),
        chunks[0],
    );
    let title = if app.search_focused {
        format!("Search / {}", vim_label(app.search.mode))
    } else {
        "Search".into()
    };
    app.search
        .area
        .set_block(Block::default().borders(Borders::ALL).title(title));
    frame.render_widget(&app.search.area, chunks[1]);

    let visible = app.visible();
    let items = if visible.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No conversations yet. Press n to create one.",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        // Each conversation is followed by its own unselectable 1-line
        // spacer ListItem, so the highlight box never covers the gap.
        visible
            .iter()
            .flat_map(|conversation| {
                let pin = if conversation.pinned { "📌" } else { "  " };
                [
                    ListItem::new(vec![
                        Line::from(vec![
                            Span::styled(
                                format!("{} ", conversation.icon),
                                Style::default().fg(Color::Yellow),
                            ),
                            Span::raw(conversation.title.clone()),
                            Span::styled(
                                format!("  {}", conversation.hash),
                                Style::default().fg(Color::DarkGray),
                            ),
                        ]),
                        Line::from(Span::styled(
                            format!(
                                "{pin} {} · {}",
                                created_label(conversation.created),
                                conversation.engine.label()
                            ),
                            Style::default().fg(Color::DarkGray),
                        )),
                    ]),
                    ListItem::new(Line::from("")),
                ]
            })
            .collect()
    };
    if !visible.is_empty() {
        // Each conversation occupies 2 rendered items (itself + its spacer),
        // so its index in `items` is double its index in `visible`.
        app.list_state
            .select(Some(app.selected.min(visible.len() - 1) * 2));
    } else {
        app.list_state.select(None);
    }
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Conversations · pinned first · newest first"),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    frame.render_stateful_widget(list, chunks[2], &mut app.list_state);
    frame.render_widget(
        Paragraph::new(format!(
            "{}  |  j/k move  Space actions  Ctrl+Q quit",
            app.status
        )),
        chunks[3],
    );
}

fn draw_new(frame: &mut Frame, app: &mut App) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " lmchatty ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  [s] search conversations"),
        ])),
        sections[0],
    );
    let area = sections[1];
    frame.render_widget(Clear, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(0)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(area);
    app.new_engine_rect = chunks[0];
    app.new_title_rect = chunks[1];
    app.new_icon_rect = chunks[2];
    let claude = if app.engine == crate::model::Engine::Claude && app.new_focus == NewFocus::Engine
    {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    };
    let codex = if app.engine == crate::model::Engine::Codex && app.new_focus == NewFocus::Engine {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    };
    let engine_block = Block::default()
        .borders(Borders::ALL)
        .title("Choose a coding agent");
    let engine_inner = engine_block.inner(chunks[0]);
    frame.render_widget(engine_block, chunks[0]);
    // Paragraph only aligns horizontally, so center the single line of
    // content vertically ourselves within the block's inner area.
    let engine_line_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .split(engine_inner)[1];
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("[ Claude ]", claude),
            Span::raw("    "),
            Span::styled("[ Codex ]", codex),
        ]))
        .alignment(Alignment::Center),
        engine_line_area,
    );
    app.title
        .area
        .set_block(Block::default().borders(Borders::ALL).title(
            if app.new_focus == NewFocus::Title {
                format!("Title · {}", vim_label(app.title.mode))
            } else {
                "Title".into()
            },
        ));
    frame.render_widget(&app.title.area, chunks[1]);
    let icon_area = chunks[2];
    app.icon
        .area
        .set_block(Block::default().borders(Borders::ALL).title(
            if app.new_focus == NewFocus::Icon {
                format!("Icon · {}", vim_label(app.icon.mode))
            } else {
                "Icon".into()
            },
        ));
    frame.render_widget(&app.icon.area, icon_area);
    frame.render_widget(
        Paragraph::new("Tab next · Shift-Tab previous · Enter next · Ctrl+Q quit"),
        sections[2],
    );
}

fn draw_modal(frame: &mut Frame, selected_title: Option<&str>, modal: &mut Modal) {
    let area = if matches!(modal, Modal::Actions { .. }) {
        bottom_actions_rect(frame.area())
    } else {
        centered_rect(52, 44, frame.area())
    };
    frame.render_widget(Clear, area);
    match modal {
        Modal::Actions { selected } => {
            let labels = [
                "Rename",
                "Change icon",
                "Change agent",
                "Pin / unpin",
                "Move to trash",
                "Cancel",
            ];
            let items = labels
                .iter()
                .enumerate()
                .map(|(index, label)| {
                    let text = format!("{}{}", if index == *selected { "› " } else { "  " }, label);
                    let style = if index == *selected {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    ListItem::new(Line::from(Span::styled(text, style)))
                })
                .collect::<Vec<_>>();
            frame.render_widget(
                List::new(items).block(Block::default().borders(Borders::ALL).title(format!(
                    "Actions · {}",
                    selected_title.unwrap_or("Conversation")
                ))),
                area,
            );
        }
        Modal::Edit { kind, field } => {
            let label = match kind {
                EditKind::Rename => "Rename conversation",
                EditKind::Icon => "Change icon",
            };
            field.area.set_block(field.title(label));
            frame.render_widget(&field.area, area);
        }
        Modal::ConfirmTrash => {
            let text = format!(
                "Move '{}' to trash?\n\ny / Enter  confirm\nn / Esc    cancel",
                selected_title.unwrap_or("this conversation")
            );
            frame.render_widget(
                Paragraph::new(text).alignment(Alignment::Center).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Confirm")
                        .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                ),
                area,
            );
        }
    }
}

fn bottom_actions_rect(area: Rect) -> Rect {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    let list_area = sections[2];
    let height = list_area.height.min(8);
    Rect {
        x: list_area.x,
        y: list_area.y + list_area.height.saturating_sub(height),
        width: list_area.width,
        height,
    }
}

fn vim_label(mode: VimMode) -> &'static str {
    match mode {
        VimMode::Normal => "NORMAL",
        VimMode::Insert => "INSERT",
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height) / 2),
            Constraint::Percentage(height),
            Constraint::Percentage((100 - height) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width) / 2),
            Constraint::Percentage(width),
            Constraint::Percentage((100 - width) / 2),
        ])
        .split(vertical[1])[1]
}
