use std::{fs, path::PathBuf, collections::HashMap};

fn main() -> std::io::Result<()> {
    let server_dir = PathBuf::from("src/server");
    let mod_path = server_dir.join("mod.rs");

    let mut routes = Vec::new();
    let mut modules = Vec::new();

    // Cari semua file `.rs` dalam `server/`
    for entry in fs::read_dir(&server_dir)? {
        let entry = entry?;
        let path = entry.path();

        // Abaikan `mod.rs`
        if path.is_file() && path.extension().unwrap_or_default() == "rs" {
            if let Some(file_name) = path.file_stem().and_then(|s| s.to_str()) {
                if file_name != "mod" {
                    // Tambahkan module dan route
                    modules.push(format!("pub mod {file_name};"));
                    routes.push(format!("(\"/{file_name}\", {file_name}::get as HandlerFn)"));
                }
            }
        }
    }

    // Generate kode untuk `mod.rs`
    let generated_code = format!(
        r#"
use std::collections::HashMap;
use lazy_static::lazy_static;
use crate::PgConnection;

{}

pub type HandlerFn = fn(&PgConnection) -> Result<String, Box<dyn std::error::Error>>;

lazy_static! {{
    pub static ref ROUTES: HashMap<&'static str, HandlerFn> = {{
        let mut map = HashMap::new();
        {};
        map
    }};
}}
"#,
        modules.join("\n"),
        routes.iter()
              .map(|route| format!("map.insert{};", route))
              .collect::<Vec<_>>()
              .join("\n        ")
    );

    // Tulis langsung ke `server/mod.rs`
    fs::write(&mod_path, generated_code)?;
    println!("Generated server/mod.rs");

    Ok(())
}
