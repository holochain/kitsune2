use crate::app::App;
use bytes::Bytes;
use rustyline::error::ReadlineError;
use rustyline::ExternalPrinter;
use std::borrow::Cow;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use strum::{
    EnumIter, EnumMessage, EnumString, IntoEnumIterator, IntoStaticStr,
};
use tokio::sync::mpsc::Receiver;

#[derive(IntoStaticStr, EnumMessage, EnumIter, EnumString)]
#[strum(serialize_all = "lowercase", prefix = "/")]
enum Command {
    #[strum(disabled)]
    Invalid,

    #[strum(message = "print this help")]
    Help,

    #[strum(message = "quit this program")]
    Exit,

    #[strum(message = "[filename] share a file if under 1K")]
    Share,

    #[strum(message = "print network statistics")]
    Stats,

    #[strum(message = "list files shared")]
    List,

    #[strum(message = "[filename] fetch a shared file")]
    Fetch,
}

/// Print out command help.
fn help() {
    println!("\n# Kitsune2 Showcase chat and file sharing app\n");
    for cmd in Command::iter() {
        println!(
            "{} - {}",
            Into::<&str>::into(&cmd),
            cmd.get_message().unwrap_or_default()
        );
    }
}

/// Parse the first part as a [`Command`] and return it with the rest.
///
/// Returns [`Command::Invalid`] if not a valid [`Command`].
/// Returns [`None`] if input does not start with the command prefix.
fn split_on_command(line: &str) -> Option<(Command, &str)> {
    line.strip_prefix("/")
        .map(|s| s.split_once(" ").unwrap_or((s, "")))
        .map(|(cmd_str, rest)| {
            (Command::from_str(cmd_str).unwrap_or(Command::Invalid), rest)
        })
}

/// Blocking loop waiting for user input / handling commands.
pub async fn readline(
    nick: String,
    mut printer_rx: Receiver<String>,
    app: App,
) {
    // print command help
    help();

    let prompt = format!("{nick}> ");

    let mut line_editor = rustyline::Editor::with_history(
        rustyline::Config::builder()
            .completion_type(rustyline::CompletionType::List)
            .build(),
        rustyline::history::MemHistory::new(),
    )
    .expect("Failed to create rustyline line editor");
    line_editor.set_helper(Some(Helper::default()));
    let mut printer = line_editor
        .create_external_printer()
        .expect("Failed to get rustyline external printer");

    tokio::spawn(async move {
        while let Some(msg) = printer_rx.recv().await {
            printer.print(format!("{msg}\n")).ok();
        }
    });

    let line_editor = Arc::new(Mutex::new(line_editor));

    loop {
        let line_editor_2 = line_editor.clone();
        let prompt = prompt.clone();
        let line = tokio::task::spawn_blocking(move || {
            line_editor_2
                .lock()
                .expect("failed to get lock for line_editor")
                .readline(&prompt)
        })
        .await
        .expect("Failed to spawn blocking task to read stdin");

        match line {
            Err(ReadlineError::Eof) => break,
            Err(ReadlineError::Interrupted) => println!("^C"),
            Err(err) => {
                eprintln!("Failed to read line: {err}");
                break;
            }
            Ok(line) if !line.trim().is_empty() => {
                if let Err(err) = line_editor
                    .lock()
                    .expect("failed to get lock for line_editor")
                    .add_history_entry(line.clone())
                {
                    eprintln!("Failed to add line to history: {err}");
                }
                if let Some((cmd, rest)) = split_on_command(&line) {
                    match cmd {
                        Command::Help => help(),
                        Command::Exit => break,
                        Command::Stats => {
                            if let Err(err) = app.stats().await {
                                eprintln!("Failed to get stats: {err}");
                            }
                        }
                        Command::Share => {
                            if let Err(err) = app.share(Path::new(rest)).await {
                                eprintln!("Failed to share file: {err}");
                            }
                        }
                        Command::List => {
                            if let Err(err) = app.list().await {
                                eprintln!("Failed to list shared files: {err}");
                            }
                        }
                        Command::Fetch => {
                            if let Err(err) = app.fetch(rest).await {
                                eprintln!("Failed to fetch file: {err}");
                            }
                        }
                        Command::Invalid => {
                            eprintln!("Invalid Command. Valid commands are:");
                            Command::iter().for_each(|cmd| {
                                eprintln!(
                                    "{} - {}",
                                    Into::<&str>::into(&cmd),
                                    cmd.get_message().unwrap_or_default()
                                );
                            });
                        }
                    }
                // Not a command so send as chat message
                } else if let Err(err) =
                    app.chat(Bytes::copy_from_slice(line.as_bytes())).await
                {
                    println!("Failed to send chat message: {err}")
                }
            }
            _ => {}
        }
    }
}

#[derive(Default, rustyline::Helper, rustyline::Validator)]
struct Helper {
    /// Used to show hints of previous entries as you type.
    ///
    /// Note: Use the right-arrow key to accept the hint.
    history_hinter: rustyline::hint::HistoryHinter,

    /// Used to auto-complete file paths when using the [`Command::Share`] command.
    ///
    /// Note: Use the tab key to complete the path.
    file_name_completer: rustyline::completion::FilenameCompleter,
}

impl rustyline::highlight::Highlighter for Helper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        Cow::Owned(format!("\x1b[1;36m{prompt}\x1b[0m"))
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!("\x1b[1;90m{hint}\x1b[0m"))
    }
}

impl rustyline::hint::Hinter for Helper {
    type Hint = String;

    fn hint(
        &self,
        line: &str,
        pos: usize,
        ctx: &rustyline::Context<'_>,
    ) -> Option<Self::Hint> {
        if line.is_empty() {
            return None;
        }
        self.history_hinter.hint(line, pos, ctx).or_else(|| {
            Command::iter()
                .map(Into::<&'static str>::into)
                .find(|c| c.starts_with(line))
                .map(|c| c.trim_start_matches(line).to_string())
        })
    }
}

impl rustyline::completion::Completer for Helper {
    type Candidate = rustyline::completion::Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        match split_on_command(line) {
            Some((Command::Share, args)) => {
                let candidates =
                    self.file_name_completer.complete_path(args, args.len())?.1;

                Ok((pos - args.len(), candidates))
            }
            _ => {
                let candidates = Command::iter()
                    .map(Into::<&'static str>::into)
                    .filter(|c| c.starts_with(line))
                    .map(|c| Self::Candidate {
                        display: c.to_string(),
                        replacement: format!("{} ", c.trim_start_matches(line)),
                    })
                    .collect();

                Ok((pos, candidates))
            }
        }
    }
}
