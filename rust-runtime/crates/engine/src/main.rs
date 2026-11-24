mod audio;
mod download;
mod enhancement;
mod models;
mod recognition;
mod server;
use light_whisper_native_asr::Family;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "Usage: engine <serve|download> --engine <qwen3-asr-0.6b|confucius4-r2t2>";
    if args == ["--help"] || args == ["-h"] {
        println!("{usage}");
        return 0.into();
    }
    let [action, flag, name] = args.as_slice() else {
        eprintln!("{usage}");
        return 2.into();
    };
    let family = match name.as_str() {
        "qwen3-asr-0.6b" => Family::Qwen,
        "confucius4-r2t2" => Family::R2t2,
        _ => {
            eprintln!("{usage}");
            return 2.into();
        }
    };
    if flag != "--engine" {
        eprintln!("{usage}");
        return 2.into();
    }
    let result = match action.as_str() {
        "serve" => server::run(name, family),
        "download" => {
            let _ = download::run(family);
            Ok(())
        }
        _ => {
            eprintln!("{usage}");
            return 2.into();
        }
    };
    match result {
        Ok(()) => 0.into(),
        Err(e) => {
            eprintln!("{e}");
            1.into()
        }
    }
}
