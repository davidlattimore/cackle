use super::FixOutcome;
use crate::config_editor;
use crate::config_editor::ConfigEditor;
use crate::config_editor::Edit;
use crate::problem::Problems;
use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::List;
use ratatui::widgets::ListItem;
use ratatui::widgets::ListState;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use ratatui::Frame;
use ratatui::Terminal;
use std::collections::VecDeque;
use std::io::Stdout;
use std::path::PathBuf;

mod edit_config_ui;

pub(crate) struct FullTermUi {
    config_path: PathBuf,
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl FullTermUi {
    pub(crate) fn new(config_path: PathBuf) -> Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
        let backend = ratatui::backend::CrosstermBackend::new(stdout);
        let terminal = ratatui::Terminal::new(backend)?;
        Ok(Self {
            config_path,
            terminal,
        })
    }
}

impl super::Ui for FullTermUi {
    fn maybe_fix_problems(&mut self, problems: &Problems) -> anyhow::Result<FixOutcome> {
        let problems = problems.clone().grouped_by_type_crate_and_api();
        let mut problems_ui = ProblemsUi::new(problems, self.config_path.clone());
        problems_ui.run(&mut self.terminal)?;
        Ok(match problems_ui.mode {
            Mode::Continue => FixOutcome::Retry,
            _ => FixOutcome::GiveUp,
        })
    }

    fn create_initial_config(&mut self) -> anyhow::Result<()> {
        edit_config_ui::EditConfigUi::new(self.config_path.clone()).run(&mut self.terminal)
    }
}

trait Screen {
    type ExitStatus;

    fn render(&self, f: &mut Frame<CrosstermBackend<Stdout>>) -> Result<()>;
    fn handle_key(&mut self, key: KeyEvent) -> Result<()>;
    fn exit_status(&self) -> Option<Self::ExitStatus>;

    fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ) -> Result<Self::ExitStatus> {
        let mut error = None;
        loop {
            if let Some(exit_status) = self.exit_status() {
                return Ok(exit_status);
            }
            terminal.draw(|f| {
                if let Err(e) = self.render(f) {
                    error = Some(e);
                }
                if let Some(e) = error.as_ref() {
                    render_error(f, e);
                }
            })?;
            if let Event::Key(key) = crossterm::event::read()? {
                // When we're displaying an error, any key will dismiss the error popup. They key
                // should then be ignored.
                if error.take().is_some() {
                    continue;
                }
                if let Err(e) = self.handle_key(key) {
                    error = Some(e);
                }
            }
        }
    }
}

impl Drop for FullTermUi {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            self.terminal.backend_mut(),
            crossterm::terminal::LeaveAlternateScreen
        );
    }
}

struct ProblemsUi {
    problems: Problems,
    mode: Mode,
    problem_index: usize,
    edit_index: usize,
    config_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    SelectProblem,
    SelectEdit,
    Quit,
    Continue,
}

impl Screen for ProblemsUi {
    type ExitStatus = FixOutcome;

    fn exit_status(&self) -> Option<Self::ExitStatus> {
        match self.mode {
            Mode::Quit => Some(FixOutcome::GiveUp),
            Mode::Continue => Some(FixOutcome::Retry),
            _ => None,
        }
    }

    fn render(&self, f: &mut Frame<CrosstermBackend<Stdout>>) -> Result<()> {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .margin(1)
            .constraints(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(f.size());

        let (top_left, bottom_left) = split_vertical(horizontal[0]);

        self.render_problems(f, top_left);
        self.render_details(f, bottom_left);

        match self.mode {
            Mode::SelectProblem => {}
            Mode::SelectEdit => self.render_edits_and_diff(f, horizontal[1])?,
            Mode::Quit | Mode::Continue => {}
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        match (self.mode, key.code) {
            (_, KeyCode::Char('q')) => self.mode = Mode::Quit,
            (Mode::SelectProblem, KeyCode::Up | KeyCode::Down) => {
                update_counter(&mut self.problem_index, key.code, self.problems.len());
            }
            (Mode::SelectEdit, KeyCode::Up | KeyCode::Down) => {
                let num_edits = self.edits().len();
                update_counter(&mut self.edit_index, key.code, num_edits);
            }
            (Mode::SelectProblem, KeyCode::Char(' ')) => {
                self.mode = Mode::SelectEdit;
                self.edit_index = 0;
            }
            (Mode::SelectEdit, KeyCode::Char(' ')) => {
                self.apply_selected_edit()?;
                self.problems.remove(self.problem_index);
                if self.problem_index >= self.problems.len() {
                    self.problem_index = 0;
                }
                if self.problems.is_empty() {
                    self.mode = Mode::Continue;
                } else {
                    self.mode = Mode::SelectProblem;
                }
            }
            (_, KeyCode::Esc) => self.mode = Mode::SelectProblem,
            _ => {}
        }
        Ok(())
    }
}

impl ProblemsUi {
    fn new(problems: Problems, config_path: PathBuf) -> Self {
        Self {
            problems,
            mode: Mode::SelectProblem,
            problem_index: 0,
            edit_index: 0,
            config_path,
        }
    }

    fn render_problems(&self, f: &mut Frame<CrosstermBackend<Stdout>>, area: Rect) {
        let items = self
            .problems
            .into_iter()
            .map(|problem| ListItem::new(problem.short_description()));
        render_list(
            f,
            "Problems",
            items,
            self.mode == Mode::SelectProblem,
            area,
            self.problem_index,
        );
    }

    fn render_details(&self, f: &mut Frame<CrosstermBackend<Stdout>>, area: Rect) {
        let block = Block::default().title("Details").borders(Borders::ALL);
        let paragraph = Paragraph::new(self.problems[self.problem_index].details())
            .block(block)
            .wrap(Wrap { trim: false });
        f.render_widget(paragraph, area);
    }

    fn render_edits_and_diff(
        &self,
        f: &mut Frame<CrosstermBackend<Stdout>>,
        area: Rect,
    ) -> Result<()> {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let edits = self.edits();
        self.render_edit_selector(&edits, f, chunks[0]);
        self.render_diff(&edits, f, chunks[1])?;
        Ok(())
    }

    fn edits(&self) -> Vec<Box<dyn Edit>> {
        let problem = &self.problems[self.problem_index];
        config_editor::fixes_for_problem(problem)
    }

    fn render_edit_selector(
        &self,
        edits: &[Box<dyn Edit>],
        f: &mut Frame<CrosstermBackend<Stdout>>,
        area: Rect,
    ) {
        let items = edits.iter().map(|fix| ListItem::new(fix.title()));
        render_list(
            f,
            "Edits",
            items,
            self.mode == Mode::SelectEdit,
            area,
            self.edit_index,
        );
    }

    fn render_diff(
        &self,
        edits: &[Box<dyn Edit>],
        f: &mut Frame<CrosstermBackend<Stdout>>,
        area: Rect,
    ) -> Result<()> {
        let Some(edit) = edits.get(self.edit_index) else {
            return Ok(());
        };

        let mut editor = ConfigEditor::from_file(&self.config_path)?;
        edit.apply(&mut editor)?;
        let original = std::fs::read_to_string(&self.config_path)?;
        let updated = editor.to_toml();

        const CONTEXT: usize = 2;
        let mut common = VecDeque::new();
        let mut after_context = 0;
        let mut lines = Vec::new();
        for diff in diff::lines(&original, &updated) {
            match diff {
                diff::Result::Both(s, _) => {
                    if after_context > 0 {
                        after_context -= 1;
                        lines.push(Line::from(format!(" {s}")));
                    } else {
                        common.push_back(s);
                        if common.len() > CONTEXT {
                            common.pop_front();
                        }
                    }
                }
                diff::Result::Left(s) => {
                    {
                        let common: &mut VecDeque<&str> = &mut common;
                        for line in common.drain(..) {
                            lines.push(Line::from(format!(" {line}")));
                        }
                    };
                    lines.push(Line::from(vec![Span::styled(
                        format!("-{s}"),
                        Style::default().fg(Color::Red),
                    )]));
                    after_context = CONTEXT;
                }
                diff::Result::Right(s) => {
                    {
                        let common: &mut VecDeque<&str> = &mut common;
                        for line in common.drain(..) {
                            lines.push(Line::from(format!(" {line}")));
                        }
                    };
                    lines.push(Line::from(vec![Span::styled(
                        format!("+{s}"),
                        Style::default().fg(Color::Green),
                    )]));
                    after_context = CONTEXT;
                }
            }
        }

        let block = Block::default().title("Config diff").borders(Borders::ALL);
        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        f.render_widget(paragraph, area);
        Ok(())
    }

    fn apply_selected_edit(&self) -> Result<()> {
        let edits = &self.edits();
        let edit = edits
            .get(self.edit_index)
            .ok_or_else(|| anyhow!("Selected edit out of range"))?;
        let mut editor = ConfigEditor::from_file(&self.config_path)?;
        edit.apply(&mut editor)?;
        std::fs::write(&self.config_path, editor.to_toml())
            .with_context(|| format!("Failed to write `{}`", self.config_path.display()))
    }
}

fn split_vertical(area: Rect) -> (Rect, Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    (chunks[0], chunks[1])
}

fn render_error(f: &mut Frame<CrosstermBackend<Stdout>>, error: &anyhow::Error) {
    let vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![
            Constraint::Percentage(25),
            Constraint::Percentage(50),
            Constraint::Percentage(25),
        ])
        .split(f.size());

    let horizontal_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
        ])
        .split(vertical_chunks[1]);
    let area = horizontal_chunks[1];

    let block = Block::default()
        .title("Error")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));
    let paragraph = Paragraph::new(format!("{error:#}"))
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(Clear, area);
    f.render_widget(paragraph, area);
}

fn render_list(
    f: &mut Frame<CrosstermBackend<Stdout>>,
    title: &str,
    items: impl Iterator<Item = ListItem<'static>>,
    active: bool,
    area: Rect,
    index: usize,
) {
    let items: Vec<_> = items.collect();
    let mut block = Block::default().title(title).borders(Borders::ALL);
    if active {
        block = block
            .border_type(BorderType::Thick)
            .border_style(Style::default().fg(Color::Yellow));
    }
    let mut style = Style::default().add_modifier(Modifier::REVERSED);
    if active {
        style = style.fg(Color::Yellow);
    }
    let list = List::new(items).block(block).highlight_style(style);
    let mut list_state = ListState::default();
    list_state.select(Some(index));
    f.render_stateful_widget(list, area, &mut list_state);
}

/// Increment or decrement `counter`, wrapping at `len`. `keycode` must be Down or Up.
fn update_counter(counter: &mut usize, key_code: KeyCode, len: usize) {
    match key_code {
        KeyCode::Up => *counter = (*counter + len - 1) % len,
        KeyCode::Down => *counter = (*counter + len + 1) % len,
        _ => panic!("Invalid call to update_counter"),
    }
}
