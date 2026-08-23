use super::{App, EditKind, Modal, NewFocus, SetupFocus};
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
        crate::model::Mode::Setup => draw_setup(frame, app),
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

/// Shared top bar for every page: the app name on the left, right-aligned
/// key hints, and a thin rule underneath separating it from page content.
/// Always exactly 2 rows tall.
fn draw_header(frame: &mut Frame, area: Rect, right_text: &str) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);
    // Inset the text row by 1 column on each side so "lmchatty" and the
    // hints get an even 1-space margin from both edges; the rule below
    // stays full-width for a clean edge-to-edge divider.
    let inset = Rect {
        x: rows[0].x + 1,
        width: rows[0].width.saturating_sub(2),
        ..rows[0]
    };
    let right_width = (right_text.chars().count() as u16).min(inset.width);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(right_width)])
        .split(inset);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "lmchatty",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        cols[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(right_text, Style::default().fg(Color::DarkGray)))
            .alignment(Alignment::Right),
        cols[1],
    );
    let rule = "─".repeat(area.width as usize);
    frame.render_widget(
        Paragraph::new(Span::styled(rule, Style::default().fg(Color::DarkGray))),
        rows[1],
    );
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
    draw_header(frame, chunks[0], "n new  ·  c setup");
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
    draw_header(frame, sections[0], "s search  ·  c setup");
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
    // One button per *enabled* agent, in rank order — a user with only one
    // agent enabled just sees the one button. The `›` marker carries the
    // selection independent of color, since a background-fill highlight
    // alone can wash out or disappear entirely when the terminal/pane loses
    // focus (many terminals and Zellij itself dim colors for inactive
    // panes) — bold survives that better than color too, so it stays on
    // the selected button even when Engine isn't the focused field.
    let mut spans = Vec::new();
    for (index, engine) in app.config.enabled_agents.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("    "));
        }
        let selected = *engine == app.engine;
        let marker = if selected { "› " } else { "  " };
        let style = if selected && app.new_focus == NewFocus::Engine {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        };
        spans.push(Span::styled(marker, Style::default().fg(Color::Cyan)));
        spans.push(Span::styled(format!("[ {} ]", engine.label()), style));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
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

fn draw_setup(frame: &mut Frame, app: &mut App) {
    // Mirrors `App::handle_setup_key`'s `mandatory` check: no saved config
    // yet means there's nowhere else to go, so the header can't advertise
    // shortcuts that don't work.
    let mandatory = app.config.enabled_agents.is_empty();
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());
    let right = if mandatory {
        "first run".to_owned()
    } else {
        "n new  ·  s search".to_owned()
    };
    draw_header(frame, sections[0], &right);

    let area = sections[1];
    frame.render_widget(Clear, area);
    let list_height = app.setup_agents.len() as u16 + 2;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(0)
        .constraints([
            Constraint::Length(list_height),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    let mut rank = 0;
    let items: Vec<ListItem> = app
        .setup_agents
        .iter()
        .zip(app.setup_enabled.iter())
        .enumerate()
        .map(|(index, (engine, enabled))| {
            let cursor = if index == app.setup_cursor && app.setup_focus == SetupFocus::Agents {
                "› "
            } else {
                "  "
            };
            let rank_label = if *enabled {
                rank += 1;
                format!("{rank}. ")
            } else {
                "   ".to_owned()
            };
            let marker = if *enabled { "[x]" } else { "[ ]" };
            let style = if index == app.setup_cursor && app.setup_focus == SetupFocus::Agents {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(
                format!("{cursor}{rank_label}{marker} {}", engine.label()),
                style,
            )))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Enabled coding agents · rank = launch order"),
        ),
        chunks[0],
    );

    app.setup_dir.area.set_block(Block::default().borders(Borders::ALL).title(
        if app.setup_focus == SetupFocus::Dir {
            format!("Chats directory · {}", vim_label(app.setup_dir.mode))
        } else {
            "Chats directory".into()
        },
    ));
    frame.render_widget(&app.setup_dir.area, chunks[1]);

    let help = if !app.status.is_empty() {
        app.status.clone()
    } else if mandatory {
        "j/k move · Space toggle · J/K reorder · Tab switch field · Enter continue".to_owned()
    } else {
        "j/k move · Space toggle · J/K reorder · Enter save · Esc/s/n leave".to_owned()
    };
    // Anchored to the outer layout's bottom row (`sections[2]`), not the
    // inner content chunks, so it sits flush with the terminal's last line
    // like the Search/New pages' footers do.
    frame.render_widget(Paragraph::new(help), sections[2]);
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
