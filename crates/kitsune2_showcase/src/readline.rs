/// Ability to print to stdout while readline is happening.
#[derive(Clone)]
pub struct Print(reedline::ExternalPrinter<String>);

impl Print {
    /// Print out a line.
    pub async fn print_line(&self, line: String) -> Result<(), ()> {
        let p = self.0.clone();
        tokio::task::spawn_blocking(move || p.print(line).map_err(|_| ()))
            .await
            .map_err(|_| ())?
    }
}

impl Default for Print {
    fn default() -> Self {
        Self(reedline::ExternalPrinter::new(4096))
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

    // setup a default prompt using the supplied nickname
    let prompt = reedline::DefaultPrompt {
        left_prompt: reedline::DefaultPromptSegment::Basic(nick),
        right_prompt: reedline::DefaultPromptSegment::Empty,
    };

    // setup command highlighting
    let mut cmds: Vec<String> =
        command_list.iter().map(|(c, _)| c.to_string()).collect();
    for (c, _) in COMMAND_LIST {
        cmds.push(c.to_string());
    }
    let mut highlighter = reedline::ExampleHighlighter::new(cmds);
    highlighter.change_colors(
        nu_ansi_term::Color::Cyan,
        nu_ansi_term::Color::Default,
        nu_ansi_term::Color::Default,
    );

    // this is the actual "reedline" struct
    let mut line_editor = reedline::Reedline::create()
        .with_highlighter(Box::new(highlighter))
        .with_external_printer(print.0);

    loop {
        // get the next line
        let sig = line_editor.read_line(&prompt);

        // parse it
        match sig {
            Ok(reedline::Signal::Success(buffer)) => {
                if buffer.starts_with("/help") {
                    help(command_list);
                } else if buffer.starts_with("/exit") {
                    break;
                } else {
                    if lines.blocking_send(buffer).is_err() {
                        break;
                    }
                }
            }
            Ok(reedline::Signal::CtrlD) | Ok(reedline::Signal::CtrlC) => {
                break;
            }
            _ => (),
        }
    }
}
