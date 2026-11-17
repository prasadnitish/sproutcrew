use m0::common::{claude_path, now, version_output, Dirs, CODEX};
use m0::ledger::Ledger;
use m0::pins::{version_of, PinCheck};
use m0::{fake, live, nomodel};
use std::path::{Path, PathBuf};

fn dirs(args: &[String]) -> Dirs {
    let repo = std::env::current_dir().expect("cwd");
    let work = args
        .windows(2)
        .find(|w| w[0] == "--work-dir")
        .map(|w| PathBuf::from(&w[1]))
        .unwrap_or_else(|| PathBuf::from(m0::common::home()).join("Library/Caches/bff-m0"));
    Dirs { repo, work }
}

fn pins() -> PinCheck {
    PinCheck {
        claude: version_output(&claude_path())
            .as_deref()
            .and_then(version_of),
        codex: version_output(Path::new(CODEX))
            .as_deref()
            .and_then(version_of),
    }
}

fn print(v: &serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("");
    let d = dirs(&args);
    let result: Result<(), String> = match cmd {
        "__fake-codex" => {
            fake::fake_codex(args.get(2).map(String::as_str).unwrap_or("echo"));
            Ok(())
        }
        "__fake-mcp" => {
            fake::fake_mcp();
            Ok(())
        }
        "pins" => {
            let p = pins();
            println!("{p:?} ok={}", p.ok());
            Ok(())
        }
        "status" => {
            let l = Ledger::load(&d.ledger_path()).map_err(|e| e.to_string());
            l.map(|l| {
                println!("next={:?} stop={:?}", l.next_id(), l.stop_reason());
                for e in l.entries() {
                    println!("{} {:?} {}", e.id, e.status, e.note);
                }
            })
        }
        "run" => match args.get(2) {
            Some(id) => live::execute(&d, id, args.iter().any(|a| a == "--clients-checked"))
                .map(|v| print(&v)),
            None => Err("usage: m0 run <ID> [--clients-checked]".into()),
        },
        "skip" => match (args.get(2), args.get(3)) {
            (Some(id), Some(reason)) => live::skip(&d, id, reason),
            _ => Err("usage: m0 skip <ID> <reason>".into()),
        },
        "clear-stop" => match args.get(2) {
            Some(words) => live::clear_stop(&d, words).map(|r| println!("cleared: {r}")),
            None => Err("usage: m0 clear-stop \"<Nitish's words authorizing it>\"".into()),
        },
        "usage" => {
            let mut stopper = bff_core::stop::Stopper::new();
            m0::telemetry::preflight(&d, &mut stopper).map(|p| {
                print(
                    &serde_json::json!({"claude": p.claude_evidence, "codex": p.codex.evidence,
                    "stop": stopper.reason().map(|r| format!("{r:?}"))}),
                )
            })
        }
        "clients" => {
            print(&live::clients_snapshot());
            Ok(())
        }
        "n1" => nomodel::n1(&d).map(|v| print(&v)),
        "n2" => nomodel::n2(&d).map(|v| print(&v)),
        "n3" => nomodel::n3(&d).map(|v| print(&v)),
        "n4" => nomodel::n4(&d).map(|v| print(&v)),
        "n7" => nomodel::n7(&d).map(|v| print(&v)),
        _ => Err(format!("unknown command {cmd:?}. try: pins status n1 n2")),
    };
    let _ = now();
    if let Err(e) = result {
        eprintln!("m0: {e}");
        std::process::exit(2);
    }
}
