use handlebars::Handlebars;
use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

fn main() {
    println!("cargo::rerun-if-changed=DOCS.md.hbs");
    println!("cargo::rerun-if-changed=art/dht-graph.svg");
    println!("cargo::rerun-if-changed=art/dht-circle.svg");

    let mut reg = Handlebars::new();
    reg.register_template_file("DOCS.md", "DOCS.md.hbs")
        .expect("Failed to register template");

    let mut data = HashMap::new();
    data.insert("dht-graph", load_svg("art/dht-graph.svg"));
    data.insert("dht-circle", load_svg("art/dht-circle.svg"));

    let mut f =
        std::fs::File::create("DOCS.md").expect("Failed to create DOCS.md");
    reg.render_to_write("DOCS.md", &data, &mut f)
        .expect("Failed to render template");
}

fn load_svg(path: impl AsRef<Path>) -> String {
    std::io::BufReader::new(std::fs::File::open(&path).unwrap_or_else(|e| {
        panic!("Could not load {:?}: {:?}", path.as_ref(), e)
    }))
    .lines()
    .skip(2)
    .fold(String::new(), |acc, line| acc + &line.unwrap() + "\n")
}
