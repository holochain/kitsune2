use std::borrow::Cow;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

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

    let mut line_editor =
        <rustyline::Editor<H, rustyline::history::MemHistory>>::with_history(
            rustyline::Config::builder().build(),
            rustyline::history::MemHistory::new(),
        )
        .unwrap();
    line_editor.set_helper(Some(H));
    let mut p = line_editor.create_external_printer().unwrap();
    let p: DynPrinter = Box::new(move |s| {
        use rustyline::ExternalPrinter;
        p.print(s).unwrap();
    });
    *print.0.lock().unwrap() = Some(p);

    // loop over input lines
    while let Ok(line) = line_editor.readline(&prompt) {
        match line
            .strip_prefix("/")
            .and_then(|s| Command::from_str(s).ok())
        {
            Some(Command::Help) => help(),
            Some(Command::Exit) => break,
            Some(Command::Share | Command::List | Command::Fetch) => {
                println!("NOT IMPLEMENTED");
            }
            Some(Command::Stats) | None => {
                if lines.blocking_send(line).is_err() {
                    break;
                }
            }
        }
    }
}

#[derive(rustyline::Helper, rustyline::Validator)]
struct H;

impl rustyline::highlight::Highlighter for H {
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

struct Hint(&'static str);

impl rustyline::hint::Hint for Hint {
    fn display(&self) -> &str {
        self.0
    }

    fn completion(&self) -> Option<&str> {
        Some(self.0)
    }
}

impl rustyline::hint::Hinter for H {
    type Hint = Hint;

    fn hint(
        &self,
        line: &str,
        _pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> Option<Self::Hint> {
        if line.len() < 2 {
            return None;
        }
        for c in Command::iter().map(Into::<&'static str>::into) {
            if c.starts_with(line) {
                return Some(Hint(c.trim_start_matches(line)));
            }
        }
        None
    }
}

pub struct Candidate(&'static str);

impl rustyline::completion::Candidate for Candidate {
    fn display(&self) -> &str {
        self.0
    }

    fn replacement(&self) -> &str {
        self.0
    }
}

impl rustyline::completion::Completer for H {
    type Candidate = Candidate;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        let mut out = Vec::new();
        if line.len() < 2 {
            return Ok((pos, out));
        }
        for c in Command::iter().map(Into::<&'static str>::into) {
            if c.starts_with(line) {
                out.push(Candidate(c.trim_start_matches(line)));
            }
        }
        Ok((pos, out))
    }
}
