//! Verification harness (not product code).
//!
//! The capability no longer grants `dialog:default`. The reasoning is that
//! this application opens its dialogs from Rust, which does not cross the
//! ACL — but reasoning is not observation, and an export that cannot open a
//! dialog is an export that silently does nothing. So: open one.

use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;

use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

struct Reported(Mutex<Sender<String>>);

fn main() {
    let (tx, rx) = channel::<String>();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Reported(Mutex::new(tx.clone())))
        .setup(move |app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1200));
                let tx = handle.state::<Reported>().0.lock().unwrap().clone();

                // The same call `export_trace` makes.
                handle
                    .dialog()
                    .file()
                    .set_file_name("trace.csv")
                    .save_file(move |picked| {
                        let _ = tx.send(format!("the dialog answered: {picked:?}"));
                    });

                // The dialog is modal to the user, not to us: what is being
                // checked is that the call was allowed to open one at all.
                match rx.recv_timeout(std::time::Duration::from_millis(2500)) {
                    Ok(answer) => println!("DIALOG: {answer}"),
                    Err(_) => println!("DIALOG: opened and left waiting for the user"),
                }
                std::process::exit(0);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the harness failed to start");
}
