use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use textwrap::wrap;

use crate::config::ResolvedRunConfig;
use crate::discovery::Repo;
use crate::state::{State, canonical_repo_key};

pub struct InteractiveSelection {
    pub selected_repos: Vec<PathBuf>,
    pub run_config: ResolvedRunConfig,
}

pub fn select_and_configure_run(
    repos: &[Repo],
    state: &mut State,
    base_run_config: &ResolvedRunConfig,
    persist_selection: bool,
) -> Result<Option<InteractiveSelection>> {
    let mut app = App::new(repos, state, base_run_config);
    let outcome = with_terminal(|terminal| run_ui(terminal, &mut app))?;
    let selection = match outcome {
        AppOutcome::Cancelled => return Ok(None),
        AppOutcome::Complete(selection) => selection,
    };

    if persist_selection {
        let selected_repo_keys: BTreeSet<String> = selection
            .selected_repos
            .iter()
            .map(|repo| canonical_repo_key(repo))
            .collect();

        for repo in repos {
            let key = canonical_repo_key(&repo.path);
            state
                .selected_repos
                .insert(key.clone(), selected_repo_keys.contains(&key));
        }
    }

    Ok(Some(selection))
}

fn with_terminal<T>(
    run: impl FnOnce(&mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<T>,
) -> Result<T> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let run_result = run(&mut terminal);

    let disable_raw_result = disable_raw_mode();
    let leave_screen_result = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let show_cursor_result = terminal.show_cursor();

    disable_raw_result?;
    leave_screen_result?;
    show_cursor_result?;
    run_result
}

fn run_ui(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<AppOutcome> {
    loop {
        terminal.draw(|frame| app.render(frame))?;

        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match app.handle_key(key.code) {
                        AppTransition::Continue => {}
                        AppTransition::Cancel => return Ok(AppOutcome::Cancelled),
                        AppTransition::Complete(selection) => {
                            return Ok(AppOutcome::Complete(selection));
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

enum AppOutcome {
    Cancelled,
    Complete(InteractiveSelection),
}

enum AppTransition {
    Continue,
    Cancel,
    Complete(InteractiveSelection),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Screen {
    RepoSelection,
    RunMode,
    IncludeUntracked,
    SideChannel,
}

struct RepoOption {
    path: PathBuf,
    label: String,
    selected: bool,
}

struct App {
    repos: Vec<RepoOption>,
    repo_cursor: usize,
    screen: Screen,
    run_mode_cursor: usize,
    include_untracked_cursor: usize,
    side_channel_cursor: usize,
    base_run_config: ResolvedRunConfig,
}

impl App {
    fn new(repos: &[Repo], state: &State, base_run_config: &ResolvedRunConfig) -> Self {
        let options = repos
            .iter()
            .map(|repo| RepoOption {
                path: repo.path.clone(),
                label: repo.path.display().to_string(),
                selected: state
                    .selected_repos
                    .get(&canonical_repo_key(&repo.path))
                    .copied()
                    .unwrap_or(true),
            })
            .collect();

        Self {
            repos: options,
            repo_cursor: 0,
            screen: Screen::RepoSelection,
            run_mode_cursor: if base_run_config.push_enabled { 0 } else { 1 },
            include_untracked_cursor: if base_run_config.include_untracked {
                0
            } else {
                1
            },
            side_channel_cursor: if base_run_config.side_channel.enabled {
                0
            } else {
                1
            },
            base_run_config: base_run_config.clone(),
        }
    }

    fn handle_key(&mut self, code: KeyCode) -> AppTransition {
        if matches!(code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q')) {
            return AppTransition::Cancel;
        }

        match self.screen {
            Screen::RepoSelection => self.handle_repo_key(code),
            Screen::RunMode => self.handle_run_mode_key(code),
            Screen::IncludeUntracked => self.handle_include_untracked_key(code),
            Screen::SideChannel => self.handle_side_channel_key(code),
        }
    }

    fn handle_repo_key(&mut self, code: KeyCode) -> AppTransition {
        if self.repos.is_empty() {
            return AppTransition::Continue;
        }

        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.repo_cursor > 0 {
                    self.repo_cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.repo_cursor + 1 < self.repos.len() {
                    self.repo_cursor += 1;
                }
            }
            KeyCode::Char(' ') => {
                if let Some(repo) = self.repos.get_mut(self.repo_cursor) {
                    repo.selected = !repo.selected;
                }
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                let select_all = self.repos.iter().any(|repo| !repo.selected);
                for repo in &mut self.repos {
                    repo.selected = select_all;
                }
            }
            KeyCode::Enter => {
                self.screen = Screen::RunMode;
            }
            _ => {}
        }

        AppTransition::Continue
    }

    fn handle_run_mode_key(&mut self, code: KeyCode) -> AppTransition {
        match code {
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Left | KeyCode::Char('h') => {
                if self.run_mode_cursor > 0 {
                    self.run_mode_cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Right | KeyCode::Char('l') => {
                if self.run_mode_cursor < 1 {
                    self.run_mode_cursor += 1;
                }
            }
            KeyCode::Enter => {
                if self.run_mode_cursor == 1 {
                    return AppTransition::Complete(self.build_selection(false));
                }
                self.screen = Screen::IncludeUntracked;
            }
            _ => {}
        }

        AppTransition::Continue
    }

    fn handle_include_untracked_key(&mut self, code: KeyCode) -> AppTransition {
        match code {
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Left | KeyCode::Char('h') => {
                if self.include_untracked_cursor > 0 {
                    self.include_untracked_cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Right | KeyCode::Char('l') => {
                if self.include_untracked_cursor < 1 {
                    self.include_untracked_cursor += 1;
                }
            }
            KeyCode::Enter => {
                self.screen = Screen::SideChannel;
            }
            _ => {}
        }

        AppTransition::Continue
    }

    fn handle_side_channel_key(&mut self, code: KeyCode) -> AppTransition {
        match code {
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Left | KeyCode::Char('h') => {
                if self.side_channel_cursor > 0 {
                    self.side_channel_cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Right | KeyCode::Char('l') => {
                if self.side_channel_cursor < 1 {
                    self.side_channel_cursor += 1;
                }
            }
            KeyCode::Enter => {
                return AppTransition::Complete(self.build_selection(true));
            }
            _ => {}
        }

        AppTransition::Continue
    }

    fn build_selection(&self, push_enabled: bool) -> InteractiveSelection {
        let mut run_config = self.base_run_config.clone();
        run_config.push_enabled = push_enabled;
        run_config.include_untracked = self.include_untracked_cursor == 0;
        run_config.side_channel.enabled = push_enabled && self.side_channel_cursor == 0;

        let selected_repos = self
            .repos
            .iter()
            .filter(|repo| repo.selected)
            .map(|repo| repo.path.clone())
            .collect();

        InteractiveSelection {
            selected_repos,
            run_config,
        }
    }

    fn render(&self, frame: &mut ratatui::Frame<'_>) {
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(4),
        ])
        .split(frame.area());

        frame.render_widget(
            Paragraph::new(vec![Line::from("Shephard Run Setup".bold())])
                .block(Block::default().borders(Borders::ALL)),
            chunks[0],
        );

        match self.screen {
            Screen::RepoSelection => self.render_repo_screen(frame, chunks[1]),
            Screen::RunMode => self.render_single_choice_screen(
                frame,
                chunks[1],
                "Run Mode",
                &["Sync all (pull + commit + push)", "Pull only"],
                self.run_mode_cursor,
            ),
            Screen::IncludeUntracked => self.render_single_choice_screen(
                frame,
                chunks[1],
                "Include Untracked Files?",
                &["Yes", "No"],
                self.include_untracked_cursor,
            ),
            Screen::SideChannel => self.render_single_choice_screen(
                frame,
                chunks[1],
                "Use Side-Channel Remote/Branch?",
                &["Yes", "No"],
                self.side_channel_cursor,
            ),
        }

        let help = self.help_text();
        let wrapped_help: Vec<Line<'_>> =
            wrap(help, usize::from(chunks[2].width.saturating_sub(2).max(1)))
                .into_iter()
                .map(|segment| Line::from(segment.into_owned()).dim())
                .collect();
        frame.render_widget(
            Paragraph::new(wrapped_help).block(Block::default().borders(Borders::ALL)),
            chunks[2],
        );
    }

    fn render_repo_screen(&self, frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect) {
        let items: Vec<ListItem<'_>> = self
            .repos
            .iter()
            .map(|repo| {
                let marker: Span<'_> = if repo.selected {
                    "[x]".green()
                } else {
                    "[ ]".dim()
                };
                ListItem::new(Line::from(vec![
                    marker,
                    " ".into(),
                    repo.label.clone().into(),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Repositories".bold()),
            )
            .highlight_style(Style::default().cyan())
            .highlight_symbol(">> ");

        let mut state = ListState::default();
        state.select(Some(self.repo_cursor));
        frame.render_stateful_widget(list, area, &mut state);
    }

    fn render_single_choice_screen(
        &self,
        frame: &mut ratatui::Frame<'_>,
        area: ratatui::layout::Rect,
        title: &str,
        options: &[&str],
        selected: usize,
    ) {
        let items: Vec<ListItem<'_>> = options
            .iter()
            .map(|option| ListItem::new(*option))
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title.bold()))
            .highlight_style(Style::default().cyan())
            .highlight_symbol(">> ");

        let mut state = ListState::default();
        state.select(Some(selected));
        frame.render_stateful_widget(list, area, &mut state);
    }

    fn help_text(&self) -> &'static str {
        match self.screen {
            Screen::RepoSelection => {
                "↑/↓ or j/k: move   space: toggle   a: toggle all   enter: continue   q/esc: cancel"
            }
            Screen::RunMode | Screen::IncludeUntracked | Screen::SideChannel => {
                "↑/↓ or j/k: move   enter: confirm   q/esc: cancel"
            }
        }
    }
}
