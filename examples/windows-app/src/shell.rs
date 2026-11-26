//! The WinUI window: analyse a folder (with progress and Cancel), preview
//! where its files would go, apply, undo.
//!
//! Engine calls run on the thread pool via `spawn_background`; each returns
//! one message. Progress is polled: a background "tick" re-arms itself
//! while an analysis runs, because a component's sender can't be used from
//! another thread.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use app_core::{Job, Progress, UserText};
use app_platform_windows::{Workspace, data_dir};
use windows_reactor::{
    Button, ButtonStyle, CheckBox, ChildrenControl, Component, ComponentContext, ContentControl,
    FontWeight, InfoBar, InfoBarSeverity, LayoutControl, Orientation, ProgressBar, ProgressRing,
    ScrollViewer, StackPanel, TextBlock, TextBox, TextWrapping, Thickness, View, ViewContext,
};

use crate::organize::{self, Outcome, Preview};
use crate::summary::{self, Analysis, Stopped, Summary};
use crate::text::Translations;

/// How often the progress bar refreshes.
const TICK: Duration = Duration::from_millis(100);
/// Rows drawn in a preview; the rest are applied too, just not listed.
const MAX_PREVIEW_ROWS: usize = 300;

/// `Clone` lets a button send a fixed message (`ViewContext::message`).
#[derive(Debug, Clone)]
pub enum Message {
    FolderChanged(String),
    DestinationChanged(String),
    Analyze,
    Cancel,
    /// Refresh progress for analysis number `.0`.
    Tick(u64),
    /// Analysis number `.0` ended.
    Analyzed(u64, Result<Analysis, Stopped>),
    Preview,
    ToggleMove(usize, bool),
    Back,
    Apply,
    Undo,
    Finished {
        result: Result<Outcome, UserText>,
        undone: bool,
        can_undo: bool,
    },
}

#[derive(Debug)]
enum Status {
    Idle,
    Analyzing {
        job: Arc<Job>,
        progress: Progress,
    },
    Analyzed(Analysis),
    Previewing {
        analysis: Analysis,
        preview: Preview,
        /// Indexes of moves the user unticked.
        skip: BTreeSet<usize>,
    },
    Working(&'static str),
    Finished {
        outcome: Outcome,
        undone: bool,
    },
    Failed(UserText),
}

#[derive(Debug)]
pub struct Shell {
    /// `Err` holds a user-facing message when the engine couldn't open.
    workspace: Result<Workspace, String>,
    notices: Vec<UserText>,
    /// Engine messages in the user's language (see `text.rs`).
    text: Translations,
    folder: String,
    destination: String,
    status: Status,
    can_undo: bool,
    /// Numbers analyses so results and ticks of an old one are ignored.
    generation: u64,
}

impl Component for Shell {
    type Input = ();
    type Message = Message;

    fn create(_input: &(), _context: &ComponentContext<Self>) -> Self {
        let (workspace, notices) = match Workspace::open(data_dir::resolve()) {
            Ok((workspace, notices)) => (Ok(workspace), notices),
            Err(message) => (Err(message), Vec::new()),
        };
        let can_undo = workspace.as_ref().is_ok_and(organize::can_undo);
        let display = |path: Option<std::path::PathBuf>| {
            path.map(|p| p.display().to_string()).unwrap_or_default()
        };
        Self {
            workspace,
            notices,
            text: load_translations(),
            folder: display(data_dir::default_inbox()),
            destination: display(data_dir::default_destination()),
            status: Status::Idle,
            can_undo,
            generation: 0,
        }
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::FolderChanged(folder) => self.folder = folder,
            Message::DestinationChanged(destination) => self.destination = destination,
            Message::Analyze => self.start_analysis(context),
            Message::Cancel => {
                if let Status::Analyzing { job, .. } = &self.status {
                    job.cancel();
                }
            }
            Message::Tick(generation) => self.refresh_progress(generation, context),
            Message::Analyzed(generation, result) if generation == self.generation => {
                self.status = match result {
                    Ok(analysis) => Status::Analyzed(analysis),
                    Err(Stopped::Cancelled) => Status::Idle,
                    Err(Stopped::Failed(message)) => Status::Failed(message),
                };
            }
            Message::Analyzed(..) => {} // superseded analysis
            Message::Preview => self.preview(),
            Message::ToggleMove(index, checked) => {
                if let Status::Previewing { skip, .. } = &mut self.status {
                    if checked {
                        skip.remove(&index);
                    } else {
                        skip.insert(index);
                    }
                }
            }
            Message::Back => {
                if let Status::Previewing { analysis, .. } =
                    std::mem::replace(&mut self.status, Status::Idle)
                {
                    self.status = Status::Analyzed(analysis);
                }
            }
            Message::Apply => self.apply(context),
            Message::Undo => self.undo(context),
            Message::Finished {
                result,
                undone,
                can_undo,
            } => {
                self.can_undo = can_undo;
                self.status = match result {
                    Ok(outcome) => Status::Finished { outcome, undone },
                    Err(message) => Status::Failed(message),
                };
            }
        }
    }

    fn view(&self, _input: &(), context: &mut ViewContext<Self>) -> View {
        context.window_title("App Starter");
        let body = StackPanel::new()
            .spacing(20.0)
            .max_width(720.0)
            .margin(Thickness::uniform(32.0))
            .children((
                header(),
                self.banner(),
                self.folder_row(context),
                self.content(context),
                self.undo_row(context),
            ));
        ScrollViewer::new().content(body)
    }
}

// Actions ------------------------------------------------------------------

impl Shell {
    fn is_busy(&self) -> bool {
        matches!(self.status, Status::Analyzing { .. } | Status::Working(_))
    }

    fn start_analysis(&mut self, context: &ComponentContext<Self>) {
        let Ok(workspace) = &self.workspace else {
            return;
        };
        if self.is_busy() {
            return;
        }
        let (workspace, folder) = (workspace.clone(), self.folder.clone());
        let job = Arc::new(Job::new());
        let worker_job = Arc::clone(&job);
        self.generation += 1;
        let generation = self.generation;
        // The engine doesn't see reactor's cancellation token; the tick
        // forwards it to the job, so closing the window stops the analysis.
        schedule_tick(generation, Arc::clone(&job), context);
        self.status = Status::Analyzing {
            job,
            progress: Progress::default(),
        };
        let _task = context.spawn_background(move |_| {
            Message::Analyzed(
                generation,
                summary::analyze(&workspace, &folder, &worker_job),
            )
        });
    }

    fn refresh_progress(&mut self, generation: u64, context: &ComponentContext<Self>) {
        if generation != self.generation {
            return;
        }
        if let Status::Analyzing { job, progress } = &mut self.status {
            *progress = job.progress();
            schedule_tick(generation, Arc::clone(job), context);
        }
    }

    /// Planning is quick (no file I/O beyond name checks), so it runs inline.
    fn preview(&mut self) {
        let (Ok(workspace), Status::Analyzed(analysis)) = (&self.workspace, &self.status) else {
            return;
        };
        match organize::plan(workspace, &analysis.insights, &self.destination) {
            Ok(preview) => {
                let Status::Analyzed(analysis) = std::mem::replace(&mut self.status, Status::Idle)
                else {
                    return;
                };
                self.status = Status::Previewing {
                    analysis,
                    preview,
                    skip: BTreeSet::new(),
                };
            }
            Err(message) => self.status = Status::Failed(message),
        }
    }

    fn apply(&mut self, context: &ComponentContext<Self>) {
        let (Ok(workspace), Status::Previewing { preview, skip, .. }) =
            (&self.workspace, &self.status)
        else {
            return;
        };
        let workspace = workspace.clone();
        let (plan_id, skip): (u64, Vec<usize>) = (preview.plan_id, skip.iter().copied().collect());
        self.status = Status::Working("Moving files…");
        let _task = context.spawn_background(move |_| Message::Finished {
            result: organize::apply(&workspace, plan_id, &skip),
            undone: false,
            can_undo: organize::can_undo(&workspace),
        });
    }

    fn undo(&mut self, context: &ComponentContext<Self>) {
        let Ok(workspace) = &self.workspace else {
            return;
        };
        if self.is_busy() {
            return;
        }
        let workspace = workspace.clone();
        self.status = Status::Working("Putting files back…");
        let _task = context.spawn_background(move |_| Message::Finished {
            result: organize::undo(&workspace),
            undone: true,
            can_undo: organize::can_undo(&workspace),
        });
    }
}

/// Refreshes progress after [`TICK`]. Reactor cancels the token when the
/// window closes; the tick then cancels `job` so the engine stops too.
/// The translation file for the user's UI languages from the `l10n`
/// folder next to the app (see `text.rs`).
fn load_translations() -> Translations {
    let languages = app_platform_windows::edge_panel::win32::preferred_ui_languages();
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("l10n")))
        .map_or_else(Translations::english, |dir| {
            Translations::for_languages(&dir, &languages)
        })
}

fn schedule_tick(generation: u64, job: Arc<Job>, context: &ComponentContext<Shell>) {
    let _task = context.spawn_background(move |token| {
        std::thread::sleep(TICK);
        if token.is_cancelled() {
            job.cancel();
        }
        Message::Tick(generation)
    });
}

// Views --------------------------------------------------------------------

fn header() -> View {
    StackPanel::new().spacing(4.0).children((
        TextBlock::new()
            .text("Tidy a folder")
            .font_size(28.0)
            .font_weight(FontWeight::SEMI_BOLD),
        TextBlock::new()
            .text("Everything runs on this PC. Nothing moves until you apply, and you can undo."),
    ))
}

impl Shell {
    /// Startup problems: the engine didn't open, or it set a file aside.
    fn banner(&self) -> View {
        match &self.workspace {
            Err(message) => info_bar(InfoBarSeverity::Error, "Can't open app data", message),
            Ok(_) if !self.notices.is_empty() => info_bar(
                InfoBarSeverity::Warning,
                "Settings were reset",
                &self
                    .notices
                    .iter()
                    .map(|notice| self.text.show(notice))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            Ok(_) => View::fragment(()),
        }
    }

    fn folder_row(&self, context: &ViewContext<Self>) -> View {
        let ready = self.workspace.is_ok() && !self.is_busy();
        StackPanel::new().spacing(8.0).children((
            TextBox::new()
                .text(self.folder.clone())
                .placeholder_text(r"C:\Users\you\Downloads")
                .is_enabled(ready)
                .on_text_changed(context.callback(Message::FolderChanged)),
            Button::new()
                .is_enabled(ready)
                .on_click(context.message(Message::Analyze))
                .content("Analyse"),
        ))
    }

    fn content(&self, context: &ViewContext<Self>) -> View {
        match &self.status {
            Status::Idle => TextBlock::new()
                .text("Enter a folder and choose Analyse.")
                .into(),
            Status::Analyzing { progress, .. } => progress_view(*progress, context),
            Status::Analyzed(analysis) => self.analyzed_view(&analysis.summary, context),
            Status::Previewing { preview, skip, .. } => preview_view(preview, skip, context),
            Status::Working(text) => StackPanel::new().spacing(8.0).children((
                ProgressRing::new().is_active(true),
                TextBlock::new().text(*text),
            )),
            Status::Finished { outcome, undone } => outcome_view(outcome, *undone, &self.text),
            Status::Failed(message) => info_bar(
                InfoBarSeverity::Error,
                "Something went wrong",
                &self.text.show(message),
            ),
        }
    }

    fn analyzed_view(&self, summary: &Summary, context: &ViewContext<Self>) -> View {
        StackPanel::new().spacing(16.0).children((
            summary_view(summary),
            TextBlock::new().text("File into (one subfolder per category):"),
            TextBox::new()
                .text(self.destination.clone())
                .on_text_changed(context.callback(Message::DestinationChanged)),
            Button::new()
                .style(ButtonStyle::Accent)
                .on_click(context.message(Message::Preview))
                .content("Preview changes"),
        ))
    }

    fn undo_row(&self, context: &ViewContext<Self>) -> View {
        if !self.can_undo || self.is_busy() {
            return View::fragment(());
        }
        Button::new()
            .on_click(context.message(Message::Undo))
            .content("Undo last change")
    }
}

fn progress_view(progress: Progress, context: &ViewContext<Shell>) -> View {
    let (bar, label) = match progress.fraction() {
        Some(fraction) => (
            ProgressBar::new().maximum(1.0).value(fraction),
            format!("Analysing… {} of {} files", progress.done, progress.total),
        ),
        None => (
            ProgressBar::new().is_indeterminate(true),
            "Looking for files…".to_owned(),
        ),
    };
    StackPanel::new().spacing(8.0).children((
        bar,
        TextBlock::new().text(label),
        Button::new()
            .on_click(context.message(Message::Cancel))
            .content("Cancel"),
    ))
}

fn summary_view(summary: &Summary) -> View {
    let folder = summary.folder.display();
    let heading = match summary.unreadable {
        0 => format!("{} files in {folder}", summary.files),
        n => format!("{} files in {folder} ({n} couldn't be read)", summary.files),
    };
    let rows = summary.categories.iter().map(|count| {
        let row = TextBlock::new().text(format!(
            "{}: {} ({})",
            count.title,
            count.files,
            summary::format_size(count.bytes)
        ));
        (count.title, View::from(row))
    });
    StackPanel::new().spacing(6.0).children((
        TextBlock::new()
            .text(heading)
            .font_weight(FontWeight::SEMI_BOLD),
        StackPanel::new().spacing(2.0).keyed_children(rows),
    ))
}

fn preview_view(preview: &Preview, skip: &BTreeSet<usize>, context: &ViewContext<Shell>) -> View {
    let selected = preview.moves.len() - skip.len();
    let mut notes = vec![format!("{} files left as they are.", preview.unchanged)];
    if !preview.missing.is_empty() {
        notes.push(format!(
            "No destination for: {}.",
            preview.missing.join(", ")
        ));
    }
    if preview.moves.len() > MAX_PREVIEW_ROWS {
        notes.push(format!(
            "Showing the first {MAX_PREVIEW_ROWS}; the other {} are included.",
            preview.moves.len() - MAX_PREVIEW_ROWS
        ));
    }
    let rows = preview
        .moves
        .iter()
        .enumerate()
        .take(MAX_PREVIEW_ROWS)
        .map(|(index, row)| {
            let label = format!("{}  →  {}\\{}", row.from_name, row.to_folder, row.to_name);
            let check = CheckBox::new()
                .is_checked(!skip.contains(&index))
                .on_is_checked_changed(
                    context.callback(move |checked| Message::ToggleMove(index, checked)),
                )
                .content(label);
            (index, check)
        });
    StackPanel::new().spacing(12.0).children((
        TextBlock::new()
            .text(format!(
                "{selected} of {} files will move",
                preview.moves.len()
            ))
            .font_weight(FontWeight::SEMI_BOLD),
        TextBlock::new()
            .text(notes.join(" "))
            .text_wrapping(TextWrapping::Wrap),
        StackPanel::new().spacing(2.0).keyed_children(rows),
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(8.0)
            .children((
                Button::new()
                    .style(ButtonStyle::Accent)
                    .is_enabled(selected > 0)
                    .on_click(context.message(Message::Apply))
                    .content("Apply"),
                Button::new()
                    .on_click(context.message(Message::Back))
                    .content("Back"),
            )),
    ))
}

fn outcome_view(outcome: &Outcome, undone: bool, text: &Translations) -> View {
    let verb = if undone { "Put back" } else { "Moved" };
    let mut lines = vec![format!("{verb} {} files.", outcome.moved)];
    if outcome.copied > 0 {
        lines.push(format!(
            "Copied {} files whose folder couldn't be changed.",
            outcome.copied
        ));
    }
    lines.extend(outcome.warning.iter().map(|warning| text.show(warning)));
    lines.extend(
        outcome
            .failures
            .iter()
            .map(|(file, reason)| format!("{file}: {}", text.show(reason))),
    );
    let severity = if outcome.failures.is_empty() && outcome.warning.is_none() {
        InfoBarSeverity::Success
    } else {
        InfoBarSeverity::Warning
    };
    info_bar(severity, "Done", &lines.join("\n"))
}

fn info_bar(severity: InfoBarSeverity, title: &str, message: &str) -> View {
    InfoBar::new()
        .severity(severity)
        .title(title)
        .message(message)
        .is_open(true)
        .is_closable(false)
        .into()
}
