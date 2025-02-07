use std::borrow::Cow;
use std::sync::{Arc, Mutex};

type DynPrinter = Box<dyn FnMut(String) + 'static + Send>;
type SyncPrinter = Arc<Mutex<Option<DynPrinter>>>;

/// Ability to print to stdout while readline is happening.
#[derive(Clone)]
pub struct Print(SyncPrinter);

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

const COMMAND_LIST: &[(&str, &str)] =
    &[("/help", "print this help"), ("/exit", "quit this program")];

/// Print out command help.
fn help(command_list: &[(&str, &str)]) {
    println!("\n# Kitsune2 Showcase chat and file sharing app\n");
    for (cmd, help) in command_list {
        println!("{cmd} - {help}");
    }
    for (cmd, help) in COMMAND_LIST {
        println!("{cmd} - {help}");
    }
}

/// Blocking loop waiting for user input / handling commands.
pub fn readline(
    nick: String,
    command_list: &'static [(&'static str, &'static str)],
    print: Print,
    lines: tokio::sync::mpsc::Sender<String>,
) {
    // print command help
    help(command_list);

    let prompt = format!("{nick}> ");

    let mut line_editor =
        <rustyline::Editor<H, rustyline::history::MemHistory>>::with_history(
            rustyline::Config::builder().build(),
            rustyline::history::MemHistory::new(),
        )
        .unwrap();
    line_editor.set_helper(Some(H(command_list)));
    let mut p = line_editor.create_external_printer().unwrap();
    let p: DynPrinter = Box::new(move |s| {
        use rustyline::ExternalPrinter;
        p.print(s).unwrap();
    });
    *print.0.lock().unwrap() = Some(p);

    // loop over input lines
    while let Ok(line) = line_editor.readline(&prompt) {
        if line.starts_with("/help") {
            help(command_list);
            continue;
        } else if line.starts_with("/exit") {
            break;
        }

        if lines.blocking_send(line).is_err() {
            break;
        }
    }
}

#[derive(rustyline::Helper, rustyline::Validator)]
struct H(&'static [(&'static str, &'static str)]);

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
        for (c, _) in self.0 {
            if c.starts_with(line) {
                return Some(Hint(c.trim_start_matches(line)));
            }
        }
        for (c, _) in COMMAND_LIST {
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
        for (c, _) in self.0 {
            if c.starts_with(line) {
                out.push(Candidate(c.trim_start_matches(line)));
            }
        }
        for (c, _) in COMMAND_LIST {
            if c.starts_with(line) {
                out.push(Candidate(c.trim_start_matches(line)));
            }
        }
        Ok((pos, out))
    }
}
