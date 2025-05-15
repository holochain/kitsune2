use std::borrow::Cow;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use rustyline::error::ReadlineError;
use rustyline::ExternalPrinter;
use strum::{
    EnumIter, EnumMessage, EnumString, IntoEnumIterator, IntoStaticStr,
};
use tokio::sync::mpsc::Receiver;

use crate::app::App;

#[derive(IntoStaticStr, EnumMessage, EnumIter, EnumString)]
#[strum(serialize_all = "lowercase", prefix = "/")]
enum Command {
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
    .unwrap();
    line_editor.set_helper(Some(Helper::default()));
    let mut printer = line_editor.create_external_printer().unwrap();

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
                line_editor
                    .lock()
                    .expect("failed to get lock for line_editor")
                    .add_history_entry(line.clone())
                    .unwrap();
                if let Some(cmd_line) = line.strip_prefix("/") {
                    let (cmd_str, rest) =
                        cmd_line.split_once(" ").unwrap_or((cmd_line, ""));
                    match Command::from_str(cmd_str) {
                        Ok(Command::Help) => help(),
                        Ok(Command::Exit) => break,
                        Ok(Command::Stats) => app.stats().await.unwrap(),
                        Ok(Command::Share) => {
                            app.share(Path::new(rest)).await.unwrap()
                        }
                        Ok(Command::List) => app.list().await.unwrap(),
                        Ok(Command::Fetch) => app.fetch(rest).await.unwrap(),
                        Err(_) => {
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
                } else {
                    // Not a command so send as chat message
                    app.chat(Bytes::copy_from_slice(line.as_bytes()))
                        .await
                        .unwrap_or_else(|err| {
                            println!("Failed to send chat message: {err}")
                        });
                }
            }
            _ => {}
        }
    }
}

#[derive(Default, rustyline::Helper, rustyline::Validator)]
struct Helper {
    #[rustyline(rustyline::Hinter)]
    history_hinter: rustyline::hint::HistoryHinter,
}

impl rustyline::highlight::Highlighter for Helper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        Cow::Owned(format!("\x1b[1;36m{}\x1b[0m", prompt))
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!("\x1b[1;90m{}\x1b[0m", hint))
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
        let candidates = Command::iter()
            .map(Into::<&'static str>::into)
            .filter(|c| c.starts_with(line))
            .map(|c| Self::Candidate {
                display: c.to_string(),
                replacement: c.trim_start_matches(line).to_string(),
            })
            .collect();

        Ok((pos, candidates))
    }
}
