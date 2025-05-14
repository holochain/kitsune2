use std::borrow::Cow;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use rustyline::error::ReadlineError;
use strum::{
    EnumIter, EnumMessage, EnumString, IntoEnumIterator, IntoStaticStr,
};

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

type DynPrinter = Box<dyn FnMut(String) + 'static + Send>;
type SyncPrinter = Arc<Mutex<Option<DynPrinter>>>;

/// Ability to print to stdout while readline is happening.
#[derive(Clone)]
pub struct Print(SyncPrinter);

impl std::fmt::Debug for Print {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Print").finish()
    }
}

impl Print {
    /// Print out a line.
    pub fn print_line(&self, line: String) {
        match self.0.lock().unwrap().as_mut() {
            Some(printer) => printer(line),
            None => println!("{line}"),
        }
    }
}

impl Default for Print {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }
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
pub fn readline(
    nick: String,
    print: Print,
    lines: tokio::sync::mpsc::Sender<String>,
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
    let mut p = line_editor.create_external_printer().unwrap();
    let p: DynPrinter = Box::new(move |s| {
        use rustyline::ExternalPrinter;
        p.print(s).unwrap();
    });
    *print.0.lock().unwrap() = Some(p);

    // loop over input lines
    loop {
        match line_editor.readline(&prompt) {
            Err(ReadlineError::Eof) => break,
            Err(ReadlineError::Interrupted) => println!("^C"),
            Err(err) => {
                eprintln!("Failed to read line: {err}");
                break;
            }
            Ok(line) => {
                line_editor.add_history_entry(line.clone()).unwrap();
                if let Some(cmd_str) = line.strip_prefix("/") {
                    match Command::from_str(cmd_str) {
                        Ok(Command::Help) => help(),
                        Ok(Command::Exit) => break,
                        Ok(Command::Share | Command::List | Command::Fetch) => {
                            println!("NOT IMPLEMENTED");
                        }
                        Ok(Command::Stats) => {
                            if lines.blocking_send(line).is_err() {
                                break;
                            }
                        }
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
                } else if lines.blocking_send(line).is_err() {
                    break;
                }
            }
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
