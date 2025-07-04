mod preview;

use std::{process, rc::Rc, sync::Arc};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use nucleo::{
    pattern::{CaseMatching, Normalization},
    Nucleo,
};
use preview::PreviewWidget;
use ratatui::{
    layout::{self, Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span},
    widgets::{
        block::Position, Block, Borders, HighlightSpacing, List, ListDirection, ListItem,
        ListState, Paragraph,
    },
    DefaultTerminal, Frame,
};
use serde::{Deserialize, Serialize};

use crate::{
    configs::PickerColorConfig,
    keymap::{Keymap, PickerAction},
    tmux::Tmux,
    Result, TmsError,
};

pub enum Preview {
    SessionPane,
    WindowPane,
    Directory,
}

#[derive(Debug, Default, PartialEq, Eq, Deserialize, Serialize, Clone, Copy)]
pub enum InputPosition {
    Top,
    #[default]
    Bottom,
}

pub struct Picker<'a> {
    matcher: Nucleo<String>,
    preview: Option<Preview>,

    colors: Option<&'a PickerColorConfig>,

    selection: ListState,
    filter: String,
    cursor_pos: u16,
    keymap: Keymap,
    input_position: InputPosition,
    tmux: &'a Tmux,
}

impl<'a> Picker<'a> {
    pub fn new(
        list: &[String],
        preview: Option<Preview>,
        keymap: Option<&Keymap>,
        input_position: InputPosition,
        tmux: &'a Tmux,
    ) -> Self {
        let matcher = Nucleo::new(nucleo::Config::DEFAULT, Arc::new(request_redraw), None, 1);

        let injector = matcher.injector();

        for str in list {
            injector.push(str.to_owned(), |_, dst| dst[0] = str.to_owned().into());
        }

        let keymap = if let Some(keymap) = keymap {
            Keymap::with_defaults(keymap)
        } else {
            Keymap::default()
        };

        Picker {
            matcher,
            preview,
            colors: None,
            selection: ListState::default(),
            filter: String::default(),
            cursor_pos: 0,
            keymap,
            input_position,
            tmux,
        }
    }

    pub fn set_colors(mut self, colors: Option<&'a PickerColorConfig>) -> Self {
        self.colors = colors;

        self
    }

    pub fn run(&mut self) -> Result<Option<String>> {
        let mut terminal = ratatui::init();

        let selected_str = self
            .main_loop(&mut terminal)
            .map_err(|e| TmsError::TuiError(e.to_string()));

        ratatui::restore();

        Ok(selected_str?)
    }

    fn main_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<Option<String>> {
        loop {
            self.matcher.tick(10);
            self.update_selection();
            terminal
                .draw(|f| self.render(f))
                .map_err(|e| TmsError::TuiError(e.to_string()))?;

            if let Event::Key(key) = event::read().map_err(|e| TmsError::TuiError(e.to_string()))? {
                if key.kind == KeyEventKind::Press {
                    match self.keymap.0.get(&key.into()) {
                        Some(PickerAction::Cancel) => return Ok(None),
                        Some(PickerAction::Confirm) => {
                            if let Some(selected) = self.get_selected() {
                                return Ok(Some(selected.to_owned()));
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

    fn update_selection(&mut self) {
        let snapshot = self.matcher.snapshot();
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
    }

    fn render(&mut self, f: &mut Frame) {
        let preview_direction;
        let picker_pane;
        let preview_pane;
        let area = f.area();
        let mut input_position = self.input_position;

        let preview_split = if self.preview.is_some() {
            preview_direction = if area.width.div_ceil(2) >= area.height {
                picker_pane = 0;
                preview_pane = 1;
                Direction::Horizontal
            } else {
                picker_pane = 1;
                preview_pane = 0;
                input_position = InputPosition::Bottom;
                Direction::Vertical
            };
            Layout::new(
                preview_direction,
                [Constraint::Percentage(50), Constraint::Percentage(50)],
            )
            .split(area)
        } else {
            picker_pane = 0;
            preview_pane = 1;
            preview_direction = Direction::Horizontal;
            Rc::new([area])
        };

        let top_constraint;
        let bottom_constraint;
        let list_direction;
        let input_index;
        let list_index;
        let borders;
        let title_position;
        match input_position {
            InputPosition::Top => {
                top_constraint = Constraint::Length(1);
                bottom_constraint = Constraint::Length(preview_split[picker_pane].height - 1);
                list_direction = ListDirection::TopToBottom;
                input_index = 0;
                list_index = 1;
                borders = Borders::TOP;
                title_position = Position::Top;
            }
            InputPosition::Bottom => {
                top_constraint = Constraint::Length(preview_split[picker_pane].height - 1);
                bottom_constraint = Constraint::Length(1);
                list_direction = ListDirection::BottomToTop;
                input_index = 1;
                list_index = 0;
                borders = Borders::BOTTOM;
                title_position = Position::Bottom;
            }
        }
        let layout = Layout::new(Direction::Vertical, [top_constraint, bottom_constraint])
            .split(preview_split[picker_pane]);

        let snapshot = self.matcher.snapshot();
        let matches = snapshot
            .matched_items(..snapshot.matched_item_count())
            .map(|item| ListItem::new(item.data.as_str()));

        let colors = if let Some(colors) = self.colors {
            colors.to_owned()
        } else {
            PickerColorConfig::default_colors()
        };

        let table = List::new(matches)
            .highlight_style(colors.highlight_style())
            .direction(list_direction)
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_symbol("> ")
            .block(
                Block::default()
                    .borders(borders)
                    .border_style(Style::default().fg(colors.border_color()))
                    .title_style(Style::default().fg(colors.info_color()))
                    .title_position(title_position)
                    .title(format!(
                        "{}/{}",
                        snapshot.matched_item_count(),
                        snapshot.item_count()
                    )),
            );
        f.render_stateful_widget(table, layout[list_index], &mut self.selection);

        let prompt = Span::styled("> ", Style::default().fg(colors.prompt_color()));
        let input_text = Span::raw(&self.filter);
        let input_line = Line::from(vec![prompt, input_text]);
        let input = Paragraph::new(vec![input_line]);
        f.render_widget(input, layout[input_index]);
        f.set_cursor_position(layout::Position {
            x: layout[input_index].x + self.cursor_pos + 2,
            y: layout[input_index].y,
        });

        if self.preview.is_some() {
            let preview = PreviewWidget::new(
                self.get_preview_text(),
                colors.border_color(),
                preview_direction,
            );
            f.render_widget(preview, preview_split[preview_pane]);
        }
    }

    fn get_preview_text(&self) -> String {
        if let Some(item_data) = self.get_selected() {
            let output = match self.preview {
                Some(Preview::SessionPane) => self.tmux.capture_pane(item_data),
                Some(Preview::WindowPane) => self.tmux.capture_pane(
                    item_data
                        .split_once(' ')
                        .map(|val| val.0)
                        .unwrap_or_default(),
                ),
                Some(Preview::Directory) => process::Command::new("ls")
                    .args(["-1", item_data])
                    .output()
                    .unwrap_or_else(|_| {
                        panic!("Failed to execute the command for directory: {}", item_data)
                    }),
                None => panic!("preview rendering should not have occured"),
            };

            if output.status.success() {
                String::from_utf8(output.stdout).unwrap()
            } else {
                String::default()
            }
        } else {
            String::default()
        }
    }

    fn get_selected(&self) -> Option<&String> {
        if let Some(index) = self.selection.selected() {
            return self
                .matcher
                .snapshot()
                .get_matched_item(index as u32)
                .map(|item| item.data);
        }

        None
    }

    fn move_up(&mut self) {
        if self.input_position == InputPosition::Bottom {
            self.do_move_up()
        } else {
            self.do_move_down()
        }
    }

    fn move_down(&mut self) {
        if self.input_position == InputPosition::Bottom {
            self.do_move_down()
        } else {
            self.do_move_up()
        }
    }

    fn do_move_up(&mut self) {
        let item_count = self.matcher.snapshot().matched_item_count() as usize;
        if item_count == 0 {
            return;
        }

        let max = item_count - 1;

        match self.selection.selected() {
            Some(i) if i >= max => self.selection.select(Some(0)),
            Some(i) => self.selection.select(Some(i + 1)),
            None => self.selection.select(Some(0)),
        }
    }

    fn do_move_down(&mut self) {
        match self.selection.selected() {
            Some(0) => {
                let item_count = self.matcher.snapshot().matched_item_count() as usize;
                if item_count == 0 {
                    return;
                }
                self.selection.select(Some(item_count - 1))
            }
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
