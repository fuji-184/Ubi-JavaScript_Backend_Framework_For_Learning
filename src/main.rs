#![allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    improper_ctypes
)]

use ::std::{
    path::{Path, PathBuf},
    io,
    fs,
    process::Command as StdCommand,
    env,
    io::Write
};
use clap::{Arg, Command};
use include_dir::{include_dir, Dir};
use uuid::Uuid;
use regex::Regex;
use gemini_rs::Conversation;
use lazy_static::lazy_static;
use serde_json::Value;

const CARGO_TOML: &str = include_str!("../project/Cargo.toml");
const MAIN_RS: &str = include_str!("../project/src/main.rs");
const BUILD_RS: &str = include_str!("../project/build.rs");
const CONFIG: &str = include_str!("../project/config.json");
const ROUTES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/routes");
const INDEX_HTML: &str = include_str!("../routes/index.html");

lazy_static! {
    static ref UBI_PATH: PathBuf = PathBuf::from(env::var("HOME").expect("Silahkan set variabel env HOME terlebih dahulu"))
            .join(".ubi");
    static ref PS_PATH: PathBuf = UBI_PATH.join("ps");
}

fn build_ubi(rt: &tokio::runtime::Runtime) -> io::Result<()> {
    let client_dir = Path::new("./routes");
    if !client_dir.exists() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "Routes directory not found"));
    }

    handle_files(client_dir, rt)?;

    Ok(())
}

fn cek_file(path_str: &str) -> bool {
    let path = Path::new(path_str);
    let nama_sesuai = path.file_stem().and_then(|stem| stem.to_str()) == Some("server");
    let ekstensi_sesuai = matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("py") | Some("js") | Some("ts")
        );

    nama_sesuai && ekstensi_sesuai
}

fn handle_files(dir: &Path, rt: &tokio::runtime::Runtime) -> io::Result<()> {
     let entries = fs::read_dir(dir)?;

    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            handle_files(Path::new(&path.to_str().unwrap()), rt)?;
        } else if path.file_name().and_then(|file_name| file_name.to_str()) == Some("ui.ubi") {
            let js_path = Path::new("./.project_build/build").join(path.strip_prefix("./routes").unwrap().to_str().unwrap());
            let content = resolve_imports(&path)?;
            fs::create_dir_all(js_path.parent().unwrap())?;
            fs::write(&js_path, &content)?;

            let main = import_main(&content)?;
            let main_path = js_path.to_str().unwrap().replace("ui.ubi", "index.html");
            fs::write(main_path, &main)?;
        } else if cek_file(path.file_name().and_then(|file_name| file_name.to_str()).unwrap()) {

            let content = fs::read_to_string(&path).unwrap();
            let hasil = rt.block_on(compile(&content));

            let server_path = Path::new("./.project_build/src/server");
            if !server_path.exists() {
                fs::create_dir_all(&server_path).unwrap();
            }
            let file_rust_path = path.with_extension("rs");
            let dest_path = format!("{}/{}", &server_path.to_str().unwrap(), &file_rust_path.strip_prefix("./routes").unwrap().to_str().unwrap());
            let dest_relative = Path::new(&dest_path).parent().unwrap().strip_prefix("./.project_build/src").unwrap();
            let tes = format!("./.project_build/src/server/{}.rs", dest_relative.to_str().unwrap().replace("/", "_").replace("server", "api"));

            let mut file = fs::File::create(&tes).unwrap();
            file.write_all(&hasil.into_bytes()).unwrap();

        }

    }

    Ok(())
}

fn resolve_imports(path: &Path) -> io::Result<String> {
    let mut content = fs::read_to_string(path)?;

    handle_if(&mut content)?;
    handle_array_loops(&mut content)?;
    handle_variables2(&mut content)?;
    handle_variables(&mut content)?;

    let re = regex::Regex::new(r#"<ubi\+\s*"(.*?)">"#).unwrap();

    let mut replacements = Vec::new();
    for captures in re.captures_iter(&content) {
        let import_path = captures.get(1).unwrap().as_str();
        let import_path_full = path.parent().unwrap().join(import_path);
        let import_content = fs::read_to_string(&import_path_full)?;
        replacements.push((
            format!("<ubi+ \"{}\">", import_path.to_string()),
            import_content
        ));
    }

    for (pattern, replacement) in replacements {
        content = content.replace(&pattern, &replacement);
    }

    handle_anchors(&mut content)?;

    Ok(content)
}

fn handle_anchors(content: &mut String) -> io::Result<()> {
    let re = regex::Regex::new(r#"<a\s+[^>]*href\s*=\s*\"([^\"]*)\"[^>]*>"#).unwrap();
    let mut result = String::new();
    let mut last_pos = 0;

    for captures in re.captures_iter(content) {
        let start = captures.get(0).unwrap().start();
        let end = captures.get(0).unwrap().end();

        result.push_str(&content[last_pos..start]);

        let mut href = captures.get(1).unwrap().as_str().to_string();
        href = href.replace(" ", "");

        if href.starts_with("/") {
            let on_click = r#" onClick="handleNavigation(event)""#;
            let replacement = format!(r#"<a href="{}"{}>"#, href, on_click);
            result.push_str(&replacement);
        }

        last_pos = end;
    }

    result.push_str(&content[last_pos..]);
    *content = result;

    Ok(())
}

fn handle_variables(content: &mut String) -> io::Result<()> {
    let script_re = Regex::new(r#"(?s)<script\b[^>]*>(.*?)</script>"#).unwrap();
    let mut script_content = String::new();
    let mut html_content = String::new();
    let mut last_script_pos = 0;

    for script_capture in script_re.captures_iter(content) {
        let start = script_capture.get(0).unwrap().start();
        let end = script_capture.get(0).unwrap().end();
        let script_part = &content[start..end];

        html_content.push_str(&content[last_script_pos..start]);
        script_content.push_str(script_part);
        last_script_pos = end;
    }
    html_content.push_str(&content[last_script_pos..]);

    // let regex_variable = Regex::new(r#"\{\s*([a-zA-Z0-9_]+)\s*\}"#).unwrap();

    let signal_vars: Vec<String> = {
        let signal_re = Regex::new(r#"(?m)^\s*(?:let|var)\s+([a-zA-Z0-9_]+)\s*=\s*new\s+Signal\("#).unwrap();
        signal_re
            .captures_iter(&script_content)
            .filter_map(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
            .collect()
    };

    let mut modified_html = html_content.clone();

    for var_name in signal_vars {
        let pattern = format!(r#"\{{\s*({})\s*\}}"#, regex::escape(&var_name));
        let var_regex = Regex::new(&pattern).unwrap();

        if var_regex.is_match(&modified_html) {
            let new_id = Uuid::new_v4().to_string();
            modified_html = var_regex.replace_all(&modified_html, format!(r#"<p id="{}"></p>"#, new_id)).to_string();

            let effect_code = format!(r#";effect(()=>{{ document.getElementById("{}").innerHTML = {}.get(); }})"#, new_id, var_name);
            if let Some(script_end_pos) = script_content.rfind("</script>") {
                script_content.insert_str(script_end_pos, &effect_code);
            }
        }
    }

    *content = if !script_content.is_empty() {
        format!("{}{}", modified_html, script_content)
    } else {
        modified_html
    };

    Ok(())
}

fn handle_variables2(content: &mut String) -> io::Result<()> {
    let script_re = Regex::new(r#"(?s)<script\b[^>]*>(.*?)</script>"#).unwrap();
    let mut script_content = String::new();
    let mut html_content = String::new();
    let mut last_script_pos = 0;
    for script_capture in script_re.captures_iter(content) {
        let start = script_capture.get(0).unwrap().start();
        let end = script_capture.get(0).unwrap().end();
        let script_part = &content[start..end];
        html_content.push_str(&content[last_script_pos..start]);
        script_content.push_str(script_part);
        last_script_pos = end;
    }
    html_content.push_str(&content[last_script_pos..]);
    let regex_variable = Regex::new(r#"\{\s*([a-zA-Z0-9_]+)\s*->\s*([a-zA-Z0-9_]+)\s*\}"#).unwrap();
    let storage_vars: Vec<String> = {
        let store_re = Regex::new(r#"(?m)^\s*(?:let|var)\s+([a-zA-Z0-9_]+)\s*=\s*new\s+GlobalStore\(\)"#).unwrap();
        store_re
            .captures_iter(&script_content)
            .filter_map(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
            .collect()
    };
    let mut modified_html = html_content.clone();
    for capture in regex_variable.captures_iter(&html_content) {
        if let (Some(storage_var), Some(var_name)) = (capture.get(1), capture.get(2)) {
            let storage_name = storage_var.as_str();
            let var_name = var_name.as_str();
            if storage_vars.iter().any(|s| s == storage_name) {
                let pattern = format!(r#"\{{\s*{}\s*->\s*{}\s*\}}"#, regex::escape(storage_name), regex::escape(var_name));
                let var_regex = Regex::new(&pattern).unwrap();
                let new_id = Uuid::new_v4().to_string();
                modified_html = var_regex.replace_all(&modified_html, format!(r#"<p id="{}"></p>"#, new_id)).to_string();
                let subscribe_code = format!(r#";{}.subscribe("{}", (value)=>{{document.getElementById("{}").innerHTML = value;}});"#,
                    storage_name, var_name, new_id);
                if let Some(script_end_pos) = script_content.rfind("</script>") {
                    script_content.insert_str(script_end_pos, &subscribe_code);
                }
            }
        }
    }
    *content = if !script_content.is_empty() {
        format!("{}{}", modified_html, script_content)
    } else {
        modified_html
    };
    Ok(())
}

fn handle_array_loops(content: &mut String) -> io::Result<()> {
    let script_re = Regex::new(r#"(?s)<script\b[^>]*>(.*?)</script>"#).unwrap();
    let mut script_content = String::new();
    let mut html_content = String::new();
    let mut last_script_pos = 0;

    for script_capture in script_re.captures_iter(content) {
        let start = script_capture.get(0).unwrap().start();
        let end = script_capture.get(0).unwrap().end();
        let script_part = &content[start..end];
        html_content.push_str(&content[last_script_pos..start]);
        script_content.push_str(script_part);
        last_script_pos = end;
    }
    html_content.push_str(&content[last_script_pos..]);

    let loop_re = Regex::new(r#"<\s*loop\s+([a-zA-Z0-9_]+)\s+in\s+([a-zA-Z0-9_]+)\s*>\s*\{\s*([a-zA-Z0-9_]+)\s*\}\s*<\s*/\s*loop\s*>"#).unwrap();
    let array_vars: Vec<(String, String)> = {
        let array_re = Regex::new(r#"(?m)^\s*(?:const|let|var)\s+([a-zA-Z0-9_]+)\s*=\s*\[\s*(.*?)\s*\]"#).unwrap();
        array_re
            .captures_iter(&script_content)
            .map(|cap| (
                cap.get(1).unwrap().as_str().to_string(),
                cap.get(2).unwrap().as_str().to_string()
            ))
            .collect()
    };

    let mut modified_html = html_content.clone();

    for capture in loop_re.captures_iter(&html_content) {
        if let (Some(item_var), Some(array_name), Some(display_var)) = (capture.get(1), capture.get(2), capture.get(3)) {
            let item_name = item_var.as_str().trim();
            let array_name = array_name.as_str().trim();
            let display_name = display_var.as_str().trim();

            if array_vars.iter().any(|(name, _)| name.trim() == array_name) {
                let new_id = Uuid::new_v4().to_string().replace("-", "_");
                let pattern = format!(r#"<\s*loop\s+{}\s+in\s+{}\s*>\s*\{{\s*{}\s*\}}\s*<\s*/\s*loop\s*>"#,
                    regex::escape(item_name),
                    regex::escape(array_name),
                    regex::escape(display_name)
                );
                let loop_regex = Regex::new(&pattern).unwrap();

                modified_html = loop_regex.replace_all(&modified_html, format!(r#"<p id="{}"></p>"#, new_id)).to_string();

                let array_code = format!(r#";const arrayList_{0} = document.getElementById("{1}"); {2}.forEach({3} => {{ const listItem = document.createElement("p"); listItem.textContent = {4}; arrayList_{0}.appendChild(listItem); }});"#,
                    new_id.replace("-", "_"),
                    new_id,
                    array_name,
                    item_name,
                    display_name
                );

                if let Some(script_end_pos) = script_content.rfind("</script>") {
                    script_content.insert_str(script_end_pos, &array_code);
                }
            }
        }
    }

    *content = if !script_content.is_empty() {
        format!("{}{}", modified_html, script_content)
    } else {
        modified_html
    };

    Ok(())
}


fn handle_if(content: &mut String) -> io::Result<()> {
    let script_re = Regex::new(r#"(?s)<script\b[^>]*>(.*?)</script>"#).unwrap();
    let mut script_content = String::new();
    let mut html_content = String::new();
    let mut last_script_pos = 0;
    for script_capture in script_re.captures_iter(content) {
        let start = script_capture.get(0).unwrap().start();
        let end = script_capture.get(0).unwrap().end();
        let script_part = &content[start..end];
        html_content.push_str(&content[last_script_pos..start]);
        script_content.push_str(script_part);
        last_script_pos = end;
    }
    html_content.push_str(&content[last_script_pos..]);

    let regex_variable = Regex::new(r#"\{\s*([a-zA-Z0-9_]+)\s*->\s*([a-zA-Z0-9_]+)\s*\}"#).unwrap();

    let storage_vars: Vec<String> = {
        let store_re = Regex::new(r#"(?m)^\s*(?:let|var)\s+([a-zA-Z0-9_]+)\s*=\s*new\s+GlobalStore\(\)"#).unwrap();
        store_re
            .captures_iter(&script_content)
            .filter_map(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
            .collect()
    };

    let mut modified_html = html_content.clone();

    for capture in regex_variable.captures_iter(&html_content) {
        if let (Some(storage_var), Some(var_name)) = (capture.get(1), capture.get(2)) {
            let storage_name = storage_var.as_str();
            let var_name = var_name.as_str();

            if storage_vars.iter().any(|s| s == storage_name) {
                let pattern = format!(r#"\{{\s*{}\s*->\s*{}\s*\}}"#, regex::escape(storage_name), regex::escape(var_name));
                let var_regex = Regex::new(&pattern).unwrap();

                let new_id = Uuid::new_v4().to_string();
                modified_html = var_regex.replace_all(&modified_html, format!(r#"<p id="{}"></p>"#, new_id)).to_string();

                let subscribe_code = format!(r#";{}.subscribe("{}", (value)=>{{document.getElementById("{}").innerHTML = value;}});"#,
                    storage_name, var_name, new_id);

                if let Some(script_end_pos) = script_content.rfind("</script>") {
                    script_content.insert_str(script_end_pos, &subscribe_code);
                }
            }
        }
    }

    let if_else_re = Regex::new(r#"<if\s+([a-zA-Z0-9_]+)\s*===\s*\"([^\"]+)\"\s*>(.*?)</if>"#).unwrap();
    modified_html = if_else_re.replace_all(&modified_html, |caps: &regex::Captures| {
        let var_name = caps.get(1).unwrap().as_str();
        let expected_value = caps.get(2).unwrap().as_str();
        let content = caps.get(3).unwrap().as_str();

        let condition_met = storage_vars.iter().any(|s| s == var_name && s == expected_value);
        if condition_met {
            content.to_string()
        } else {
            "".to_string()
        }
    }).to_string();

    *content = if !script_content.is_empty() {
        format!("{}{}", modified_html, script_content)
    } else {
        modified_html
    };
    Ok(())
}

fn import_main(new_code: &str) -> io::Result<String> {
    let mut content = INDEX_HTML;
    let binding = content.replace("<ubi:main>", new_code);
    content = &binding;
    let mut content_string = content.to_string();
    handle_anchors(&mut content_string)?;
    Ok(content_string)
}

fn init_ubi(project_name: &str) -> io::Result<()> {
    let project_path = Path::new(project_name);
    let routes_path = project_path.join("routes");
    let static_path = project_path.join("static");
    fs::create_dir_all(&routes_path)?;
    fs::create_dir_all(&static_path)?;

    let config_path = project_path.join("config.json");
    let mut config = fs::File::create(&config_path).expect("Failed to init project");
    config.write_all(CONFIG.as_bytes()).expect("Failed to init project");
    let _ = set_json_name(project_name, project_path.join("config.json").to_str().unwrap());

    for file in ROUTES_DIR.files() {
        let path = Path::new(file.path());
        let target_path = routes_path.join(path);

         if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target_path, file.contents())?;
    }

    Ok(())
}

fn generate_mod_rs(server_dir: &str) {
    let mod_file = format!("{}/mod.rs", server_dir);
    let mut content = String::new();

    let entries = find_server_files(Path::new(server_dir));

    if entries.is_empty() {
        return;
    }

    for entry in entries {
        if let Ok(relative_path) = entry.strip_prefix(server_dir) {
            let mod_name = relative_path
                .file_name().unwrap().to_str().unwrap();

            content.push_str(&format!("pub mod {};\n", mod_name));
        }
    }

    fs::write(mod_file, content).expect("Gagal menulis mod.rs");
}

fn find_server_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(find_server_files(&path));
            } else if path.extension().unwrap() == "rs" {
                files.push(path);
            }
        }
    }
    files
}
async fn compile(code: &str) -> String {
    let mut convo = Conversation::new(
        "".to_string(),
        "gemini-2.0-flash".to_string()
    );
    let a = convo.prompt(&format!(r#"convert kode ini ke rust, untuk jsonnya pakai serde, jadikan 1 filde kode, tanpa fn main, bukan async, semua namanya samakan, buat semuanya public, hindary pakai unwrap, return Result<String, Box<dyn std::error::Error>, tambahkan import crate::PgConnection jangan buat manual cukup tambahkan itu saja karena sudah aku buat di file main kamu tinggal pakai, dependensi 3rd librarynya hanya serde dan serde_json, jika ada fungsi yang bernama (get, post, update, delete) di kodenya maka tambahkan parameter db: &PgConnection ingat jangan ditambahin fungsi sendiri kalau emang ga ada yaudah biarkan ga ada, jika ada fungsi ubi.query() maka ubah menjadi db.query()? parameternya 1 tipe string literal jangan diubah menjadi lainnya lalu proses hasil returnnya seperti berikut
    ikutin pola cara meangambil hasil query ini jangan bikin pola sendiri :
        misal let rows = db.query("select * from data")? cara ambilnya gini
        let all_rows = Vec::from_iter(rows.map(|r| r.unwrap()));
            let mut vektor = Vec::with_capacity(all_rows.len());
            vektor.extend(all_rows.iter().map(|r| Nama_Struct {{
                nama kolom: r.get(0),
                nama_kolom: r.get(1),

            }}));
 jangan ubah-ubah method pola di atas karena itu udah dikonfigurasikan seperti itu, hanya ubah dobel {{ }} nya jadi 1 aja lalu nama struct dan kolomnya sama dengan yang ada di kode asli, nurut sama aku buat seperti perintahnya, sisanya buat sama persis dengan kode aslinya, jangan nambah-nambahin sendiri sesuatu yang ga ada di kode aslinya, jangan diberi komentar, langsung berikan kodenya tanpa ada kata pengantar atau penjelasan apapun, langsung kodenya saja karena akan langsung ditempel di kode aplikasinya: {}"#, code)).await;
    a.replace("```rust", "").replace("```", "")
}

fn set_name(nama_baru: &str, cargo_path: &str) -> io::Result<()> {
    let isi = fs::read_to_string(cargo_path)?;

    let isi_baru = isi
        .lines()
            .map(|line| {
            if line.starts_with("name = ") {
                format!("name = \"{}\"", nama_baru)
            } else {
                line.to_string()
                }
        })
                .collect::<Vec<String>>()
            .join("\n");

    fs::write(cargo_path, isi_baru)?;
    Ok(())
}

fn set_json_name(nama_baru: &str, config_path: &str) -> io::Result<()> {
    let isi = fs::read_to_string(config_path)?;

    let mut json: Value = serde_json::from_str(&isi)?;
    if let Some(obj) = json.as_object_mut() {
        if let Some(name) = obj.get_mut("name") {
            *name = Value::String(nama_baru.to_string());
        }
    }

    let isi_baru = serde_json::to_string_pretty(&json)?;
    fs::write(config_path, isi_baru)?;
    Ok(())
}

fn get_json_name(config_path: &str) -> Option<String> {
    let isi = fs::read_to_string(config_path).ok()?;

    let json: Value = serde_json::from_str(&isi).ok()?;
    json.get("name")?.as_str().map(|s| s.to_string())
}

fn main() -> io::Result<()> {
    let matches = Command::new("ubi")
        .version("1.0")
        .author("Fuji <fujisantoso134@gmail.com>")
        .about("Create JavaScript backend easily")
        .subcommand(
            Command::new("init")
                .about("Initialize Ubi project")
                .arg(
                    Arg::new("project_name")
                        .value_name("PROJECT_NAME")
                        .required(true)
                        .help("The name of the project"),
                ),
        )
        .subcommand(Command::new("setup").about("Configure Ubi environment"))
        .subcommand(Command::new("build").about("Build Ubi project"))
        .get_matches();

        let mut project_name = String::new();

        let _ = match matches.subcommand_name() {
            Some("init") => {
                if let Some(init_matches) = matches.subcommand_matches("init") {
                    project_name = init_matches.get_one::<String>("project_name").unwrap().to_string();
                    let _ = init_ubi(&project_name);
                    println!("Project {} has been initialized, let's go coding ^^", project_name);
                }
            },
            Some("build") => {
                println!("Compiling project... (first time compile might be slow, please wait...)");
                let current_dir = env::current_dir().unwrap().join(".project_build");
                let project_build_dir =  env::current_dir().unwrap().join(".project_build");
                let libs_build_dir = project_build_dir.clone().join("libs");
                let final_build_dir =  env::current_dir().unwrap().join("build");
                // let routes_dir = env::current_dir().unwrap().join("routes");
                let rt = tokio::runtime::Runtime::new().unwrap();

                if current_dir.exists() {
                    fs::remove_dir_all(&current_dir).expect("Gagal menghapus direktori lama");
                }
                if project_build_dir.exists() {
                    fs::remove_dir_all(&project_build_dir).expect("Gagal menghapus direktori lama");
                }

                if final_build_dir.exists() {
                    fs::remove_dir_all(&final_build_dir).expect("Gagal menghapus direktori lama");
                }

                fs::create_dir_all(current_dir.join("src/server")).expect("Compiling failed");
                fs::create_dir_all(&libs_build_dir).expect("Compiling failed");
                fs::create_dir_all(&final_build_dir).expect("Compiling failed");

                let _ = build_ubi(&rt);

                StdCommand::new("cp")
                    .arg("./config.json")
                    .arg("./.project_build")
                    .output()
                    .expect("Compiling failed");

                generate_mod_rs("./.project_build/src/server");

                let cargo_path = current_dir.join("Cargo.toml");
                let mut cargo_file = fs::File::create(&cargo_path).expect("Compiling failed");
                cargo_file.write_all(CARGO_TOML.as_bytes()).expect("Compiling failed");
                let name = get_json_name("./.project_build/config.json").unwrap();
                let _ = set_name(&name,  &cargo_path.to_str().unwrap()).unwrap();

                let mut main_file = fs::File::create(current_dir.join("src/main.rs")).expect("Compiling failed");
                main_file.write_all(MAIN_RS.as_bytes()).expect("Compiling failed");

                let mut build_file = fs::File::create(current_dir.join("build.rs")).expect("Compiling failed");
                build_file.write_all(BUILD_RS.as_bytes()).expect("Compiling failed");

                let _ = env::set_current_dir(&project_build_dir);

                StdCommand::new("cargo")
                    .arg("build")
                    .arg("--release")
                    .current_dir(&project_build_dir)
                    .output()
                    .expect("Compiling failed");

                StdCommand::new("cp")
                    .arg("-r")
                    .arg("../static")
                    .arg("../build")
                    .current_dir(env::current_dir().unwrap())
                    .output()
                    .expect("Compiling failed");

                StdCommand::new("mv")
                    .arg(format!("./target/release/{}", name))
                    .arg("../build")
                    .current_dir(env::current_dir().unwrap())
                    .output()
                    .expect("Compiling failed");
                println!("Yeayy, The project has been built in the build folder");
            },
            _ => println!("invalid command!")
        };

        Ok(())
}
