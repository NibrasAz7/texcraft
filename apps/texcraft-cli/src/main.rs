//! texcraft-cli — headless TexCraft.
//!
//! M0 surface (grows into `run`/`--cmd`/`mcp` once the engine lands):
//!   texcraft-cli parse <file.tex>   document structure outline
//!   texcraft-cli check <file.tex>   structural issues only; exit 1 if any
//!   texcraft-cli bib <file.bib>     bibliography entries and issues

use std::process::ExitCode;

use texparse::{parse_bib, parse_structure};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("parse") => with_file(args.get(1), parse_file),
        Some("check") => with_file(args.get(1), check_file),
        Some("bib") => with_file(args.get(1), bib_file),
        _ => {
            usage();
            if args.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
    }
}

fn usage() {
    eprintln!(
        "texcraft-cli — headless TexCraft\n\
         \n\
         usage:\n\
         \x20 texcraft-cli parse <file.tex>   document structure outline\n\
         \x20 texcraft-cli check <file.tex>   structural issues (exit 1 if any)\n\
         \x20 texcraft-cli bib   <file.bib>   bibliography entries\n"
    );
}

fn with_file(arg: Option<&String>, f: fn(&str, &str) -> ExitCode) -> ExitCode {
    let Some(path) = arg else {
        usage();
        return ExitCode::FAILURE;
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("texcraft-cli: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let src = String::from_utf8_lossy(&bytes);
    f(path, &src)
}

const LEVEL_NAMES: [&str; 7] = ["part", "chapter", "section", "subsection", "subsubsection", "paragraph", "subparagraph"];

fn parse_file(_path: &str, src: &str) -> ExitCode {
    let d = parse_structure(src);
    if let Some((class, _)) = &d.class {
        if d.class_options.is_empty() {
            println!("class: {class}");
        } else {
            println!("class: {class} [{}]", d.class_options);
        }
    }
    if !d.packages.is_empty() {
        println!("packages: {}", d.packages.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>().join(", "));
    }
    if let Some((t, _)) = &d.title {
        println!("title: {t}");
    }
    if let Some((a, _)) = &d.author {
        println!("author: {a}");
    }
    for h in &d.headings {
        let name = LEVEL_NAMES.get(h.level as usize).copied().unwrap_or("?");
        println!("{}:{}: {} {}", h.pos.line, h.pos.col, name, h.title);
    }
    if !d.labels.is_empty() {
        println!("labels: {}", d.labels.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>().join(", "));
    }
    if !d.cite_keys.is_empty() {
        println!("cites: {}", d.cite_keys.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", "));
    }
    if !d.inputs.is_empty() {
        println!("inputs: {}", d.inputs.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>().join(", "));
    }
    if !d.graphics.is_empty() {
        println!("graphics: {}", d.graphics.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>().join(", "));
    }
    if !d.bibliographies.is_empty() {
        println!("bibliography: {}", d.bibliographies.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>().join(", "));
    }
    for i in &d.issues {
        println!("issue {}:{}: {}", i.pos.line, i.pos.col, i.message);
    }
    ExitCode::SUCCESS
}

fn check_file(_path: &str, src: &str) -> ExitCode {
    let d = parse_structure(src);
    for i in &d.issues {
        println!("{}:{}: {}", i.pos.line, i.pos.col, i.message);
    }
    if d.issues.is_empty() {
        println!("ok");
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn bib_file(_path: &str, src: &str) -> ExitCode {
    let f = parse_bib(src);
    for (name, val) in &f.strings {
        println!("@string {name} = \"{val}\"");
    }
    for e in &f.entries {
        if e.key.is_empty() {
            println!("@{} (empty)", e.kind);
            continue;
        }
        let mut fields = e.fields.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ");
        if fields.is_empty() {
            fields = "no fields".into();
        }
        println!("{}:{}: @{} {{{}}} — {}", e.pos.line, e.pos.col, e.kind, e.key, fields);
    }
    for i in &f.issues {
        println!("issue {}:{}: {}", i.pos.line, i.pos.col, i.message);
    }
    ExitCode::SUCCESS
}
