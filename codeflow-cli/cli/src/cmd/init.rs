//! Init command: 7-step ratatui onboarding wizard for `CodeFlow` projects.
//!
//! Steps:
//! 1. Project location detect (new/existing/join)
//! 2. Prerequisites check (git, Claude Code)
//! 3. Claude Code authentication
//! 4. Git provider setup
//! 5. Project configuration (name, description)
//! 6. CodeFlow setup (directories, optional PathFlow)
//! 7. Verification (health checks)

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use codeflow_core::tui::theme;
use codeflow_core::tui::widgets::{
    CheckItem, CheckStatus, Checklist, SelectOption, SelectionList, StepEntry, StepProgress,
    StepStatus, TextInput,
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// CLI flags for `codeflow init`.
#[derive(Debug, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)]
pub struct InitFlags {
    /// Skip location detection, assume existing project.
    pub existing: bool,
    /// Join an existing CodeFlow project.
    pub join: bool,
    /// Skip authentication steps (3 and 4).
    pub skip_auth: bool,
    /// Non-interactive: accept all defaults.
    pub yes: bool,
}

/// Entry point for `codeflow init`.
pub fn run(flags: InitFlags) -> Result<()> {
    if !std::io::stdout().is_terminal() || flags.yes {
        return run_non_interactive(flags);
    }
    run_tui_wizard(flags)
}

// ---------------------------------------------------------------------------
// CommandRunner trait for injectable dependencies
// ---------------------------------------------------------------------------

/// Abstracts external command execution for testability.
pub trait CommandRunner: Send + Sync {
    /// Check if a command is available on PATH and return its version string.
    fn check_command(&self, name: &str, version_flag: &str) -> Option<String>;

    /// Run an external command (e.g., `claude login`) and return its exit status.
    ///
    /// Used during auth and git provider setup steps when running interactive
    /// commands like `claude login` or `gh auth login`. Reserved for future
    /// interactive command execution within wizard steps.
    #[allow(dead_code)]
    fn run_command(&self, name: &str, args: &[&str]) -> Result<bool>;

    /// Get the current working directory.
    fn current_dir(&self) -> Result<PathBuf>;
}

/// Default implementation that calls real system commands.
struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn check_command(&self, name: &str, version_flag: &str) -> Option<String> {
        std::process::Command::new(name)
            .arg(version_flag)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let stderr = String::from_utf8_lossy(&o.stderr);
                let version = stdout.trim().to_string();
                if version.is_empty() {
                    let v = stderr.trim().to_string();
                    if v.is_empty() { None } else { Some(v) }
                } else {
                    Some(version)
                }
            })
    }

    fn run_command(&self, name: &str, args: &[&str]) -> Result<bool> {
        let status = std::process::Command::new(name)
            .args(args)
            .status()
            .with_context(|| format!("running {name}"))?;
        Ok(status.success())
    }

    fn current_dir(&self) -> Result<PathBuf> {
        std::env::current_dir().context("getting current directory")
    }
}

// ---------------------------------------------------------------------------
// Step result
// ---------------------------------------------------------------------------

/// Result of processing a single wizard step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepResult {
    /// Advance to next step.
    Advance,
    /// Stay on current step (e.g., typing in text field).
    Stay,
    /// Go back to previous step.
    Back,
    /// User cancelled the wizard.
    Quit,
}

// ---------------------------------------------------------------------------
// Project location detection
// ---------------------------------------------------------------------------

/// Detected project type for step 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectType {
    New,
    Existing,
    Join,
}

// ---------------------------------------------------------------------------
// Wizard state
// ---------------------------------------------------------------------------

/// Holds all mutable state for the wizard.
pub struct WizardState {
    /// Current step (0-based index, 0..7).
    pub current_step: usize,
    /// Project type selected in step 1.
    pub project_type: ProjectType,
    /// Selection index for step 1.
    pub location_selection: usize,
    /// Prerequisites check results.
    pub prereqs: Vec<CheckItem>,
    /// Auth method selection (0=subscription, 1=API key).
    pub auth_selection: usize,
    /// Git provider selection (0=GitHub, 1=GitLab, 2=Bitbucket).
    pub git_provider_selection: usize,
    /// Project name text input.
    pub project_name: String,
    /// Project name cursor position.
    pub project_name_cursor: usize,
    /// Project description text input.
    pub project_description: String,
    /// Project description cursor position.
    pub project_description_cursor: usize,
    /// Which text field is focused in step 5 (0=name, 1=description).
    pub config_field_focus: usize,
    /// CodeFlow setup options (PathFlow enabled).
    pub pathflow_enabled: bool,
    /// Setup selection index.
    pub setup_selection: usize,
    /// Verification results.
    pub verification: Vec<CheckItem>,
    /// Project directory path.
    pub project_dir: PathBuf,
    /// Directories created during setup (for cleanup on cancel).
    pub created_dirs: Vec<PathBuf>,
    /// Whether auth steps are skipped.
    pub skip_auth: bool,
    /// Whether step 1 was pre-determined by flags.
    pub location_preset: bool,
    /// Transient hint message shown in the keybinding bar.
    pub hint_message: Option<String>,
}

impl WizardState {
    fn new(flags: InitFlags, runner: &dyn CommandRunner) -> Result<Self> {
        let project_dir = runner.current_dir()?;
        let project_type = if flags.join {
            ProjectType::Join
        } else if flags.existing {
            ProjectType::Existing
        } else {
            detect_project_type(&project_dir)
        };
        let location_preset = flags.existing || flags.join;

        let project_name = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("my-project")
            .to_string();

        Ok(Self {
            current_step: 0,
            project_type,
            location_selection: match project_type {
                ProjectType::New => 0,
                ProjectType::Existing => 1,
                ProjectType::Join => 2,
            },
            prereqs: vec![
                CheckItem {
                    label: "git".to_string(),
                    status: CheckStatus::Pending,
                    detail: None,
                },
                CheckItem {
                    label: "Claude Code (claude)".to_string(),
                    status: CheckStatus::Pending,
                    detail: None,
                },
            ],
            auth_selection: 0,
            git_provider_selection: 0,
            project_name_cursor: project_name.len(),
            project_name,
            project_description: String::new(),
            project_description_cursor: 0,
            config_field_focus: 0,
            pathflow_enabled: false,
            setup_selection: 0,
            verification: Vec::new(),
            project_dir,
            created_dirs: Vec::new(),
            skip_auth: flags.skip_auth,
            location_preset,
            hint_message: None,
        })
    }

    fn step_entries(&self) -> Vec<StepEntry> {
        let labels = [
            "Location",
            "Prerequisites",
            "Authentication",
            "Git Provider",
            "Configuration",
            "Setup",
            "Verification",
        ];
        labels
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let status = match i.cmp(&self.current_step) {
                    std::cmp::Ordering::Less => {
                        if (i == 2 || i == 3) && self.skip_auth {
                            StepStatus::Skipped
                        } else {
                            StepStatus::Complete
                        }
                    }
                    std::cmp::Ordering::Equal => StepStatus::Current,
                    std::cmp::Ordering::Greater => StepStatus::Pending,
                };
                StepEntry {
                    label: label.to_string(),
                    status,
                }
            })
            .collect()
    }
}

fn detect_project_type(dir: &Path) -> ProjectType {
    let has_codeflow = dir.join(".codeflow").is_dir();
    let has_state = dir.join(".state").is_dir();
    if has_codeflow && has_state {
        ProjectType::Existing
    } else if has_codeflow {
        // .codeflow exists but no .state -- likely a cloned repo
        ProjectType::Join
    } else {
        ProjectType::New
    }
}

// ---------------------------------------------------------------------------
// TUI wizard
// ---------------------------------------------------------------------------

fn run_tui_wizard(flags: InitFlags) -> Result<()> {
    let runner = RealCommandRunner;
    run_tui_wizard_with_runner(flags, &runner)
}

fn run_tui_wizard_with_runner(flags: InitFlags, runner: &dyn CommandRunner) -> Result<()> {
    let mut state = WizardState::new(flags, runner)?;

    let mut terminal = ratatui::init();

    // Cleanup guard: ensure terminal is restored on any exit path.
    struct TermGuard;
    impl Drop for TermGuard {
        fn drop(&mut self) {
            ratatui::restore();
        }
    }
    let term_guard = TermGuard;

    // If location was preset, auto-advance past step 1.
    if state.location_preset {
        state.current_step = 1;
    }

    loop {
        // Auto-skip auth steps when --skip-auth is set.
        if state.skip_auth && (state.current_step == 2 || state.current_step == 3) {
            state.current_step = 4;
        }

        // Run prerequisite checks when entering step 1.
        if state.current_step == 1 {
            check_prerequisites(&mut state, runner);
        }

        // Run verification when entering step 6.
        if state.current_step == 6 {
            run_verification_checks(&mut state, runner);
        }

        terminal.draw(|frame| {
            let area = frame.area();

            // Two-panel layout: left sidebar (step progress) + right content.
            let panels =
                Layout::horizontal([Constraint::Length(24), Constraint::Min(40)]).split(area);

            // Left panel: step progress.
            let step_entries = state.step_entries();
            let progress = StepProgress::new(&step_entries, "Steps");
            frame.render_widget(progress, panels[0]);

            // Right panel: step content + keybinding bar.
            let content_chunks =
                Layout::vertical([Constraint::Min(5), Constraint::Length(1)]).split(panels[1]);

            render_step_content(frame, content_chunks[0], &state);
            render_keybinding_bar(frame, content_chunks[1], &state);
        })?;

        // Handle input.
        let tick = std::time::Duration::from_millis(100);
        if event::poll(tick)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Ctrl+C: cleanup and exit.
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    cleanup_on_cancel(&state);
                    return Ok(());
                }

                let result = handle_step_input(&mut state, key.code, runner);
                match result {
                    StepResult::Advance => {
                        if state.current_step < 6 {
                            // Run setup when advancing from step 5 to step 6.
                            if state.current_step == 5 {
                                run_setup(&mut state)?;
                            }
                            state.current_step += 1;
                        } else {
                            // Final step completed.
                            break;
                        }
                    }
                    StepResult::Stay => {
                        // Remain on the current step.
                    }
                    StepResult::Back => {
                        if state.current_step > 0 {
                            state.current_step -= 1;
                            // Skip auth steps backward too.
                            if state.skip_auth
                                && (state.current_step == 3 || state.current_step == 2)
                            {
                                state.current_step = 1;
                            }
                        }
                    }
                    StepResult::Quit => {
                        cleanup_on_cancel(&state);
                        return Ok(());
                    }
                }
            }
        }
    }

    // Success message after wizard completes.
    drop(term_guard);
    println!(
        "{} CodeFlow initialized at {}",
        theme::CHECKMARK,
        state.project_dir.display()
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Step content rendering
// ---------------------------------------------------------------------------

fn render_step_content(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &WizardState,
) {
    match state.current_step {
        0 => render_step_location(frame, area, state),
        1 => render_step_prerequisites(frame, area, state),
        2 => render_step_auth(frame, area, state),
        3 => render_step_git_provider(frame, area, state),
        4 => render_step_config(frame, area, state),
        5 => render_step_setup(frame, area, state),
        6 => render_step_verification(frame, area, state),
        _ => {}
    }
}

fn render_step_location(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &WizardState,
) {
    let options = vec![
        SelectOption {
            label: "New project".to_string(),
            description: Some("Initialize CodeFlow in a new directory".to_string()),
        },
        SelectOption {
            label: "Existing project".to_string(),
            description: Some("Add CodeFlow to an existing project".to_string()),
        },
        SelectOption {
            label: "Join project".to_string(),
            description: Some("Join a cloned CodeFlow project".to_string()),
        },
    ];

    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(5)]).split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(" Step 1: ", theme::header()),
        Span::raw("Project Location"),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_type(theme::BORDER_TYPE),
    );
    frame.render_widget(header, chunks[0]);

    let list = SelectionList::new(&options, state.location_selection, "Select project type");
    frame.render_widget(list, chunks[1]);
}

fn render_step_prerequisites(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &WizardState,
) {
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(5)]).split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(" Step 2: ", theme::header()),
        Span::raw("Prerequisites Check"),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_type(theme::BORDER_TYPE),
    );
    frame.render_widget(header, chunks[0]);

    let checklist = Checklist::new(&state.prereqs, "Required Tools");
    frame.render_widget(checklist, chunks[1]);
}

fn render_step_auth(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &WizardState) {
    let options = vec![
        SelectOption {
            label: "Subscription".to_string(),
            description: Some("Use your Claude Pro/Team subscription".to_string()),
        },
        SelectOption {
            label: "API Key".to_string(),
            description: Some("Use an Anthropic API key".to_string()),
        },
    ];

    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(5)]).split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(" Step 3: ", theme::header()),
        Span::raw("Claude Code Authentication"),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_type(theme::BORDER_TYPE),
    );
    frame.render_widget(header, chunks[0]);

    let list = SelectionList::new(&options, state.auth_selection, "Authentication method");
    frame.render_widget(list, chunks[1]);
}

fn render_step_git_provider(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &WizardState,
) {
    let options = vec![
        SelectOption {
            label: "GitHub".to_string(),
            description: Some("Installs gh CLI, runs gh auth login".to_string()),
        },
        SelectOption {
            label: "GitLab".to_string(),
            description: Some("Configure GitLab remote".to_string()),
        },
        SelectOption {
            label: "Bitbucket".to_string(),
            description: Some("Configure Bitbucket remote".to_string()),
        },
    ];

    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(5)]).split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(" Step 4: ", theme::header()),
        Span::raw("Git Provider Setup"),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_type(theme::BORDER_TYPE),
    );
    frame.render_widget(header, chunks[0]);

    let list = SelectionList::new(&options, state.git_provider_selection, "Select provider");
    frame.render_widget(list, chunks[1]);
}

fn render_step_config(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &WizardState,
) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(1),
    ])
    .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(" Step 5: ", theme::header()),
        Span::raw("Project Configuration"),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_type(theme::BORDER_TYPE),
    );
    frame.render_widget(header, chunks[0]);

    let name_input = TextInput::new(&state.project_name, "Project Name")
        .placeholder("my-project")
        .cursor(state.project_name_cursor)
        .focused(state.config_field_focus == 0);
    frame.render_widget(name_input, chunks[1]);

    let desc_input = TextInput::new(&state.project_description, "Description")
        .placeholder("A CodeFlow project")
        .cursor(state.project_description_cursor)
        .focused(state.config_field_focus == 1);
    frame.render_widget(desc_input, chunks[2]);

    let hint = Paragraph::new(Line::from(vec![
        Span::styled("  Tab", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" to switch fields, "),
        Span::styled("Enter", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" to continue"),
    ]));
    frame.render_widget(hint, chunks[3]);
}

fn render_step_setup(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &WizardState) {
    let options = vec![
        SelectOption {
            label: "Standard setup".to_string(),
            description: Some("Create directories and initialize database".to_string()),
        },
        SelectOption {
            label: "Standard + PathFlow".to_string(),
            description: Some("Also enable PathFlow enforcement (recommended)".to_string()),
        },
    ];

    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(5)]).split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(" Step 6: ", theme::header()),
        Span::raw("CodeFlow Setup"),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_type(theme::BORDER_TYPE),
    );
    frame.render_widget(header, chunks[0]);

    let list = SelectionList::new(&options, state.setup_selection, "Setup mode");
    frame.render_widget(list, chunks[1]);
}

fn render_step_verification(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &WizardState,
) {
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(5)]).split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(" Step 7: ", theme::header()),
        Span::raw("Verification"),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_type(theme::BORDER_TYPE),
    );
    frame.render_widget(header, chunks[0]);

    let checklist = Checklist::new(&state.verification, "Health Checks");
    frame.render_widget(checklist, chunks[1]);
}

fn render_keybinding_bar(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &WizardState,
) {
    let mut spans = vec![
        Span::styled(" [Enter]", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(if state.current_step == 6 {
            " Finish "
        } else {
            " Next "
        }),
    ];

    if state.current_step > 0 {
        spans.push(Span::styled(
            "[Backspace]",
            Style::new().fg(theme::BLUE_ACCENT),
        ));
        spans.push(Span::raw(" Back "));
    }

    if state.current_step == 0
        || state.current_step == 2
        || state.current_step == 3
        || state.current_step == 5
    {
        spans.push(Span::styled(
            "[Up/Down]",
            Style::new().fg(theme::BLUE_ACCENT),
        ));
        spans.push(Span::raw(" Navigate "));
    }

    spans.push(Span::styled(
        "[Ctrl+C]",
        Style::new().fg(theme::BLUE_ACCENT),
    ));
    spans.push(Span::raw(" Quit"));

    // Show transient hint message if present.
    if let Some(ref hint) = state.hint_message {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            hint.clone(),
            Style::new().fg(theme::YELLOW_RUNNING),
        ));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ---------------------------------------------------------------------------
// Step input handling
// ---------------------------------------------------------------------------

fn handle_step_input(
    state: &mut WizardState,
    key: KeyCode,
    runner: &dyn CommandRunner,
) -> StepResult {
    match key {
        KeyCode::Esc => return StepResult::Quit,
        KeyCode::Backspace if state.current_step == 4 => {
            // In text input mode, backspace deletes character.
            handle_text_backspace(state);
            return StepResult::Stay;
        }
        _ => {}
    }

    match state.current_step {
        0 => handle_location_input(state, key),
        1 => handle_prereqs_input(state, key),
        2 => handle_auth_input(state, key, runner),
        3 => handle_git_provider_input(state, key, runner),
        4 => handle_config_input(state, key),
        5 => handle_setup_input(state, key),
        6 => handle_verification_input(key),
        _ => StepResult::Stay,
    }
}

fn handle_location_input(state: &mut WizardState, key: KeyCode) -> StepResult {
    match key {
        KeyCode::Up => {
            state.location_selection = state.location_selection.saturating_sub(1);
            StepResult::Stay
        }
        KeyCode::Down => {
            if state.location_selection < 2 {
                state.location_selection += 1;
            }
            StepResult::Stay
        }
        KeyCode::Enter => {
            state.project_type = match state.location_selection {
                0 => ProjectType::New,
                1 => ProjectType::Existing,
                _ => ProjectType::Join,
            };
            StepResult::Advance
        }
        _ => StepResult::Stay,
    }
}

fn handle_prereqs_input(state: &mut WizardState, key: KeyCode) -> StepResult {
    match key {
        KeyCode::Enter => {
            let all_pass = state.prereqs.iter().all(|p| p.status == CheckStatus::Pass);
            if all_pass {
                state.hint_message = None;
                StepResult::Advance
            } else {
                state.hint_message = Some("Fix prerequisites before continuing".to_string());
                StepResult::Stay
            }
        }
        KeyCode::Backspace => StepResult::Back,
        _ => {
            state.hint_message = None;
            StepResult::Stay
        }
    }
}

fn handle_auth_input(
    state: &mut WizardState,
    key: KeyCode,
    _runner: &dyn CommandRunner,
) -> StepResult {
    match key {
        KeyCode::Up => {
            state.auth_selection = state.auth_selection.saturating_sub(1);
            StepResult::Stay
        }
        KeyCode::Down => {
            if state.auth_selection < 1 {
                state.auth_selection += 1;
            }
            StepResult::Stay
        }
        KeyCode::Enter => StepResult::Advance,
        KeyCode::Backspace => StepResult::Back,
        _ => StepResult::Stay,
    }
}

fn handle_git_provider_input(
    state: &mut WizardState,
    key: KeyCode,
    _runner: &dyn CommandRunner,
) -> StepResult {
    match key {
        KeyCode::Up => {
            state.git_provider_selection = state.git_provider_selection.saturating_sub(1);
            StepResult::Stay
        }
        KeyCode::Down => {
            if state.git_provider_selection < 2 {
                state.git_provider_selection += 1;
            }
            StepResult::Stay
        }
        KeyCode::Enter => StepResult::Advance,
        KeyCode::Backspace => StepResult::Back,
        _ => StepResult::Stay,
    }
}

fn handle_config_input(state: &mut WizardState, key: KeyCode) -> StepResult {
    match key {
        KeyCode::Tab => {
            state.config_field_focus = (state.config_field_focus + 1) % 2;
            StepResult::Stay
        }
        KeyCode::Enter => {
            if state.project_name.is_empty() {
                StepResult::Stay
            } else {
                StepResult::Advance
            }
        }
        KeyCode::Char(c) => {
            handle_text_char(state, c);
            StepResult::Stay
        }
        KeyCode::Left => {
            handle_cursor_left(state);
            StepResult::Stay
        }
        KeyCode::Right => {
            handle_cursor_right(state);
            StepResult::Stay
        }
        _ => StepResult::Stay,
    }
}

fn handle_setup_input(state: &mut WizardState, key: KeyCode) -> StepResult {
    match key {
        KeyCode::Up => {
            state.setup_selection = state.setup_selection.saturating_sub(1);
            StepResult::Stay
        }
        KeyCode::Down => {
            if state.setup_selection < 1 {
                state.setup_selection += 1;
            }
            StepResult::Stay
        }
        KeyCode::Enter => {
            state.pathflow_enabled = state.setup_selection == 1;
            StepResult::Advance
        }
        KeyCode::Backspace => StepResult::Back,
        _ => StepResult::Stay,
    }
}

fn handle_verification_input(key: KeyCode) -> StepResult {
    match key {
        KeyCode::Enter => StepResult::Advance,
        KeyCode::Backspace => StepResult::Back,
        _ => StepResult::Stay,
    }
}

// ---------------------------------------------------------------------------
// Text input helpers
// ---------------------------------------------------------------------------

fn handle_text_char(state: &mut WizardState, c: char) {
    if state.config_field_focus == 0 {
        state.project_name.insert(state.project_name_cursor, c);
        state.project_name_cursor += c.len_utf8();
    } else {
        state
            .project_description
            .insert(state.project_description_cursor, c);
        state.project_description_cursor += c.len_utf8();
    }
}

fn handle_text_backspace(state: &mut WizardState) {
    if state.config_field_focus == 0 {
        if state.project_name_cursor > 0 {
            let prev = prev_char_boundary(&state.project_name, state.project_name_cursor);
            state.project_name.drain(prev..state.project_name_cursor);
            state.project_name_cursor = prev;
        }
    } else if state.project_description_cursor > 0 {
        let prev = prev_char_boundary(&state.project_description, state.project_description_cursor);
        state
            .project_description
            .drain(prev..state.project_description_cursor);
        state.project_description_cursor = prev;
    }
}

fn handle_cursor_left(state: &mut WizardState) {
    if state.config_field_focus == 0 {
        if state.project_name_cursor > 0 {
            state.project_name_cursor =
                prev_char_boundary(&state.project_name, state.project_name_cursor);
        }
    } else if state.project_description_cursor > 0 {
        state.project_description_cursor =
            prev_char_boundary(&state.project_description, state.project_description_cursor);
    }
}

fn handle_cursor_right(state: &mut WizardState) {
    if state.config_field_focus == 0 {
        if state.project_name_cursor < state.project_name.len() {
            state.project_name_cursor =
                next_char_boundary(&state.project_name, state.project_name_cursor);
        }
    } else if state.project_description_cursor < state.project_description.len() {
        state.project_description_cursor =
            next_char_boundary(&state.project_description, state.project_description_cursor);
    }
}

/// Find the previous UTF-8 character boundary before `pos`.
fn prev_char_boundary(s: &str, pos: usize) -> usize {
    let mut p = pos.saturating_sub(1);
    while p > 0 && !s.is_char_boundary(p) {
        p -= 1;
    }
    p
}

/// Find the next UTF-8 character boundary after `pos`.
fn next_char_boundary(s: &str, pos: usize) -> usize {
    let mut p = pos + 1;
    while p < s.len() && !s.is_char_boundary(p) {
        p += 1;
    }
    p.min(s.len())
}

// ---------------------------------------------------------------------------
// Prerequisite checks
// ---------------------------------------------------------------------------

fn check_prerequisites(state: &mut WizardState, runner: &dyn CommandRunner) {
    // Check git.
    if let Some(version) = runner.check_command("git", "--version") {
        state.prereqs[0].status = CheckStatus::Pass;
        state.prereqs[0].detail = Some(version);
    } else {
        state.prereqs[0].status = CheckStatus::Fail;
        state.prereqs[0].detail = Some("not found".to_string());
    }

    // Check Claude Code.
    if let Some(version) = runner.check_command("claude", "--version") {
        state.prereqs[1].status = CheckStatus::Pass;
        state.prereqs[1].detail = Some(version);
    } else {
        state.prereqs[1].status = CheckStatus::Fail;
        state.prereqs[1].detail = Some("not found".to_string());
    }
}

// ---------------------------------------------------------------------------
// Setup (step 6 action)
// ---------------------------------------------------------------------------

fn run_setup(state: &mut WizardState) -> Result<()> {
    create_project_directories(&state.project_dir, &mut state.created_dirs)?;

    if state.pathflow_enabled {
        let pathflow_dir = state.project_dir.join(".codeflow/config/pathflow");
        std::fs::create_dir_all(&pathflow_dir)
            .with_context(|| format!("creating {}", pathflow_dir.display()))?;
        state.created_dirs.push(pathflow_dir);
    }

    Ok(())
}

/// Create all essential CodeFlow directories.
pub fn create_project_directories(
    project_dir: &Path,
    created_dirs: &mut Vec<PathBuf>,
) -> Result<()> {
    let dirs = [
        ".state/db",
        ".state/ledger",
        ".state/runtime",
        ".state/logs",
        ".state/sentinels",
        ".state/session",
        ".codeflow/config",
    ];

    for dir in &dirs {
        let path = project_dir.join(dir);
        std::fs::create_dir_all(&path).with_context(|| format!("creating {}", path.display()))?;
        created_dirs.push(path);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Verification (step 7)
// ---------------------------------------------------------------------------

fn run_verification_checks(state: &mut WizardState, runner: &dyn CommandRunner) {
    state.verification.clear();

    // Check directories exist.
    let dirs_ok = state.project_dir.join(".state/db").is_dir()
        && state.project_dir.join(".codeflow/config").is_dir();
    state.verification.push(CheckItem {
        label: "Project directories".to_string(),
        status: if dirs_ok {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        detail: None,
    });

    // Check git available.
    let git_ok = runner.check_command("git", "--version").is_some();
    state.verification.push(CheckItem {
        label: "git accessible".to_string(),
        status: if git_ok {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        detail: None,
    });

    // Check Claude Code available.
    let claude_ok = runner.check_command("claude", "--version").is_some();
    state.verification.push(CheckItem {
        label: "Claude Code accessible".to_string(),
        status: if claude_ok {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        detail: None,
    });

    // Check PathFlow config if enabled.
    if state.pathflow_enabled {
        let pf_ok = state.project_dir.join(".codeflow/config/pathflow").is_dir();
        state.verification.push(CheckItem {
            label: "PathFlow configuration".to_string(),
            status: if pf_ok {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            detail: None,
        });
    }
}

// ---------------------------------------------------------------------------
// Cleanup on cancel
// ---------------------------------------------------------------------------

fn cleanup_on_cancel(state: &WizardState) {
    for dir in state.created_dirs.iter().rev() {
        // Only remove if the directory is empty (safety check).
        let _ = std::fs::remove_dir(dir);
    }
}

// ---------------------------------------------------------------------------
// Non-interactive mode (--yes or non-TTY)
// ---------------------------------------------------------------------------

fn run_non_interactive(flags: InitFlags) -> Result<()> {
    let runner = RealCommandRunner;
    run_non_interactive_with_runner(flags, &runner)
}

fn run_non_interactive_with_runner(flags: InitFlags, runner: &dyn CommandRunner) -> Result<()> {
    let project_dir = runner.current_dir()?;
    let project_type = if flags.join {
        ProjectType::Join
    } else if flags.existing {
        ProjectType::Existing
    } else {
        detect_project_type(&project_dir)
    };

    println!(
        "Project type: {}",
        match project_type {
            ProjectType::New => "new",
            ProjectType::Existing => "existing",
            ProjectType::Join => "join",
        }
    );

    // Step 2: Prerequisites.
    print!("Checking prerequisites... ");
    let git_ok = runner.check_command("git", "--version").is_some();
    let claude_ok = runner.check_command("claude", "--version").is_some();
    if git_ok {
        println!("{} git", theme::CHECKMARK);
    } else {
        println!("{} git (not found)", theme::CROSS);
    }
    if claude_ok {
        println!("  {} Claude Code", theme::CHECKMARK);
    } else {
        println!("  {} Claude Code (not found)", theme::CROSS);
    }

    if !git_ok {
        anyhow::bail!("git is required but not found on PATH");
    }

    // Steps 3-4: Skip auth in non-interactive mode.
    if !flags.skip_auth {
        println!("Skipping authentication steps in non-interactive mode");
    }

    // Step 5: Project name from directory.
    let project_name = project_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("my-project");
    println!("Project name: {project_name}");

    // Step 6: Setup.
    let mut created_dirs = Vec::new();
    create_project_directories(&project_dir, &mut created_dirs)?;
    println!("{} Created project directories", theme::CHECKMARK);

    // Step 7: Verification.
    let dirs_ok = project_dir.join(".state/db").is_dir();
    if dirs_ok {
        println!("{} Verification passed", theme::CHECKMARK);
    } else {
        println!("{} Verification failed", theme::CROSS);
    }

    println!(
        "\n{} CodeFlow initialized at {}",
        theme::CHECKMARK,
        project_dir.display()
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Mock command runner for testing.
    struct MockCommandRunner {
        commands: Mutex<Vec<(String, Option<String>)>>,
        current_dir: PathBuf,
    }

    impl MockCommandRunner {
        fn new(dir: &Path) -> Self {
            Self {
                commands: Mutex::new(Vec::new()),
                current_dir: dir.to_path_buf(),
            }
        }

        fn with_command(self, name: &str, version: Option<&str>) -> Self {
            self.commands
                .lock()
                .unwrap()
                .push((name.to_string(), version.map(ToString::to_string)));
            self
        }
    }

    impl CommandRunner for MockCommandRunner {
        fn check_command(&self, name: &str, _version_flag: &str) -> Option<String> {
            let commands = self.commands.lock().unwrap();
            commands
                .iter()
                .find(|(n, _)| n == name)
                .and_then(|(_, v)| v.clone())
        }

        fn run_command(&self, _name: &str, _args: &[&str]) -> Result<bool> {
            Ok(true)
        }

        fn current_dir(&self) -> Result<PathBuf> {
            Ok(self.current_dir.clone())
        }
    }

    #[test]
    fn test_detect_project_type_new() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect_project_type(dir.path()), ProjectType::New);
    }

    #[test]
    fn test_detect_project_type_existing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        std::fs::create_dir_all(dir.path().join(".state")).unwrap();
        assert_eq!(detect_project_type(dir.path()), ProjectType::Existing);
    }

    #[test]
    fn test_detect_project_type_join() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        // No .state directory -- this is a "join" scenario.
        assert_eq!(detect_project_type(dir.path()), ProjectType::Join);
    }

    #[test]
    fn test_wizard_state_new() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let state = WizardState::new(flags, &runner).unwrap();
        assert_eq!(state.current_step, 0);
        assert_eq!(state.project_type, ProjectType::New);
        assert_eq!(state.prereqs.len(), 2);
        assert!(!state.skip_auth);
    }

    #[test]
    fn test_wizard_state_with_existing_flag() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: true,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let state = WizardState::new(flags, &runner).unwrap();
        assert_eq!(state.project_type, ProjectType::Existing);
        assert!(state.location_preset);
    }

    #[test]
    fn test_wizard_state_with_join_flag() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: true,
            skip_auth: false,
            yes: false,
        };
        let state = WizardState::new(flags, &runner).unwrap();
        assert_eq!(state.project_type, ProjectType::Join);
        assert!(state.location_preset);
    }

    #[test]
    fn test_step_entries_initial() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let state = WizardState::new(flags, &runner).unwrap();
        let entries = state.step_entries();
        assert_eq!(entries.len(), 7);
        assert_eq!(entries[0].status, StepStatus::Current);
        for entry in &entries[1..] {
            assert_eq!(entry.status, StepStatus::Pending);
        }
    }

    #[test]
    fn test_step_entries_skip_auth() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: true,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.current_step = 4; // Past auth steps.
        let entries = state.step_entries();
        assert_eq!(entries[2].status, StepStatus::Skipped);
        assert_eq!(entries[3].status, StepStatus::Skipped);
    }

    #[test]
    fn test_check_prerequisites_all_pass() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path())
            .with_command("git", Some("git version 2.42.0"))
            .with_command("claude", Some("claude 1.0.0"));
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        check_prerequisites(&mut state, &runner);
        assert_eq!(state.prereqs[0].status, CheckStatus::Pass);
        assert_eq!(state.prereqs[1].status, CheckStatus::Pass);
    }

    #[test]
    fn test_check_prerequisites_git_missing() {
        let dir = tempfile::tempdir().unwrap();
        let runner =
            MockCommandRunner::new(dir.path()).with_command("claude", Some("claude 1.0.0"));
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        check_prerequisites(&mut state, &runner);
        assert_eq!(state.prereqs[0].status, CheckStatus::Fail);
        assert_eq!(state.prereqs[1].status, CheckStatus::Pass);
    }

    #[test]
    fn test_check_prerequisites_claude_missing() {
        let dir = tempfile::tempdir().unwrap();
        let runner =
            MockCommandRunner::new(dir.path()).with_command("git", Some("git version 2.42.0"));
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        check_prerequisites(&mut state, &runner);
        assert_eq!(state.prereqs[0].status, CheckStatus::Pass);
        assert_eq!(state.prereqs[1].status, CheckStatus::Fail);
    }

    #[test]
    fn test_create_project_directories() {
        let dir = tempfile::tempdir().unwrap();
        let mut created = Vec::new();
        create_project_directories(dir.path(), &mut created).unwrap();
        assert!(dir.path().join(".state/db").is_dir());
        assert!(dir.path().join(".state/ledger").is_dir());
        assert!(dir.path().join(".state/runtime").is_dir());
        assert!(dir.path().join(".state/logs").is_dir());
        assert!(dir.path().join(".state/sentinels").is_dir());
        assert!(dir.path().join(".state/session").is_dir());
        assert!(dir.path().join(".codeflow/config").is_dir());
        assert_eq!(created.len(), 7);
    }

    #[test]
    fn test_create_project_directories_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let mut c1 = Vec::new();
        let mut c2 = Vec::new();
        create_project_directories(dir.path(), &mut c1).unwrap();
        create_project_directories(dir.path(), &mut c2).unwrap();
        assert!(dir.path().join(".state/db").is_dir());
    }

    #[test]
    fn test_run_setup_standard() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.pathflow_enabled = false;
        run_setup(&mut state).unwrap();
        assert!(dir.path().join(".state/db").is_dir());
        assert!(!dir.path().join(".codeflow/config/pathflow").is_dir());
    }

    #[test]
    fn test_run_setup_with_pathflow() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.pathflow_enabled = true;
        run_setup(&mut state).unwrap();
        assert!(dir.path().join(".codeflow/config/pathflow").is_dir());
    }

    #[test]
    fn test_verification_checks_pass() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path())
            .with_command("git", Some("2.42"))
            .with_command("claude", Some("1.0"));
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        run_setup(&mut state).unwrap();
        run_verification_checks(&mut state, &runner);
        assert!(
            state
                .verification
                .iter()
                .all(|v| v.status == CheckStatus::Pass)
        );
    }

    #[test]
    fn test_verification_checks_dirs_missing() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path())
            .with_command("git", Some("2.42"))
            .with_command("claude", Some("1.0"));
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        // Do NOT run setup -- directories are missing.
        run_verification_checks(&mut state, &runner);
        assert_eq!(state.verification[0].status, CheckStatus::Fail);
    }

    #[test]
    fn test_cleanup_on_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        run_setup(&mut state).unwrap();
        assert!(dir.path().join(".state/db").is_dir());
        cleanup_on_cancel(&state);
        // Cleanup removes empty leaf directories in reverse order.
        // Parent dirs may remain if non-empty. This is intentional safety.
    }

    #[test]
    fn test_handle_location_input_up() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.location_selection = 2;
        let result = handle_location_input(&mut state, KeyCode::Up);
        assert_eq!(result, StepResult::Stay);
        assert_eq!(state.location_selection, 1);
    }

    #[test]
    fn test_handle_location_input_down() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.location_selection = 0;
        handle_location_input(&mut state, KeyCode::Down);
        assert_eq!(state.location_selection, 1);
    }

    #[test]
    fn test_handle_location_input_down_at_max() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.location_selection = 2;
        handle_location_input(&mut state, KeyCode::Down);
        assert_eq!(state.location_selection, 2);
    }

    #[test]
    fn test_handle_location_input_enter() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.location_selection = 1;
        let result = handle_location_input(&mut state, KeyCode::Enter);
        assert_eq!(result, StepResult::Advance);
        assert_eq!(state.project_type, ProjectType::Existing);
    }

    #[test]
    fn test_handle_prereqs_input_all_pass() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path())
            .with_command("git", Some("2.42"))
            .with_command("claude", Some("1.0"));
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        check_prerequisites(&mut state, &runner);
        let result = handle_prereqs_input(&mut state, KeyCode::Enter);
        assert_eq!(result, StepResult::Advance);
    }

    #[test]
    fn test_handle_prereqs_input_back() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        let result = handle_prereqs_input(&mut state, KeyCode::Backspace);
        assert_eq!(result, StepResult::Back);
    }

    #[test]
    fn test_handle_config_input_text() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        let original_len = state.project_name.len();
        handle_config_input(&mut state, KeyCode::Char('!'));
        assert_eq!(state.project_name.len(), original_len + 1);
    }

    #[test]
    fn test_handle_config_input_tab_switches_field() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        assert_eq!(state.config_field_focus, 0);
        handle_config_input(&mut state, KeyCode::Tab);
        assert_eq!(state.config_field_focus, 1);
        handle_config_input(&mut state, KeyCode::Tab);
        assert_eq!(state.config_field_focus, 0);
    }

    #[test]
    fn test_handle_text_backspace() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.project_name = "hello".to_string();
        state.project_name_cursor = 5;
        handle_text_backspace(&mut state);
        assert_eq!(state.project_name, "hell");
        assert_eq!(state.project_name_cursor, 4);
    }

    #[test]
    fn test_handle_text_backspace_empty() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.project_name = String::new();
        state.project_name_cursor = 0;
        handle_text_backspace(&mut state);
        assert_eq!(state.project_name, "");
        assert_eq!(state.project_name_cursor, 0);
    }

    #[test]
    fn test_handle_cursor_left_right() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.project_name = "abc".to_string();
        state.project_name_cursor = 3;
        handle_cursor_left(&mut state);
        assert_eq!(state.project_name_cursor, 2);
        handle_cursor_right(&mut state);
        assert_eq!(state.project_name_cursor, 3);
    }

    #[test]
    fn test_handle_cursor_left_at_zero() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.project_name = "abc".to_string();
        state.project_name_cursor = 0;
        handle_cursor_left(&mut state);
        assert_eq!(state.project_name_cursor, 0);
    }

    #[test]
    fn test_handle_cursor_right_at_end() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.project_name = "abc".to_string();
        state.project_name_cursor = 3;
        handle_cursor_right(&mut state);
        assert_eq!(state.project_name_cursor, 3);
    }

    #[test]
    fn test_prev_char_boundary() {
        assert_eq!(prev_char_boundary("hello", 5), 4);
        assert_eq!(prev_char_boundary("hello", 0), 0);
        assert_eq!(prev_char_boundary("hello", 1), 0);
    }

    #[test]
    fn test_next_char_boundary() {
        assert_eq!(next_char_boundary("hello", 0), 1);
        assert_eq!(next_char_boundary("hello", 4), 5);
        assert_eq!(next_char_boundary("hello", 5), 5);
    }

    #[test]
    fn test_prev_char_boundary_multibyte() {
        // Two-byte char: 'e' with accent (U+00E9 = 2 bytes in UTF-8).
        let s = "caf\u{00E9}";
        assert_eq!(s.len(), 5); // c(1) a(1) f(1) e-accent(2)
        assert_eq!(prev_char_boundary(s, 5), 3);
    }

    #[test]
    fn test_next_char_boundary_multibyte() {
        let s = "caf\u{00E9}";
        assert_eq!(next_char_boundary(s, 3), 5);
    }

    #[test]
    fn test_non_interactive_with_runner() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path())
            .with_command("git", Some("git version 2.42.0"))
            .with_command("claude", Some("claude 1.0.0"));
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: true,
        };
        let result = run_non_interactive_with_runner(flags, &runner);
        assert!(result.is_ok());
        assert!(dir.path().join(".state/db").is_dir());
    }

    #[test]
    fn test_non_interactive_git_missing_fails() {
        let dir = tempfile::tempdir().unwrap();
        let runner =
            MockCommandRunner::new(dir.path()).with_command("claude", Some("claude 1.0.0"));
        // No git command registered.
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: true,
        };
        let result = run_non_interactive_with_runner(flags, &runner);
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_setup_input_up_down() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.setup_selection = 0;
        handle_setup_input(&mut state, KeyCode::Down);
        assert_eq!(state.setup_selection, 1);
        handle_setup_input(&mut state, KeyCode::Up);
        assert_eq!(state.setup_selection, 0);
    }

    #[test]
    fn test_handle_setup_input_enter_pathflow() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.setup_selection = 1;
        handle_setup_input(&mut state, KeyCode::Enter);
        assert!(state.pathflow_enabled);
    }

    #[test]
    fn test_handle_verification_input_enter() {
        let result = handle_verification_input(KeyCode::Enter);
        assert_eq!(result, StepResult::Advance);
    }

    #[test]
    fn test_handle_verification_input_back() {
        let result = handle_verification_input(KeyCode::Backspace);
        assert_eq!(result, StepResult::Back);
    }

    #[test]
    fn test_step_result_equality() {
        assert_eq!(StepResult::Advance, StepResult::Advance);
        assert_eq!(StepResult::Stay, StepResult::Stay);
        assert_eq!(StepResult::Back, StepResult::Back);
        assert_eq!(StepResult::Quit, StepResult::Quit);
        assert_ne!(StepResult::Advance, StepResult::Stay);
        assert_ne!(StepResult::Stay, StepResult::Back);
    }

    #[test]
    fn test_project_type_equality() {
        assert_eq!(ProjectType::New, ProjectType::New);
        assert_ne!(ProjectType::New, ProjectType::Existing);
        assert_ne!(ProjectType::Existing, ProjectType::Join);
    }

    #[test]
    fn test_handle_config_input_description_field() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.config_field_focus = 1;
        handle_config_input(&mut state, KeyCode::Char('x'));
        assert_eq!(state.project_description, "x");
    }

    #[test]
    fn test_handle_text_backspace_description() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.config_field_focus = 1;
        state.project_description = "abc".to_string();
        state.project_description_cursor = 3;
        handle_text_backspace(&mut state);
        assert_eq!(state.project_description, "ab");
    }

    #[test]
    fn test_verification_with_pathflow() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path())
            .with_command("git", Some("2.42"))
            .with_command("claude", Some("1.0"));
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.pathflow_enabled = true;
        run_setup(&mut state).unwrap();
        run_verification_checks(&mut state, &runner);
        assert_eq!(state.verification.len(), 4); // 3 standard + 1 pathflow
        assert!(
            state
                .verification
                .iter()
                .all(|v| v.status == CheckStatus::Pass)
        );
    }

    #[test]
    fn test_handle_auth_input_up_down() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.auth_selection = 0;
        handle_auth_input(&mut state, KeyCode::Down, &runner);
        assert_eq!(state.auth_selection, 1);
        handle_auth_input(&mut state, KeyCode::Down, &runner);
        assert_eq!(state.auth_selection, 1); // Can't go beyond 1.
        handle_auth_input(&mut state, KeyCode::Up, &runner);
        assert_eq!(state.auth_selection, 0);
    }

    #[test]
    fn test_handle_git_provider_input_navigation() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.git_provider_selection = 0;
        handle_git_provider_input(&mut state, KeyCode::Down, &runner);
        assert_eq!(state.git_provider_selection, 1);
        handle_git_provider_input(&mut state, KeyCode::Down, &runner);
        assert_eq!(state.git_provider_selection, 2);
        handle_git_provider_input(&mut state, KeyCode::Down, &runner);
        assert_eq!(state.git_provider_selection, 2); // Max.
    }

    #[test]
    fn test_handle_step_input_esc_quits() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        let result = handle_step_input(&mut state, KeyCode::Esc, &runner);
        assert_eq!(result, StepResult::Quit);
    }

    #[test]
    fn test_mock_runner_run_command() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let result = runner.run_command("echo", &["hello"]);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_mock_runner_current_dir() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let cwd = runner.current_dir().unwrap();
        assert_eq!(cwd, dir.path().to_path_buf());
    }

    #[test]
    fn test_mock_runner_check_command_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        assert!(runner.check_command("nonexistent", "--version").is_none());
    }

    #[test]
    fn test_handle_config_enter_empty_name_stays() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.project_name = String::new();
        let result = handle_config_input(&mut state, KeyCode::Enter);
        assert_eq!(result, StepResult::Stay);
    }

    #[test]
    fn test_handle_config_enter_with_name_advances() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MockCommandRunner::new(dir.path());
        let flags = InitFlags {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        };
        let mut state = WizardState::new(flags, &runner).unwrap();
        state.project_name = "my-project".to_string();
        let result = handle_config_input(&mut state, KeyCode::Enter);
        assert_eq!(result, StepResult::Advance);
    }
}
