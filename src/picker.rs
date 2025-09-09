use std::{
    io::{self, Stdout},
    sync::Arc,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    style::Colored,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use nucleo::{
    pattern::{CaseMatching, Normalization},
    Nucleo, Snapshot,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{
        block::Position, Block, Borders, HighlightSpacing, List, ListDirection, ListItem,
        ListState, Paragraph, Wrap,
    },
    Frame, Terminal,
};

use crate::{
    configs::PickerColorConfig,
    execute_tmux_command,
    keymap::{default_keymap, Keymap, PickerAction},
    TmsError,
};

pub struct Picker {
    matcher: Nucleo<String>,
    preview_command: Option<String>,

    colors: Option<PickerColorConfig>,

    selection: ListState,
    filter: String,
    cursor_pos: u16,
    keymap: Keymap,
}

impl Picker {
    pub fn new(list: &[String], preview_command: Option<String>, keymap: Option<Keymap>) -> Self {
        let matcher = Nucleo::new(nucleo::Config::DEFAULT, Arc::new(request_redraw), None, 1);

        let injector = matcher.injector();

        for str in list {
            injector.push(str.to_owned(), |dst| dst[0] = str.to_owned().into());
        }

        let mut default_keymap = default_keymap();

        if let Some(keymap) = keymap {
            keymap.iter().for_each(|(event, action)| {
                default_keymap.insert(*event, *action);
            })
        }

        Picker {
            matcher,
            preview_command,
            colors: None,
            selection: ListState::default(),
            filter: String::default(),
            cursor_pos: 0,
            keymap: default_keymap,
        }
    }

    pub fn set_colors(mut self, colors: Option<PickerColorConfig>) -> Self {
        self.colors = colors;

        self
    }

    pub fn run(&mut self) -> Result<Option<String>, TmsError> {
        enable_raw_mode().map_err(|e| TmsError::TuiError(e.to_string()))?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen).map_err(|e| TmsError::TuiError(e.to_string()))?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).map_err(|e| TmsError::TuiError(e.to_string()))?;

        let selected_str = self
            .main_loop(&mut terminal)
            .map_err(|e| TmsError::TuiError(e.to_string()))?;

        disable_raw_mode().map_err(|e| TmsError::TuiError(e.to_string()))?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)
            .map_err(|e| TmsError::TuiError(e.to_string()))?;
        terminal
            .show_cursor()
            .map_err(|e| TmsError::TuiError(e.to_string()))?;

        Ok(selected_str)
    }

    fn main_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ) -> Result<Option<String>, TmsError> {
        loop {
            terminal
                .draw(|f| self.render(f))
                .map_err(|e| TmsError::TuiError(e.to_string()))?;

            if let Event::Key(key) = event::read().map_err(|e| TmsError::TuiError(e.to_string()))? {
                if key.kind == KeyEventKind::Press {
                    match self.keymap.get(&key.into()) {
                        Some(PickerAction::Cancel) => return Ok(None),
                        Some(PickerAction::Confirm) => {
                            if let Some(selected) = self.get_selected() {
                                return Ok(Some(selected));
                            }
                        }
                        Some(PickerAction::Backspace) => self.remove_filter(),
                        Some(PickerAction::Delete) => self.delete(),
                        Some(PickerAction::DeleteWord) => self.delete_word(),
                        Some(PickerAction::DeleteToLineStart) => self.delete_to_line(false),
                        Some(PickerAction::DeleteToLineEnd) => self.delete_to_line(true),
                        Some(PickerAction::MoveUp) => self.move_up(),
                        Some(PickerAction::MoveDown) => self.move_down(),
                        Some(PickerAction::CursorLeft) => self.move_cursor_left(),
                        Some(PickerAction::CursorRight) => self.move_cursor_right(),
                        Some(PickerAction::MoveToLineStart) => self.move_to_start(),
                        Some(PickerAction::MoveToLineEnd) => self.move_to_end(),
                        Some(PickerAction::Noop) => {}
                        None => {
                            if let KeyCode::Char(c) = key.code {
                                self.update_filter(c)
                            }
                        }
                    }
                }
            }
        }
    }

    fn render(&mut self, f: &mut Frame) {
        let preview_direction;
        let picker_pane;
        let preview_pane;

        let preview_split = if self.preview_command.is_some() {
            preview_direction = if f.size().width.div_ceil(2) >= f.size().height {
                picker_pane = 0;
                preview_pane = 1;
                Direction::Horizontal
            } else {
                picker_pane = 1;
                preview_pane = 0;
                Direction::Vertical
            };
            Layout::new(
                preview_direction,
                [Constraint::Percentage(50), Constraint::Percentage(50)],
            )
            .split(f.size())
        } else {
            picker_pane = 0;
            preview_pane = 1;
            preview_direction = Direction::Horizontal;
            [f.size()].into()
        };

        let layout = Layout::new(
            Direction::Vertical,
            [
                Constraint::Length(preview_split[picker_pane].height - 1),
                Constraint::Length(1),
            ],
        )
        .split(preview_split[picker_pane]);

        self.matcher.tick(10);
        let snapshot = self.matcher.snapshot();
        let matches = snapshot
            .matched_items(..snapshot.matched_item_count())
            .map(|item| ListItem::new(item.data.as_str()));

        if let Some(selected) = self.selection.selected() {
            if snapshot.matched_item_count() == 0 {
                self.selection.select(None);
            } else if selected > snapshot.matched_item_count() as usize {
                self.selection
                    .select(Some(snapshot.matched_item_count() as usize - 1));
            }
        } else if snapshot.matched_item_count() > 0 {
            self.selection.select(Some(0));
        }

        let mut selected_style = Style::default()
            .bg(Color::LightBlue)
            .fg(Color::Black)
            .bold();
        let mut border_color = Color::DarkGray;
        let mut info_color = Color::LightYellow;
        let mut prompt_color = Color::LightGreen;

        if let Some(colors) = &self.colors {
            selected_style = colors.highlight_style().bold();

            if let Some(color) = colors.border_color() {
                border_color = color;
            }

            if let Some(color) = colors.info_color() {
                info_color = color;
            }

            if let Some(color) = colors.prompt_color() {
                prompt_color = color;
            }
        }

        let table = List::new(matches)
            .highlight_style(selected_style)
            .direction(ListDirection::BottomToTop)
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_symbol("> ")
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(border_color))
                    .title_style(Style::default().fg(info_color))
                    .title_position(Position::Bottom)
                    .title(format!(
                        "{}/{}",
                        snapshot.matched_item_count(),
                        snapshot.item_count()
                    )),
            );
        f.render_stateful_widget(table, layout[0], &mut self.selection);

        let prompt = Span::styled("> ", Style::default().fg(prompt_color));
        let input_text = Span::raw(&self.filter);
        let input_line = Line::from(vec![prompt, input_text]);
        let input = Paragraph::new(vec![input_line]);
        f.render_widget(input, layout[1]);
        f.set_cursor(layout[1].x + self.cursor_pos + 2, layout[1].y);

        if let Some(command) = &self.preview_command {
            self.render_preview(
                command,
                snapshot,
                f,
                &border_color,
                &preview_direction,
                preview_split[preview_pane],
            );
        }
    }

    fn render_preview(
        &self,
        command: &str,
        snapshot: &Snapshot<String>,
        f: &mut Frame,
        border_color: &Color,
        direction: &Direction,
        rect: Rect,
    ) {
        let text = if let Some(index) = self.selection.selected() {
            if let Some(item) = snapshot.get_matched_item(index as u32) {
                let command = command.replace("{}", item.data);
                let output = execute_tmux_command(&command);

                if output.status.success() {
                    String::from_utf8(output.stdout).unwrap()
                } else {
                    "".to_string()
                }
            } else {
                "".to_string()
            }
        } else {
            "".to_string()
        };
        let text = str_to_text(&text, (rect.width - 1).into());
        let border_position = if *direction == Direction::Horizontal {
            Borders::LEFT
        } else {
            Borders::BOTTOM
        };
        let preview = Paragraph::new(text)
            .block(
                Block::default()
                    .borders(border_position)
                    .border_style(Style::default().fg(*border_color)),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(preview, rect);
    }

    fn get_selected(&self) -> Option<String> {
        if let Some(index) = self.selection.selected() {
            return self
                .matcher
                .snapshot()
                .get_matched_item(index as u32)
                .map(|item| item.data.to_owned());
        }

        None
    }

    fn move_up(&mut self) {
        let item_count = self.matcher.snapshot().matched_item_count() as usize;
        let max = if item_count == 0 {
            return;
        } else {
            item_count - 1
        };
        match self.selection.selected() {
            Some(i) if i >= max => {}
            Some(i) => self.selection.select(Some(i + 1)),
            None => self.selection.select(Some(0)),
        }
    }

    fn move_down(&mut self) {
        match self.selection.selected() {
            Some(0) => {}
            Some(i) => self.selection.select(Some(i - 1)),
            None => self.selection.select(Some(0)),
        }
    }

    fn move_cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
        }
    }

    fn move_cursor_right(&mut self) {
        if self.cursor_pos < self.filter.len() as u16 {
            self.cursor_pos += 1;
        }
    }

    fn update_filter(&mut self, c: char) {
        if self.filter.len() == u16::MAX as usize {
            return;
        }

        let prev_filter = self.filter.clone();
        self.filter.insert(self.cursor_pos as usize, c);
        self.cursor_pos += 1;

        self.update_matcher_pattern(&prev_filter);
    }

    fn remove_filter(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }

        let prev_filter = self.filter.clone();
        self.filter.remove(self.cursor_pos as usize - 1);

        self.cursor_pos -= 1;

        if self.filter != prev_filter {
            self.update_matcher_pattern(&prev_filter);
        }
    }

    fn delete(&mut self) {
        if (self.cursor_pos as usize) == self.filter.len() {
            return;
        }

        let prev_filter = self.filter.clone();
        self.filter.remove(self.cursor_pos as usize);

        if self.filter != prev_filter {
            self.update_matcher_pattern(&prev_filter);
        }
    }

    fn update_matcher_pattern(&mut self, prev_filter: &str) {
        self.matcher.pattern.reparse(
            0,
            self.filter.as_str(),
            CaseMatching::Smart,
            Normalization::Smart,
            self.filter.starts_with(prev_filter),
        );
    }

    fn delete_word(&mut self) {
        let mut chars = self
            .filter
            .chars()
            .rev()
            .skip(self.filter.chars().count() - self.cursor_pos as usize);
        let length = std::cmp::min(
            u16::try_from(
                1 + chars.by_ref().take_while(|c| *c == ' ').count()
                    + chars.by_ref().take_while(|c| *c != ' ').count(),
            )
            .unwrap_or(self.cursor_pos),
            self.cursor_pos,
        );

        let prev_filter = self.filter.clone();
        let new_cursor_pos = self.cursor_pos - length;

        self.filter
            .drain((new_cursor_pos as usize)..(self.cursor_pos as usize));

        self.cursor_pos = new_cursor_pos;

        if self.filter != prev_filter {
            self.update_matcher_pattern(&prev_filter);
        }
    }

    fn delete_to_line(&mut self, forward: bool) {
        let prev_filter = self.filter.clone();

        if forward {
            self.filter.drain((self.cursor_pos as usize)..);
        } else {
            self.filter.drain(..(self.cursor_pos as usize));
            self.cursor_pos = 0;
        }

        if self.filter != prev_filter {
            self.update_matcher_pattern(&prev_filter);
        }
    }

    fn move_to_start(&mut self) {
        self.cursor_pos = 0;
    }

    fn move_to_end(&mut self) {
        self.cursor_pos = u16::try_from(self.filter.len()).unwrap_or_default();
    }
}

fn request_redraw() {}

fn str_to_text(s: &str, max: usize) -> Text<'_> {
    let mut text = Text::default();
    let mut style = Style::default();
    let mut tspan = String::new();
    let mut ansi_state;

    for l in s.lines() {
        let mut line = Line::default();
        ansi_state = false;

        for (i, ch) in l.chars().enumerate() {
            if !ansi_state {
                if ch == '\x1b' && l.chars().nth(i + 1) == Some('[') {
                    if !tspan.is_empty() {
                        let span = Span::styled(tspan.clone(), style);
                        line.spans.push(span);
                    }

                    tspan.clear();
                    ansi_state = true;
                } else {
                    tspan.push(ch);

                    if (line.width() + tspan.chars().count()) == max || i == (l.chars().count() - 1)
                    {
                        let span = Span::styled(tspan.clone(), style);
                        line.spans.push(span);
                        tspan.clear();
                        break;
                    }
                }
            } else {
                match ch {
                    '[' => {}
                    'm' => {
                        style = match tspan.as_str() {
                            "" => style.reset(),
                            "0" => style.reset(),
                            "1" => style.bold(),
                            "3" => style.italic(),
                            "4" => style.underlined(),
                            "5" => style.rapid_blink(),
                            "6" => style.slow_blink(),
                            "7" => style.reversed(),
                            "9" => style.crossed_out(),
                            "22" => style.not_bold(),
                            "23" => style.not_italic(),
                            "24" => style.not_underlined(),
                            "25" => style.not_rapid_blink().not_slow_blink(),
                            "27" => style.not_reversed(),
                            "29" => style.not_crossed_out(),
                            "30" => style.fg(Color::Black),
                            "31" => style.fg(Color::Red),
                            "32" => style.fg(Color::Green),
                            "33" => style.fg(Color::Yellow),
                            "34" => style.fg(Color::Blue),
                            "35" => style.fg(Color::Magenta),
                            "36" => style.fg(Color::Cyan),
                            "37" => style.fg(Color::Gray),
                            "40" => style.bg(Color::Black),
                            "41" => style.bg(Color::Red),
                            "42" => style.bg(Color::Green),
                            "43" => style.bg(Color::Yellow),
                            "44" => style.bg(Color::Blue),
                            "45" => style.bg(Color::Magenta),
                            "46" => style.bg(Color::Cyan),
                            "47" => style.bg(Color::Gray),
                            "90" => style.fg(Color::DarkGray),
                            "91" => style.fg(Color::LightRed),
                            "92" => style.fg(Color::LightGreen),
                            "93" => style.fg(Color::LightYellow),
                            "94" => style.fg(Color::LightBlue),
                            "95" => style.fg(Color::LightMagenta),
                            "96" => style.fg(Color::LightCyan),
                            "97" => style.fg(Color::White),
                            "100" => style.bg(Color::DarkGray),
                            "101" => style.bg(Color::LightRed),
                            "102" => style.bg(Color::LightGreen),
                            "103" => style.bg(Color::LightYellow),
                            "104" => style.bg(Color::LightBlue),
                            "105" => style.bg(Color::LightMagenta),
                            "106" => style.bg(Color::LightCyan),
                            "107" => style.bg(Color::White),
                            code => {
                                if let Some(colored) = Colored::parse_ansi(code) {
                                    match colored {
                                        Colored::ForegroundColor(c) => style.fg(c.into()),
                                        Colored::BackgroundColor(c) => style.bg(c.into()),
                                        Colored::UnderlineColor(c) => {
                                            style.underline_color(c.into())
                                        }
                                    }
                                } else {
                                    style
                                }
                            }
                        };

                        tspan.clear();
                        ansi_state = false;
                    }
                    _ => tspan.push(ch),
                }
            }
        }

        text.lines.push(line);
    }

    text
}
