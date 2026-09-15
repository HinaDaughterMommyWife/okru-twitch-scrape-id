//! Rendering. Pure function of `App` (plus table scroll state).

use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, Padding, Paragraph, Row, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Table, Wrap,
};
use ratatui::Frame;

use okru_tui::db::User;

use crate::app::{App, Mode, ToastKind};
use crate::form::{Form, FormField};
use crate::input::TextInput;

const ACCENT: Color = Color::Rgb(145, 71, 255); // Twitch purple
const ACCENT_SOFT: Color = Color::Rgb(191, 148, 255);
const OK: Color = Color::Rgb(80, 200, 120);
const WARN: Color = Color::Rgb(240, 190, 80);
const ERR: Color = Color::Rgb(240, 90, 90);
const INFO: Color = Color::Rgb(100, 180, 240);
const DIM: Color = Color::Rgb(120, 120, 135);
const CHIP_BG: Color = Color::Rgb(55, 40, 90);
const CHIP_WL_BG: Color = Color::Rgb(35, 60, 70);
const SURFACE: Color = Color::Rgb(24, 24, 30);

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    draw_header(frame, app, header);

    // Side by side on wide terminals, stacked on narrow ones.
    let [list_area, detail_area] = if body.width >= 90 {
        Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(body)
    } else {
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body)
    };
    draw_list(frame, app, list_area);
    // Bot activity (IPC events) under the detail panel; grows while focused.
    let activity = if matches!(app.mode, Mode::Activity) {
        Constraint::Percentage(65)
    } else {
        Constraint::Length(if detail_area.height >= 26 { 9 } else { 6 })
    };
    let [detail_area, activity_area] =
        Layout::vertical([Constraint::Min(6), activity]).areas(detail_area);
    draw_detail(frame, app, detail_area);
    draw_activity(frame, app, activity_area);
    draw_footer(frame, app, footer);

    match &app.mode {
        Mode::Form(form) => draw_form(frame, form),
        Mode::ConfirmDelete { slug, input, .. } => draw_confirm_delete(frame, slug, input),
        Mode::Help => draw_help(frame),
        Mode::List | Mode::Filter | Mode::Activity => {}
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let left = Line::from(vec![
        Span::styled(" okru ", Style::new().bg(ACCENT).fg(Color::White).bold()),
        Span::styled(" canales ", Style::new().fg(ACCENT_SOFT).bold()),
    ]);
    let bot = if app.link.is_connected() {
        Span::styled(format!(" ● bot :{} ", app.link.port), Style::new().fg(Color::Black).bg(OK))
    } else {
        Span::styled(format!(" ○ bot offline :{} ", app.link.port), Style::new().fg(Color::Black).bg(ERR))
    };
    let right = Line::from(vec![
        Span::styled(format!("{} ", app.db_path.display()), Style::new().fg(DIM)),
        Span::styled("│ ", Style::new().fg(DIM)),
        Span::styled(format!("{} usuarios ", app.users.len()), Style::new().fg(Color::White)),
        Span::styled(format!("● {} activos ", app.active_count()), Style::new().fg(OK)),
        bot,
    ])
    .alignment(Alignment::Right);
    frame.render_widget(Paragraph::new(left), area);
    frame.render_widget(Paragraph::new(right), area);
}

fn panel(title: Line<'static>, focused: bool) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(if focused { ACCENT } else { DIM }))
        .title(title)
        .padding(Padding::horizontal(1))
}

fn draw_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let filtering = matches!(app.mode, Mode::Filter);
    let mut title = vec![Span::styled(" Usuarios ", Style::new().bold().fg(Color::White))];
    if filtering || !app.filter.value().is_empty() {
        title.push(Span::styled(
            format!(" / {} ", app.filter.value()),
            Style::new().fg(Color::Black).bg(if filtering { WARN } else { ACCENT_SOFT }),
        ));
        title.push(Span::raw(" "));
    }
    let block = panel(Line::from(title), matches!(app.mode, Mode::List | Mode::Filter));

    let rows: Vec<Row> = app
        .visible()
        .into_iter()
        .map(|u| {
            let (dot, dot_color) = if u.enabled { ("●", OK) } else { ("○", DIM) };
            let text_style = if u.enabled { Style::new() } else { Style::new().fg(DIM) };
            Row::new(vec![
                Cell::from(Span::styled(dot, Style::new().fg(dot_color))),
                Cell::from(Span::styled(u.slug.clone(), text_style.bold())),
                Cell::from(Span::styled(format!("#{}", u.twitch_channel), text_style.fg(if u.enabled { ACCENT_SOFT } else { DIM }))),
                Cell::from(Span::styled(u.commands.len().to_string(), text_style)).style(Style::new()),
                Cell::from(Span::styled(u.whitelist.len().to_string(), text_style)),
            ])
        })
        .collect();

    let empty = rows.is_empty();
    let count = rows.len();
    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Fill(2),
            Constraint::Fill(2),
            Constraint::Length(3),
            Constraint::Length(3),
        ],
    )
    .header(
        Row::new(vec!["", "slug", "canal", "cmd", "wl"])
            .style(Style::new().fg(DIM).add_modifier(Modifier::UNDERLINED)),
    )
    .row_highlight_style(Style::new().bg(SURFACE).add_modifier(Modifier::BOLD))
    .highlight_symbol(Span::styled("▌", Style::new().fg(ACCENT)))
    .block(block);

    frame.render_stateful_widget(table, area, &mut app.table);

    if empty {
        let msg = if app.filter.value().is_empty() {
            "Sin usuarios · pulsa  a  para crear el primero"
        } else {
            "Nada coincide con el filtro · Esc para limpiar"
        };
        let inner = area.inner(Margin::new(2, 3));
        frame.render_widget(
            Paragraph::new(msg).fg(DIM).alignment(Alignment::Center).wrap(Wrap { trim: true }),
            inner,
        );
    } else if count + 3 > area.height as usize {
        let mut state = ScrollbarState::new(count).position(app.table.selected().unwrap_or(0));
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight).style(Style::new().fg(DIM)),
            area.inner(Margin::new(0, 1)),
            &mut state,
        );
    }

    if filtering {
        // border + " Usuarios " + " / "
        let x = area.x + 1 + 10 + 3 + app.filter.cursor_col();
        frame.set_cursor_position((x.min(area.right().saturating_sub(2)), area.y));
    }
}

fn chips(items: &[String], bg: Color) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for item in items {
        spans.push(Span::styled(format!(" {item} "), Style::new().bg(bg).fg(Color::White)));
        spans.push(Span::raw(" "));
    }
    spans
}

fn kv(label: &str, value: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![Span::styled(format!("{label:<11}"), Style::new().fg(DIM))];
    spans.extend(value);
    Line::from(spans)
}

fn draw_detail(frame: &mut Frame, app: &App, area: Rect) {
    let Some(user) = app.selected() else {
        let block = panel(Line::from(" Detalle "), false);
        frame.render_widget(
            Paragraph::new("\nSelecciona un usuario").fg(DIM).alignment(Alignment::Center).block(block),
            area,
        );
        return;
    };

    let title = Span::styled(format!(" {} ", user.display_name), Style::new().bold().fg(Color::White));
    let block = panel(Line::from(title), false);
    let lines = detail_lines(user, app.bot_in_channel(user), &app.config.page_url(&user.slug), &app.config.vods_url(&user.slug));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
}

/// `in_channel`: Twitch-confirmed bot presence (`None` = backend offline).
fn detail_lines(user: &User, in_channel: Option<bool>, page_url: &str, vods_url: &str) -> Text<'static> {
    let status = match (user.enabled, in_channel) {
        (false, _) => Span::styled("○ desactivado — sin bot ni página", Style::new().fg(DIM)),
        (true, Some(true)) => Span::styled("● activo — bot dentro del canal", Style::new().fg(OK)),
        (true, Some(false)) => Span::styled("◌ activo — esperando JOIN del bot", Style::new().fg(WARN)),
        (true, None) => Span::styled("● activo — bot offline, sin confirmar", Style::new().fg(DIM)),
    };

    let mut commands = chips(&user.commands, CHIP_BG);
    if commands.is_empty() {
        commands.push(Span::styled("—", Style::new().fg(DIM)));
    }
    let mut whitelist = chips(&user.whitelist, CHIP_WL_BG);
    if whitelist.is_empty() {
        whitelist.push(Span::styled("solo mods y broadcaster", Style::new().fg(DIM)));
    }

    Text::from(vec![
        Line::raw(""),
        kv("Estado", vec![status]),
        Line::raw(""),
        kv("Slug", vec![Span::styled(user.slug.clone(), Style::new().bold())]),
        kv("Página", vec![Span::styled(page_url.to_string(), Style::new().fg(INFO).underlined())]),
        kv("VODs", vec![Span::styled(vods_url.to_string(), Style::new().fg(INFO))]),
        Line::raw(""),
        kv("Twitch", vec![Span::styled(format!("#{}", user.twitch_channel), Style::new().fg(ACCENT_SOFT))]),
        kv("VK", vec![Span::raw(user.idvk.clone())]),
        Line::raw(""),
        kv("Comandos", commands),
        Line::raw(""),
        kv("Whitelist", whitelist),
        Line::raw(""),
        kv("Creado", vec![Span::styled(user.created_at.clone(), Style::new().fg(DIM))]),
        kv("Editado", vec![Span::styled(user.updated_at.clone(), Style::new().fg(DIM))]),
    ])
}

fn ago(at: std::time::Instant) -> String {
    let secs = at.elapsed().as_secs();
    match secs {
        0..=59 => format!("{secs:>2}s"),
        60..=3599 => format!("{:>2}m", secs / 60),
        _ => format!("{:>2}h", secs / 3600),
    }
}

fn draw_activity(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = matches!(app.mode, Mode::Activity);
    let rows = area.height.saturating_sub(2) as usize;
    app.activity_page = rows.max(1);
    app.activity_scroll = app.activity_scroll.min(app.max_activity_scroll());

    let mut title = vec![Span::styled(" Actividad del bot ", Style::new().bold().fg(Color::White))];
    if !app.activity.is_empty() {
        title.push(Span::styled(
            format!("{}/{} ", app.activity.len(), crate::app::ACTIVITY_MAX),
            Style::new().fg(DIM),
        ));
    }
    if app.activity_scroll > 0 {
        title.push(Span::styled(
            format!(" ↑ {} más nuevos ", app.activity_scroll),
            Style::new().fg(Color::Black).bg(WARN),
        ));
        title.push(Span::raw(" "));
    } else if !focused {
        title.push(Span::styled("tab ", Style::new().fg(DIM)));
    }
    let block = panel(Line::from(title), focused);
    let lines: Vec<Line> = if app.activity.is_empty() {
        let text = if app.link.is_connected() {
            "sin eventos todavía"
        } else {
            "esperando conexión con okru-backend…"
        };
        vec![Line::styled(text, Style::new().fg(DIM).italic())]
    } else {
        app.activity
            .iter()
            .skip(app.activity_scroll)
            .take(rows)
            .map(|a| {
                let color = match a.kind {
                    ToastKind::Ok => OK,
                    ToastKind::Info => INFO,
                    ToastKind::Error => ERR,
                };
                Line::from(vec![
                    Span::styled(format!("{} ", ago(a.at)), Style::new().fg(DIM)),
                    Span::styled("▎", Style::new().fg(color)),
                    Span::raw(a.text.clone()),
                ])
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(lines).block(block), area);

    if app.activity.len() > rows {
        let mut state = ScrollbarState::new(app.max_activity_scroll() + 1).position(app.activity_scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(Style::new().fg(if focused { ACCENT } else { DIM })),
            area.inner(Margin::new(0, 1)),
            &mut state,
        );
    }
}

fn key_hint(key: &str, label: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(format!(" {key} "), Style::new().bg(SURFACE).fg(ACCENT_SOFT).bold()),
        Span::styled(format!(" {label}  "), Style::new().fg(DIM)),
    ]
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let hints: &[(&str, &str)] = match app.mode {
        Mode::List => &[("a", "nuevo"), ("e", "editar"), ("c", "clonar"), ("d", "borrar"), ("␣", "on/off"), ("/", "filtrar"), ("tab", "actividad"), ("?", "ayuda"), ("q", "salir")],
        Mode::Activity => &[("j/k", "scroll"), ("pgup/pgdn", "página"), ("g/G", "nuevo/viejo"), ("x", "limpiar"), ("tab/esc", "volver")],
        Mode::Filter => &[("⏎", "aplicar"), ("esc", "limpiar")],
        Mode::Form(_) => &[("tab", "siguiente"), ("⇧tab", "anterior"), ("ctrl+s", "guardar"), ("esc", "cancelar")],
        Mode::ConfirmDelete { .. } => &[("⏎", "confirmar"), ("esc", "cancelar")],
        Mode::Help => &[("cualquier tecla", "cerrar")],
    };
    let line = Line::from(hints.iter().flat_map(|(k, l)| key_hint(k, l)).collect::<Vec<_>>());
    frame.render_widget(Paragraph::new(line), area);

    if let Some(toast) = &app.toast {
        let color = match toast.kind {
            ToastKind::Ok => OK,
            ToastKind::Info => INFO,
            ToastKind::Error => ERR,
        };
        let text = format!(" {} ", toast.message);
        let width = (Span::raw(text.as_str()).width() as u16).min(area.width);
        let toast_area = Rect { x: area.right() - width, width, ..area };
        frame.render_widget(Clear, toast_area);
        frame.render_widget(Paragraph::new(text).style(Style::new().bg(color).fg(Color::Black).bold()), toast_area);
    }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

fn modal(frame: &mut Frame, area: Rect, title: Line<'static>, color: Color) -> Rect {
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(color))
        .title(title)
        .padding(Padding::new(2, 2, 1, 0))
        .style(Style::new().bg(Color::Rgb(14, 14, 18)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

fn draw_form(frame: &mut Frame, form: &Form) {
    // 3 rows per field (label, input, hint/error) + discard notice.
    let height = FormField::ALL.len() as u16 * 3 + 4;
    let area = centered(frame.area(), 78, height);
    let mut title = vec![Span::styled(form.title(), Style::new().bold().fg(Color::White))];
    if form.is_dirty() {
        title.push(Span::styled("● sin guardar ", Style::new().fg(WARN)));
    }
    let inner = modal(frame, area, Line::from(title), ACCENT);

    let mut constraints: Vec<Constraint> = FormField::ALL.iter().map(|_| Constraint::Length(3)).collect();
    constraints.push(Constraint::Length(1));
    let rows = Layout::vertical(constraints).split(inner);

    for (i, field) in FormField::ALL.iter().copied().enumerate() {
        let focused = form.focus == field;
        let row = rows[i];
        let [label_area, input_area, hint_area] = Layout::vertical([Constraint::Length(1); 3]).areas(row);

        let error = form.error_for(field);
        let marker = if focused { "▶ " } else { "  " };
        let label_style = match (focused, error.is_some()) {
            (_, true) => Style::new().fg(ERR).bold(),
            (true, _) => Style::new().fg(ACCENT_SOFT).bold(),
            _ => Style::new().fg(Color::Gray),
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(marker, Style::new().fg(ACCENT)),
                Span::styled(field.label(), label_style),
            ])),
            label_area,
        );

        let input_rect = Rect { x: input_area.x + 2, width: input_area.width.saturating_sub(2), ..input_area };
        let bg = if focused { SURFACE } else { Color::Rgb(20, 20, 25) };
        if field == FormField::Enabled {
            let (mark, text, color) = if form.enabled { ("[●]", " activo", OK) } else { ("[ ]", " desactivado", DIM) };
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(mark, Style::new().fg(color).bold()),
                    Span::styled(text, Style::new().fg(color)),
                ]))
                .style(Style::new().bg(bg)),
                input_rect,
            );
        } else if let Some(input) = form.input(field) {
            frame.render_widget(
                Paragraph::new(format!(" {}", input.value())).style(Style::new().bg(bg).fg(Color::White)),
                input_rect,
            );
            if focused {
                let x = input_rect.x + 1 + input.cursor_col();
                frame.set_cursor_position((x.min(input_rect.right().saturating_sub(1)), input_rect.y));
            }
        }

        let hint_rect = Rect { x: hint_area.x + 2, width: hint_area.width.saturating_sub(2), ..hint_area };
        let hint = if let Some(err) = error {
            Line::from(Span::styled(format!("✗ {err}"), Style::new().fg(ERR)))
        } else if field.is_list() && !form.chips(field).is_empty() {
            let bg = if field == FormField::Commands { CHIP_BG } else { CHIP_WL_BG };
            Line::from(chips(&form.chips(field), bg))
        } else if focused {
            Line::from(Span::styled(field.hint(), Style::new().fg(DIM).italic()))
        } else {
            Line::raw("")
        };
        frame.render_widget(Paragraph::new(hint), hint_rect);
    }

    if form.confirm_discard {
        frame.render_widget(
            Paragraph::new("Hay cambios sin guardar · Esc otra vez para descartar")
                .fg(WARN)
                .alignment(Alignment::Center),
            rows[FormField::ALL.len()],
        );
    }
}

fn draw_confirm_delete(frame: &mut Frame, slug: &str, input: &TextInput) {
    let area = centered(frame.area(), 60, 10);
    let inner = modal(frame, area, Line::from(Span::styled(" Borrar usuario ", Style::new().fg(ERR).bold())), ERR);
    let [text_area, _, input_area, status_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::raw("Se eliminará "),
                Span::styled(slug.to_string(), Style::new().bold().fg(Color::White)),
                Span::raw(": el bot sale del canal y la página desaparece."),
            ]),
            Line::from(vec![Span::styled("Escribe el slug para confirmar:", Style::new().fg(DIM))]),
        ])
        .wrap(Wrap { trim: true }),
        text_area,
    );

    let matches = input.value().trim() == slug;
    frame.render_widget(
        Paragraph::new(format!(" {}", input.value())).style(Style::new().bg(SURFACE).fg(if matches { ERR } else { Color::White })),
        input_area,
    );
    frame.set_cursor_position((input_area.x + 1 + input.cursor_col(), input_area.y));
    let status = if matches {
        Span::styled("⏎ para borrar definitivamente", Style::new().fg(ERR).bold())
    } else {
        Span::styled("", Style::new())
    };
    frame.render_widget(Paragraph::new(Line::from(status)), status_area);
}

fn draw_help(frame: &mut Frame) {
    let area = centered(frame.area(), 68, 25);
    let inner = modal(frame, area, Line::from(Span::styled(" Ayuda ", Style::new().bold().fg(Color::White))), ACCENT);
    let row = |k: &str, d: &str| {
        Line::from(vec![
            Span::styled(format!("{k:>12}  "), Style::new().fg(ACCENT_SOFT).bold()),
            Span::raw(d.to_string()),
        ])
    };
    let lines = vec![
        Line::styled("Lista", Style::new().fg(DIM).underlined()),
        row("j/k ↑/↓", "moverse · g/G inicio/fin"),
        row("a", "nuevo usuario"),
        row("e / ⏎", "editar seleccionado"),
        row("c", "clonar (mismos comandos/whitelist)"),
        row("d", "borrar (pide confirmar slug)"),
        row("espacio", "activar / desactivar"),
        row("/", "filtrar por slug, canal o nombre"),
        row("r", "recargar desde la DB"),
        row("tab", "actividad del bot: j/k, pgup/pgdn, g/G, x limpiar"),
        row("", "cada cambio avisa al bot por 127.0.0.1:ipcPort"),
        row("q", "salir"),
        Line::raw(""),
        Line::styled("Formulario", Style::new().fg(DIM).underlined()),
        row("tab ⇧tab", "siguiente / anterior campo"),
        row("ctrl+s", "guardar"),
        row("ctrl+w/u", "borrar palabra / hasta el inicio"),
        row("esc", "cancelar (dos veces si hay cambios)"),
        Line::raw(""),
        Line::styled(
            "Los cambios se guardan en SQLite; el bot los aplica en caliente.",
            Style::new().fg(DIM).italic(),
        ),
    ];
    frame.render_widget(Paragraph::new(lines).block(Block::new().borders(Borders::NONE)), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form::Form;
    use okru_tui::db::{Store, UserDraft};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    fn app() -> App {
        let mut store = Store::open_in_memory().unwrap();
        for (slug, name, wl) in [("thedarkraimola", "SapoPerro Ruso", vec!["user1".to_string(), "user2".into()]), ("otro", "Otro Canal", vec![])] {
            store
                .insert(&UserDraft {
                    slug: slug.into(),
                    display_name: name.into(),
                    twitch_channel: slug.into(),
                    idvk: "id1117440596".into(),
                    commands: vec!["#vk".into(), "#okru".into(), "!web".into()],
                    whitelist: wl,
                    ..UserDraft::default()
                })
                .unwrap();
        }
        App::new(store, PathBuf::from("data/okru.db"), okru_tui::config::SharedConfig::default(), crate::link::Link::offline()).unwrap()
    }

    fn render(app: &mut App, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_every_mode_at_several_sizes() {
        let mut app = app();
        for (w, h) in [(120, 36), (70, 30), (30, 10)] {
            app.mode = Mode::List;
            render(&mut app, w, h);
            app.mode = Mode::Filter;
            render(&mut app, w, h);
            for i in 0..30 {
                app.activity.push_front(crate::app::Activity {
                    kind: ToastKind::Info,
                    text: format!("#otro · friend (whitelist) usó #okru {i}"),
                    at: std::time::Instant::now(),
                });
            }
            app.mode = Mode::Activity;
            app.activity_scroll = 3;
            render(&mut app, w, h);
            app.mode = Mode::Form(Box::new(Form::edit(&app.users[1].clone())));
            render(&mut app, w, h);
            app.mode = Mode::Help;
            render(&mut app, w, h);
            app.mode = Mode::ConfirmDelete { id: 1, slug: "otro".into(), input: TextInput::new("ot") };
            render(&mut app, w, h);
        }
    }
}
