// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Küçük resim alt süreci katalogları açmaz veya ikinci uygulama penceresi oluşturmaz.
    #[cfg(windows)]
    {
        let args: Vec<_> = std::env::args_os().collect();
        if args.get(1).is_some_and(|arg| arg == "--lumiadam-shell-thumbnail") {
            if args.len() != 4 { std::process::exit(2); }
            let result = lumiadam_desktop_lib::windows_thumbnail::generate(
                std::path::Path::new(&args[2]), std::path::Path::new(&args[3]));
            if let Err(error) = result { eprintln!("{error:#}"); std::process::exit(1); }
            return;
        }
    }
    lumiadam_desktop_lib::run();
}

