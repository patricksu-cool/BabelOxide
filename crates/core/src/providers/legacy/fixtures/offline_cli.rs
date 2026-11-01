use std::{
    env,
    fs::{self, OpenOptions},
    io::{Read, Write},
    process::{self, Command},
    thread,
    time::Duration,
};

fn publish_pid(root: &std::path::Path, name: &str) {
    let temporary = root.join(format!("{name}.tmp"));
    fs::write(&temporary, process::id().to_string()).unwrap();
    fs::rename(temporary, root.join(name)).unwrap();
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let exe = env::current_exe().unwrap();
    let root = exe.parent().unwrap();
    if args.first().map(String::as_str) == Some("--grandchild") {
        publish_pid(root, "descendant.pid");
        thread::sleep(Duration::from_secs(300));
        return;
    }
    let mode = if args.first().map(String::as_str) == Some("--version") {
        "version"
    } else if let Some(i) = args.iter().position(|arg| arg == "--model") {
        &args[i + 1]
    } else {
        args.first().map(String::as_str).unwrap_or("echo")
    };
    let mut calls = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("calls.log"))
        .unwrap();
    writeln!(calls, "{} {}", process::id(), mode).unwrap();
    drop(calls);
    if mode == "version" {
        println!("fixture 1.0");
        return;
    }
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input).unwrap();
    fs::write(root.join(format!("input-{}.bin", process::id())), &input).unwrap();
    fs::write(
        root.join(format!("args-{}.txt", process::id())),
        args.join("\n"),
    )
    .unwrap();
    match mode {
        "sleep" | "claude-sleep" | "tree" => {
            publish_pid(root, "child.pid");
            let _descendant = if mode == "tree" {
                Some(Command::new(&exe).arg("--grandchild").spawn().unwrap())
            } else {
                None
            };
            thread::sleep(Duration::from_secs(300));
        }
        "fail" | "claude-fail" => {
            eprintln!("fixture-key source text https://fixture.invalid/private");
            process::exit(7);
        }
        "flaky" => {
            let count = fs::read_to_string(root.join("calls.log"))
                .unwrap()
                .lines()
                .filter(|line| line.ends_with(" flaky"))
                .count();
            if count == 1 {
                process::exit(9);
            }
            std::io::stdout().write_all(&input).unwrap();
        }
        "post" => print!("{}", String::from_utf8_lossy(&input).to_uppercase()),
        "claude-success" => {
            println!("fixture banner");
            println!("{{invalid");
            println!(
                "{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\" 譯\"}},{{\"type\":\"tool_use\"}}]}}}}"
            );
            println!("{{\"type\":\"text\",\"text\":\"文 \"}}");
            println!("{{\"type\":\"result\",\"result\":\"must not duplicate\"}}");
        }
        "claude-empty" => println!("{{\"type\":\"result\",\"result\":\"ignored\"}}"),
        _ => std::io::stdout().write_all(&input).unwrap(),
    }
}
