#![allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    improper_ctypes
)]

use ::std::{
    env, fs,
    fs::File,
    io,
    io::Write,
    path::{Path, PathBuf},
    process::{Command as StdCommand, Stdio},
    os::unix::fs as unix_fs,
};
use clap::{Arg, Command};
use include_dir::{include_dir, Dir};
use lazy_static::lazy_static;
use regex::Regex;
use serde_json::Value;
use uuid::Uuid;
use walkdir::WalkDir;
// use compact_str::{format_compact, CompactString, ToCompactString};

const CARGO_TOML: &str = include_str!("../project/Cargo.toml");
const MAIN_RS: &str = include_str!("../project/src/main.rs");
const CONFIG: &str = include_str!("../project/config.json");
const ROUTES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/routes");
const INDEX_HTML: &str = include_str!("../libs/index.html");
const DPRINT_CONFIG: &str = include_str!("../dprint.json");
const CB: &[u8] = include_bytes!("../libs/cb");
const PN: &[u8] = include_bytes!("../libs/pn");
const DP: &[u8] = include_bytes!("../libs/dp");

lazy_static! {
    static ref UBI_PATH: PathBuf =
        PathBuf::from(env::var("HOME").expect("Silahkan set variabel env HOME terlebih dahulu"))
            .join(".ubi");
    static ref PS_PATH: PathBuf = UBI_PATH.join("ps");
}

fn build_ubi() -> io::Result<()> {
    StdCommand::new("cp")
        .arg("-r")
        .arg("./routes")
        .arg("./.project_build")
        .output()
        .expect("Compiling failed");

    let client_dir = Path::new("./.project_build/routes");
    if !client_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Routes directory not found",
        ));
    }

    handle_files(client_dir)?;

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

fn handle_files(dir: &Path) -> io::Result<()> {
    let entries = fs::read_dir(dir)?;

    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            handle_files(Path::new(&path.to_str().unwrap()))?;
        } else if path.file_name().and_then(|file_name| file_name.to_str()) == Some("ui.ubi") {
            let js_path = Path::new("./.project_build/build").join(
                path.strip_prefix("./.project_build/routes")
                    .unwrap()
                    .to_str()
                    .unwrap(),
            );
            let mut content = resolve_imports(&path)?;
            fs::create_dir_all(js_path.parent().unwrap())?;
            content = convert_ubi(&content, &path).expect("failed to compile");
            fs::write(js_path.with_extension("html"), &content)?;

            let main = import_main(&content)?;
            let main_path = js_path.to_str().unwrap().replace("ui.ubi", "index.html");
            fs::write(main_path, &main)?;
        } else if cek_file(
            path.file_name()
                .and_then(|file_name| file_name.to_str())
                .unwrap(),
        ) {
            let server_path = Path::new("./.project_build/src/server");
            if !server_path.exists() {
                fs::create_dir_all(server_path).unwrap();
            }
            let file_rust_path = path.with_extension("rs");
            let dest_path = format!(
                "{}/{}",
                &server_path.to_str().unwrap(),
                &file_rust_path
                    .strip_prefix("./.project_build/routes")
                    .unwrap()
                    .to_str()
                    .unwrap()
            );
            let dest_relative = Path::new(&dest_path)
                .parent()
                .unwrap()
                .strip_prefix("./.project_build/src")
                .unwrap();
            let tes = format!(
                "./.project_build/src/server/{}.rs",
                dest_relative
                    .to_str()
                    .unwrap()
                    .replace("/", "_")
                    .replace("server", "api")
            );

            process_file(path.to_str().unwrap(), &tes).expect("Compilation failed");
        }
    }

    Ok(())
}

fn resolve_imports(path: &Path) -> io::Result<String> {
    let mut content = fs::read_to_string(path)?;

    let re = regex::Regex::new(r#"<ubi\+\s*"(.*?)">"#).unwrap();

    let mut replacements = Vec::new();
    for captures in re.captures_iter(&content) {
        let import_path = captures.get(1).unwrap().as_str();
        let import_path_full = path.parent().unwrap().join(import_path);
        let import_content = fs::read_to_string(&import_path_full)?;
        replacements.push((format!("<ubi+ \"{}\">", import_path), import_content));
    }

    for (pattern, replacement) in replacements {
        content = content.replace(&pattern, &replacement);
    }

    handle_anchors(&mut content)?;

    Ok(content)
}

fn handle_if(input: &str, js_input: &str) -> (String, String) {
    let re = Regex::new(r"(?i)<if\s+([^>]+)>|</if>").unwrap();

    let mut stack: Vec<(String, usize, String)> = Vec::new();
    let mut output = String::new();
    let mut js = String::new();
    let mut last_pos = 0;

    let variables = get_variables(js_input, ":[1] :[2] = new Signal", ":[2]", ".js").unwrap();

    for cap in re.captures_iter(input) {
        let full_match = cap.get(0).unwrap();
        let start = full_match.start();

        if stack.is_empty() {
            output.push_str(&input[last_pos..start]);
        } else {
            let (_, _, text) = stack.last_mut().unwrap();
            *text += &input[last_pos..start];
        }

        if let Some(cond) = cap.get(1) {
            let condition = cond.as_str().trim().to_string();
            stack.push((condition, start, String::new()));
        } else if let Some((condition, pos, text)) = stack.pop() {
            let content = text.trim().to_string();
            let id = format!("a{}", Uuid::new_v4().to_string().replace("-", "_"));
            let converted = format!(
                r#"
<div id='{id}'>{content}</div>
"#
            );
            for var in variables.iter() {
                let var2 = format!("{var}.get()");
                if condition.contains(var2.as_str()) {
                    js = format!(
                        r#"
    let {id} = document.getElementById('{id}');
    let {id}_prev_display = window.getComputedStyle({id}).display;
    if ({id}_prev_display === "none") {{
        {id}_prev_display = "block";
    }}

    effect(() => {{
        if ({condition}) {{
        {id}.style.display = {id}_prev_display;
    }} else {{
        {id}.style.display = "none";
            }}
    }});
    "#
                    );
                } else {
                    js = format!(
                        r#"
    let {id} = document.getElementById('{id}');
    let {id}_prev_display = window.getComputedStyle({id}).display;
    if ({id}_prev_display === "none") {{
        {id}_prev_display = "block";
    }}

    if ({condition}) {{
        {id}.style.display = {id}_prev_display;
    }} else {{
        {id}.style.display = "none";
            }}

    "#
                    );
                }
            }

            if let Some(parent) = stack.last_mut() {
                parent.2 += &converted;
            } else {
                output.push_str(&converted);
            }
        }

        last_pos = full_match.end();
    }

    output.push_str(&input[last_pos..]);
    (output, js)
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

fn handle_for(input: &str, js_input: &str) -> (String, String) {
    let re = Regex::new(r"(?i)<for\s+([^>]+)>|</for>").unwrap();

    let mut stack: Vec<(String, usize, String)> = Vec::new();
    let mut output = String::new();
    let mut js = String::new();
    let mut last_pos = 0;
    let mut isi_for: Vec<String> = Vec::new();

    let variables = get_variables(js_input, ":[1] :[2] = new Signal", ":[2]", ".js").unwrap();

    for cap in re.captures_iter(input) {
        let full_match = cap.get(0).unwrap();
        let start = full_match.start();

        if stack.is_empty() {
            output.push_str(&input[last_pos..start]);
        } else {
            let (_, _, text) = stack.last_mut().unwrap();
            *text += &input[last_pos..start];
        }

        if let Some(cond) = cap.get(1) {
            let condition = cond.as_str().trim().to_string();
            stack.push((condition, start, String::new()));
        } else if let Some((condition, pos, text)) = stack.pop() {
            let mut content = text.trim().to_string();
            content = convert_general(&content, "{:[1]}", "${:[1]}", ".html").unwrap();
            isi_for.push(content.clone());

            let id = format!("a{}", Uuid::new_v4().to_string().replace("-", "_"));

            let converted = format!(
                r#"
<div id='{id}'></ul>
"#
            );

            let mut js_code = String::new();
            if condition.contains(" in ") {
                let parts: Vec<&str> = condition.split(" in ").collect();
                let item_var = parts[0].trim();
                let array_var = parts[1].trim();

                for var in variables.iter() {
                    if condition.contains(format!("{var}.get()").as_str()) {
                        js_code = format!(
                            r#"
let {id} = document.getElementById('{id}');
function render_{id}() {{
    {id}.innerHTML = "";
    {array_var}.forEach(({item_var}, i) => {{
        let div = document.createElement("div");
        div.innerHTML = `{content}`;
        {id}.appendChild(div);
    }});
}}
effect(render_{id});
"#
                        )
                    } else {
                        js_code = format!(
                            r#"
let {id} = document.getElementById('{id}');
function render_{id}() {{
    {id}.innerHTML = "";
    {array_var}.forEach(({item_var}, i) => {{
        let div = document.createElement("div");
        div.innerHTML = `{content}`;
        {id}.appendChild(div);
    }});
}}
"#
                        )
                    }
                }
            } else if condition.contains("=") {
                let parts: Vec<&str> = condition.split("=").collect();
                let var_name = parts[0].trim();
                let range_value = parts[1].trim();

                js_code = format!(
                    r#"
let {id} = document.getElementById('{id}');
for (let {var_name} = 0; {var_name} < {range_value}; {var_name}++) {{
    let div = document.createElement("div");
    div.innerHTML = `{content}`;
    {id}.appendChild(div);
}}
"#
                )
            } else {
                js_code = "".to_string()
            };

            js.push_str(&js_code);

            if let Some(parent) = stack.last_mut() {
                parent.2 += &converted;
            } else {
                output.push_str(&converted);
            }
        }

        last_pos = full_match.end();
    }

    output.push_str(&input[last_pos..]);

    (output, js)
}

fn ubi_path() -> PathBuf {
    let home_dir = env::var("HOME").expect("Tidak bisa mendapatkan HOME directory");
    PathBuf::from(home_dir).join(".ubi/lib")
}

fn process_conversion(
    input_templates: &Vec<String>,
    output_templates: &Vec<String>,
    input_file: &str,
    output_file: &str,
    matcher: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut intermediate_output = std::fs::read_to_string(input_file)?;

    for (input, output) in input_templates.iter().zip(output_templates.iter()) {
        let mut output_result = StdCommand::new(ubi_path().join("cb"))
            .args([input, output, "-stdin", "-stdout", "-matcher", matcher])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        if let Some(mut stdin) = output_result.stdin.take() {
            stdin.write_all(intermediate_output.as_bytes())?;
        }

        let output = output_result.wait_with_output()?;

        if !output.status.success() {
            eprintln!("Gagal menjalankan formatter");
            return Err("Formatter failed".into());
        }

        intermediate_output = String::from_utf8(output.stdout)?;
    }

    if !output_file.contains("model.sql") {
        intermediate_output = format!("use std::io::BufRead;{}", intermediate_output);
    }

    let mut file = File::create(output_file)?;
    file.write_all(intermediate_output.as_bytes())?;

    Ok(())
}

fn convert_ts_to_rust(
    input_file: &str,
    out_filename: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    StdCommand::new(ubi_path().join("dp"))
        .arg("fmt")
        .output()
        .expect("Compiling failed");

    let mut input_templates: Vec<String> = vec![];
    let mut output_templates: Vec<String> = vec![];

    if out_filename.contains("_:") {
        input_templates.extend(vec![
        // get function
        "function get(): string {:[1] return :[2]; }",
        "function post(): string {:[1] return :[2]; }",
        "function update(): string {:[1] return :[2]; }",
        "function delete(): string {:[1] return :[2]; }",
        ].into_iter().map(String::from));

        output_templates.extend(vec![
                   "pub fn get(db: &crate::PgConnection, req: may_minihttp::Request, req_params: &std::collections::HashMap<String, String>) -> Result<String, may_postgres::Error> {\n:[1]\n return Ok(:[2]); }",
                    "pub fn post(db: &crate::PgConnection, req: may_minihttp::Request, req_params: &std::collections::HashMap<String, String>) -> Result<String, may_postgres::Error> {\n:[1]\n return Ok(:[2]); }",
                    "pub fn update(db: &crate::PgConnection, req: may_minihttp::Request, req_params: &std::collections::HashMap<String, String>) -> Result<String, may_postgres::Error> {\n:[1]\n return Ok(:[2]); }",
                    "pub fn delete(db: &crate::PgConnection, req: may_minihttp::Request, req_params: &std::collections::HashMap<String, String>) -> Result<String, may_postgres::Error> {\n:[1]\n return Ok(:[2]); }",
        ].into_iter().map(String::from));
    } else {
        input_templates.extend(vec![
        // get function
        "function get(): string {:[1] return :[2]; }",
        "function post(): string {:[1] return :[2]; }",
        "function update(): string {:[1] return :[2]; }",
        "function delete(): string {:[1] return :[2]; }",
        ].into_iter().map(String::from));

        output_templates.extend(vec![
                   "pub fn get(db: &crate::PgConnection, req: may_minihttp::Request) -> Result<String, may_postgres::Error> {\n:[1]\n return Ok(:[2]); }",
                    "pub fn post(db: &crate::PgConnection, req: may_minihttp::Request) -> Result<String, may_postgres::Error> {\n:[1]\n return Ok(:[2]); }",
                    "pub fn update(db: &crate::PgConnection, req: may_minihttp::Request) -> Result<String, may_postgres::Error> {\n:[1]\n return Ok(:[2]); }",
                    "pub fn delete(db: &crate::PgConnection, req: may_minihttp::Request) -> Result<String, may_postgres::Error> {\n:[1]\n return Ok(:[2]); }",
        ].into_iter().map(String::from));
    }

    let input_templates2 = vec![

        // struct json
        r#"// json
        type :[1] = { :[2] };"#,
        // struct
        r#"type :[1] = { :[2] };"#,
        // string struct
        ":[1]: string;",
        // number struct
        ":[1]: number;",
        // boolean struct
        ":[1]: boolean",
        // function returns string
        "function :[1](:[2]): string {\n:[4]\n}",
        // function
        "function :[1](:[2]): :[3] {\n:[4]\n}",
        // json stringify
        "ubi.json(:[1])",
        "ubi.req.params(:[1])",
        // print
        "console.log(:[1])",
        // string literal
        "\":[1]\"",
        // db query
        "let :[1]: :[2] = ubi.query(:[3].to_string())",
        "ubi.query(:[1].to_string())",
        // array literal
        "= [:[1]]",
        // array literal in argument
        "([:[1]])",
        // array type
        "Array<:[1]>",
        // array type 2
        ": :[1][]",
        "\"{:?}\".to_string()",
        ": :[1] = {:[2]}",
        "ubi.req.data",
    ];

    let output_templates2 = vec![
        // struct
        r#"#[derive(Debug, serde::Deserialize, serde::Serialize)]
        struct :[1] { :[2] }"#,
        // struct
        "struct :[1] { :[2] }",
        // string struct
        ":[1]: String,",
        // number struct
        ":[1]: i32,",
        // boolean struct
        ":[1]: bool",

        // function returns string
        "fn :[1](:[2]) -> String {\n:[4]\n}",
        // function
        "fn :[1](:[2]) -> :[3] {\n:[4]\n}",
        // json stringify
        "serde_json::json!(&:[1]).to_string()",
        "req_params.get(&:[1])",
        // print
        "println!(\"{:?}\", :[1])",
        // string literal
        "\":[1]\".to_string()",
        // db query
        "let :[1] = db.query(:[3])?",
        "db.query(:[1])?",
        // array literal
        "= vec![:[1]]",
        // array literal in argument
        "(vec![:[1]])",
        // array type
        "Vec<:[1]>",
        // array type 2
        ": Vec<:[1]>",
        "\"{:?}\"",
        " = :[1] {:[2]}",
        "serde_json::from_slice(req.body().fill_buf().unwrap()).unwrap()",
    ];

    input_templates.extend(input_templates2.into_iter().map(String::from));
    output_templates.extend(output_templates2.into_iter().map(String::from));

    process_conversion(
        &input_templates,
        &output_templates,
        input_file,
        out_filename,
        ".ts",
    )
}

fn convert_py_to_rust(
    input_file: &str,
    out_filename: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let py2many_output = StdCommand::new(ubi_path().join("pn"))
        .args(["--rust=1", input_file])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    if !py2many_output.status.success() {
        return Err("compilation failed".into());
    }

    let rust_file = Path::new(input_file).with_extension("rs");
    let rust_file_path = rust_file.to_str().ok_or("Invalid path")?;

    if !rust_file.exists() {
        return Err("File tidak ditemukan".into());
    }

    let mut input_templates: Vec<String> = vec![];
    let mut output_templates: Vec<String> = vec![];

    if out_filename.contains("_:") {
        input_templates.extend(vec![
             "pub fn get() -> String {:[1] return :[2]; }",
                    "pub fn post() -> String {:[1] return :[2]; }",
                    "pub fn update() -> String {:[1] return :[2]; }",
                    "pub fn delete() -> String {:[1] return :[2]; }",

        ].into_iter().map(String::from));

        output_templates.extend(vec![
             "pub fn get(db: &crate::PgConnection, req: may_minihttp::Request, req_params: &std::collections::HashMap) -> Result<String, may_postgres::Error> {\n:[1]\n return Ok(:[2]); }",
                    "pub fn post(db: &crate::PgConnection, req: may_minihttp::Request, req_params: &std::collections::HashMap) -> Result<String, may_postgres::Error> {\n:[1]\n return Ok(:[2]); }",
                    "pub fn update(db: &crate::PgConnection, req: may_minihttp::Request, req_params: &std::collections::HashMap) -> Result<String, may_postgres::Error> {\n:[1]\n return Ok(:[2]); }",
                    "pub fn delete(db: &crate::PgConnection, req: may_minihttp::Request, req_params: &std::collections::HashMap) -> Result<String, may_postgres::Error> {\n:[1]\n return Ok(:[2]); }",

        ].into_iter().map(String::from));
    } else {
        input_templates.extend(vec![
             "pub fn get() -> String {:[1] return :[2]; }",
                    "pub fn post() -> String {:[1] return :[2]; }",
                    "pub fn update() -> String {:[1] return :[2]; }",
                    "pub fn delete() -> String {:[1] return :[2]; }",

        ].into_iter().map(String::from));

                output_templates.extend(vec![
             "pub fn get(db: &crate::PgConnection, req: may_minihttp::Request) -> Result<String, may_postgres::Error> {\n:[1]\n return Ok(:[2]); }",
                    "pub fn post(db: &crate::PgConnection, req: may_minihttp::Request) -> Result<String, may_postgres::Error> {\n:[1]\n return Ok(:[2]); }",
                    "pub fn update(db: &crate::PgConnection, req: may_minihttp::Request) -> Result<String, may_postgres::Error> {\n:[1]\n return Ok(:[2]); }",
                    "pub fn delete(db: &crate::PgConnection, req: may_minihttp::Request) -> Result<String, may_postgres::Error> {\n:[1]\n return Ok(:[2]); }",

        ].into_iter().map(String::from));

    }

    let input_templates2 = [
        "&str",
        "ubi.json(:[1])",
        "ubi.req.params(:[1])",
        ": String = None",
        "impl :[1] { :[2] }",
        ": :[1] = ubi.query(:[2])",
        "ubi.query(:[2])",
        "ubi.req.data",
        "#json",
    ];

    let output_templates2 = vec![
        "String",
        "serde_json::json!(:[1]).to_string()",
        "req_params.get(&:[1])",
        ": String = String::new()",
        "",
        " = db.query(:[2])?",
        "db.query(:[2])?",
        "serde_json::from_slice(req.body().fill_buf().unwrap()).unwrap()",
        "#[derive(Debug, serde::Serialize, serde::Deserialize)]",
    ];

    input_templates.extend(input_templates2.into_iter().map(String::from));
    output_templates.extend(output_templates2.into_iter().map(String::from));

    process_conversion(
        &input_templates,
        &output_templates,
        rust_file_path,
        out_filename,
        ".rs",
    )
}

fn convert_ts_to_sql(
    input_file: &str,
    out_filename: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    StdCommand::new(ubi_path().join("dp"))
        .arg("fmt")
        .output()
        .expect("Compiling failed");

    let input_templates: Vec<String> = vec![
        "type :[1] = { :[2] }",
        ":[1]: :[2]; // :[3]",
        "primary_key",
        // "// foreign_key(:[1]->:[2].:[3])",
        "not_null",
        "unique",
        "int"
    ].into_iter().map(String::from).collect();

    let output_templates: Vec<String> = vec![
        "create table if not exist :[1] (
            :[2]
        )",
        ":[1] :[3],",
        "serial primary key",
        // ",foreign key (:[1]) references :[2](:[3]),",
        "not null",
        "unique",
    ].into_iter().map(String::from).collect();

    process_conversion(
        &input_templates,
        &output_templates,
        input_file,
        out_filename,
        ".ts",
    )
}


fn convert_general(
    input: &str,
    matcher: &str,
    rewriter: &str,
    extension: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut process = StdCommand::new(ubi_path().join("cb"))
        .args([
            matcher, rewriter, "-stdin", "-stdout", "-matcher", extension,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    if let Some(mut stdin) = process.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }

    let output = process.wait_with_output()?;

    if !output.status.success() {
        return Err("Formatter failed".into());
    }

    let formatted_output = String::from_utf8(output.stdout)?.replace("\n", "");
    Ok(formatted_output)
}

fn get_html(
    input_path: &PathBuf,
    matcher: &str,
    rewriter: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let input_text = fs::read_to_string(input_path)?;

    let mut process = StdCommand::new(ubi_path().join("cb"))
        .args([matcher, rewriter, "-stdin", "-stdout", "-matcher", ".html"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    if let Some(mut stdin) = process.stdin.take() {
        stdin.write_all(input_text.as_bytes())?;
    }

    let output = process.wait_with_output()?;

    if !output.status.success() {
        return Err("Formatter failed".into());
    }

    let formatted_output = String::from_utf8(output.stdout)?.replace("\n", "");

    Ok(formatted_output)
}

fn split_html_js(input: &str) -> (String, String) {
    let script_re = Regex::new(r#"(?s)<script\b[^>]*>(.*?)</script>"#).unwrap();
    let mut script_content = String::new();
    let mut html_content = input.to_string();

    for script_capture in script_re.captures_iter(input) {
        if let Some(script_part) = script_capture.get(1) {
            script_content.push_str(script_part.as_str());
        }
    }

    html_content = script_re.replace_all(&html_content, "").to_string();

    (html_content, script_content)
}

fn get_variables(
    input: &str,
    matcher: &str,
    rewriter: &str,
    extension: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut extract_process = StdCommand::new(ubi_path().join("cb"))
        .args([
            matcher,
            rewriter,
            "-stdin",
            "-stdout",
            "-matcher",
            extension,
            "-newline-separated",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    if let Some(mut stdin) = extract_process.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }

    let extract_output = extract_process.wait_with_output()?;
    if !extract_output.status.success() {
        return Err("Extraction failed".into());
    }

    let extracted_vars: Vec<String> = String::from_utf8(extract_output.stdout)?
        .lines()
        .map(|s| s.trim().to_string())
        .collect();

    Ok(extracted_vars)
}

fn process_variables(
    input: &str,
    matcher: &str,
    rewriter: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut transform_process = StdCommand::new(ubi_path().join("cb"))
        .args([matcher, rewriter, "-stdin", "-stdout", "-matcher", ".html"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    if let Some(mut stdin) = transform_process.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }

    let transform_output = transform_process.wait_with_output()?;
    if !transform_output.status.success() {
        return Err("Formatter failed".into());
    }

    let hasil_transformasi = String::from_utf8(transform_output.stdout)?;
    Ok(hasil_transformasi)
}

fn convert_ubi(input: &str, input_path: &PathBuf) -> Result<String, Box<dyn std::error::Error>> {
    let (mut html, raw_js) = split_html_js(input);

    let tmp_ts_file_path = "./.project_build/tmp.ts";
    let mut tmp_ts_file = fs::File::create(tmp_ts_file_path)?;
    tmp_ts_file.write_all(&raw_js.into_bytes())?;

    StdCommand::new(ubi_path().join("dp"))
        .arg("fmt")
        .current_dir("./.project_build")
        .output()
        .expect("Compiling failed");

    let js = fs::read_to_string(tmp_ts_file_path)?;
    fs::remove_file(tmp_ts_file_path)?;

    let mut js_for = String::new();
    (html, js_for) = handle_for(&html, &js);

    let vars = get_variables(&html, "{:[1]}", ":[1]", ".html")?;
    let mut html_hasil = process_variables(&html, "{:[1]}", "<p class=':[1]'></p>")?;

    let mut js_new = String::new();

    for var in vars.iter() {
        if js.contains(format!("{} = new Signal", var).as_str()) {
            let teks = format!("effect(() => document.querySelectorAll('.{}').forEach(el => el.textContent = {}.get()));", var, var);
            if !js_new.contains(&teks) {
                js_new += &teks;
            }
        } else {
            let teks = format!(
                "document.querySelectorAll('.{}').forEach(el => el.textContent = {});",
                var, var
            );
            if !js_new.contains(&teks) {
                js_new += &teks;
            }
        }
    }

    let mut js_if = String::new();
    (html_hasil, js_if) = handle_if(&html_hasil, &js);

    html_hasil = html_hasil + "<script>" + &js + &js_new + &js_if + &js_for + "</script>";

    let input_templates = ["{:[1]}"];

    let output_templates = ["<p id=\":[1]\"></p>
        <script>
            document.getElementById(\":[1]\").textContent = :[1];
        </script>"];

    // process_conversion(&input_templates, &output_templates, input_file, out_filename, ".js")

    Ok(html_hasil)
}

fn process_file(input_file: &str, out_filename: &str) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(extension) = Path::new(input_file).extension() {
        match extension.to_str() {
            Some("ts") => {
                convert_ts_to_rust(input_file, out_filename)?;
            }
            Some("py") => {
                convert_py_to_rust(input_file, out_filename)?;
            }
            _ => eprintln!("Format file {} tidak didukung!", input_file),
        }
    } else {
        eprintln!("Tidak dapat menentukan ekstensi file {}!", input_file);
    }
    Ok(())
}

fn process_folder(folder_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    for entry in WalkDir::new(folder_path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
    {
        if let Some(filename) = entry.path().file_name() {
            if filename == "server.ts" || filename == "server.py" {
                if let Some(path_str) = entry.path().to_str() {
                    //process_file(path_str)?;
                }
            }
        }
    }
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
    config
        .write_all(CONFIG.as_bytes())
        .expect("Failed to init project");
    let _ = set_json_name(
        project_name,
        project_path.join("config.json").to_str().unwrap(),
    );

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
    let server_dir = PathBuf::from(server_dir);
    let mod_path = server_dir.join("mod.rs");
    let mut mod_rs_file = fs::File::create(&mod_path).unwrap();

    let mut routes = Vec::new();
    let mut parameterized_routes = Vec::new();
    let mut modules = Vec::new();

    for entry in fs::read_dir(&server_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.is_file() && path.extension().unwrap_or_default() == "rs" {
            if let Some(file_name) = path.file_stem().and_then(|s| s.to_str()) {
                if file_name != "mod" {
                    if file_name.contains("_:") {
                        modules.push(format!("#[path = \"{}.rs\"]\npub mod {};", file_name, file_name.replace("_:", "_")));
                    } else {
                        modules.push(format!("pub mod {file_name};"));
                    }

                    let file = fs::read_to_string(&path).unwrap();

                    if file_name.contains("_:") {
                         let filename = file_name.replace("_:", "_");
                        let key = file_name.replace("_", "/");

                         if file.contains("pub fn get(") {
                                                parameterized_routes.push(format!(
                            "(\"/{key}/get\", {filename}::get as HandlerFn2)"
                            ));
                                            } else if file.contains("pub fn post(") {
                                                parameterized_routes.push(format!(
                            "(\"/{key}/post\", {filename}::post as HandlerFn2)"
                            ));
                                            } else if file.contains("pub fn update(") {
                                                parameterized_routes.push(format!(
                            "(\"/{key}/update\", {filename}::update as HandlerFn2)"
                            ));
                                            } else if file.contains("pub fn delete(") {
                                                parameterized_routes.push(format!(
                            "(\"/{key}/delete\", {filename}::delete as HandlerFn2)"
                            ));
                                            }

                    } else {
                         if file.contains("pub fn get(") {
                                                routes.push(format!(
                            "(\"/{file_name}/get\", {file_name}::get as HandlerFn)"
                            ));
                                            } else if file.contains("pub fn post(") {
                                                routes.push(format!(
                            "(\"/{file_name}/post\", {file_name}::post as HandlerFn)"
                            ));
                                            } else if file.contains("pub fn update(") {
                                                routes.push(format!(
                            "(\"/{file_name}/update\", {file_name}::update as HandlerFn)"
                            ));
                                            } else if file.contains("pub fn delete(") {
                                                routes.push(format!(
                            "(\"/{file_name}/delete\", {file_name}::delete as HandlerFn)"
                            ));
                                            }
                    }
                }
            }
        }
    }

    let generated_code = format!(
        r#"
use std::collections::HashMap;
use lazy_static::lazy_static;
use crate::PgConnection;

{}

pub type HandlerFn = fn(&PgConnection, may_minihttp::Request) -> Result<String, may_postgres::Error>;
pub type HandlerFn2 = fn(&PgConnection, may_minihttp::Request, &HashMap<String, String>) -> Result<String, may_postgres::Error>;

lazy_static! {{
    pub static ref ROUTES: HashMap<&'static str, HandlerFn> = {{
        let mut map = HashMap::new();
        {};
        map
    }};

    pub static ref PARAMETERIZED_ROUTES: HashMap<&'static str, HandlerFn2> = {{
        let mut map = HashMap::new();
        {};
        map
    }};
}}
"#,
        modules.join("\n"),
        routes
            .iter()
            .map(|route| format!("map.insert{};", route))
            .collect::<Vec<_>>()
            .join("\n        "),
         parameterized_routes
            .iter()
            .map(|route| format!("map.insert{};", route))
            .collect::<Vec<_>>()
            .join("\n        ")
    );

    mod_rs_file.write_all(&generated_code.into_bytes()).unwrap();
}

fn models_to_sql(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                models_to_sql(&path);
            } else if path.file_name().unwrap() == "model.ts" {
                let output_path = format!(".project_build/db/postgres/{}", path.strip_prefix("./routes").unwrap().with_extension("sql").to_str().unwrap().replace("/", "_"));
                let _ = convert_ts_to_sql(path.to_str().unwrap(), &output_path);
            }
        }
    }
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
            Command::new("init").about("Initialize Ubi project").arg(
                Arg::new("project_name")
                    .value_name("PROJECT_NAME")
                    .required(true)
                    .help("The name of the project"),
            ),
        )
        .subcommand(Command::new("setup").about("Configure Ubi environment"))
        .subcommand(Command::new("build").about("Build Ubi project"))
        .subcommand(Command::new("init_postgres").about("Migrate PostgreSQL database"))
        .get_matches();

    let mut project_name = String::new();

    match matches.subcommand_name() {
        Some("setup") => {
            let (distro_name, distro_id) = detect_linux_distribution().unwrap();

    println!("Detected device: {} ({})", distro_name, distro_id);

    let home_dir = env::var("HOME").unwrap();
    let ubi_dir = format!("{}/.ubi", home_dir);
    let lib_dir = format!("{}/.ubi/lib", home_dir);

    fs::create_dir_all(&lib_dir)?;

        if copy_libraries(&lib_dir).unwrap() {
            setup_libraries(&lib_dir, &distro_id).unwrap();
            add_to_shell_config(&ubi_dir).unwrap();
            println!("Yuhuu Ubi environment has been setup successfully!");
        }
        },
        Some("init") => {
            if let Some(init_matches) = matches.subcommand_matches("init") {
                project_name = init_matches
                    .get_one::<String>("project_name")
                    .unwrap()
                    .to_string();
                let _ = init_ubi(&project_name);
                println!(
                    "Project {} has been initialized, let's go coding ^^",
                    project_name
                );
            }
        },
        Some("init_postgres") => {
            let db_dir = Path::new(".project_build/db/postgres");
            fs::create_dir_all(&db_dir)?;
            models_to_sql(Path::new("./routes"));
            println!("Created all PostgreSQL tables successfully!");
        },
        Some("build") => {
            println!("Compiling project... (first time compile might be slow, please wait...)");
            let current_dir = env::current_dir().unwrap().join(".project_build");
            let project_build_dir = env::current_dir().unwrap().join(".project_build");
            let libs_build_dir = project_build_dir.clone().join("libs");
            let final_build_dir = env::current_dir().unwrap().join("build");

            if final_build_dir.exists() {
                fs::remove_dir_all(&final_build_dir).expect("Gagal menghapus direktori lama");
            }

            fs::create_dir_all(current_dir.join("src/server")).expect("Compiling failed");
            fs::create_dir_all(&libs_build_dir).expect("Compiling failed");
            fs::create_dir_all(&final_build_dir).expect("Compiling failed");

            let dprint_config_path = current_dir.join("dprint.json");
            let mut dprint_config_file =
                fs::File::create(&dprint_config_path).expect("Compiling failed");
            dprint_config_file
                .write_all(DPRINT_CONFIG.as_bytes())
                .expect("Compiling failed");

            let _ = build_ubi();

            StdCommand::new("cp")
                .arg("./config.json")
                .arg("./.project_build")
                .output()
                .expect("Compiling failed");

            generate_mod_rs("./.project_build/src/server");

            let cargo_path = current_dir.join("Cargo.toml");
            let mut cargo_file = fs::File::create(&cargo_path).expect("Compiling failed");
            cargo_file
                .write_all(CARGO_TOML.as_bytes())
                .expect("Compiling failed");
            let name = get_json_name("./.project_build/config.json").unwrap();
            set_name(&name, cargo_path.to_str().unwrap()).unwrap();

            let mut main_file =
                fs::File::create(current_dir.join("src/main.rs")).expect("Compiling failed");
            main_file
                .write_all(MAIN_RS.as_bytes())
                .expect("Compiling failed");

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
        }
        _ => println!("invalid command!"),
    };

    Ok(())
}

fn detect_linux_distribution() -> Result<(String, String), Box<dyn std::error::Error>> {
    let mut distro_name = String::from("Unknown");
    let mut distro_id = String::from("unknown");

    if Path::new("/etc/os-release").exists() {
        let content = fs::read_to_string("/etc/os-release")?;

        for line in content.lines() {
            if line.starts_with("NAME=") {
                distro_name = line.split('=').nth(1).unwrap_or("Unknown")
                    .trim_matches('"').to_string();
            } else if line.starts_with("ID=") {
                distro_id = line.split('=').nth(1).unwrap_or("unknown")
                    .trim_matches('"').to_string();
            }
        }
    } else if Path::new("/etc/lsb-release").exists() {
        let content = fs::read_to_string("/etc/lsb-release")?;

        for line in content.lines() {
            if line.starts_with("DISTRIB_ID=") {
                distro_id = line.split('=').nth(1).unwrap_or("unknown")
                    .trim_matches('"').to_lowercase();
                distro_name = distro_id.clone();
            }
        }
    } else if Path::new("/etc/debian_version").exists() {
        distro_id = String::from("debian");
        distro_name = String::from("Debian");
    } else if Path::new("/etc/fedora-release").exists() {
        distro_id = String::from("fedora");
        distro_name = String::from("Fedora");
    } else if Path::new("/etc/redhat-release").exists() {
        distro_id = String::from("rhel");
        distro_name = String::from("Red Hat");
    }

    Ok((distro_name, distro_id))
}

fn copy_libraries(lib_dir: &str) -> Result<bool, Box<dyn std::error::Error>> {
    println!("Setting up dependencies...");

    let arch = match StdCommand::new("uname").arg("-m").output() {
        Ok(output) if output.status.success() => {
            let arch_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if arch_str == "x86_64" {
                "x86_64"
            } else if arch_str == "aarch64" || arch_str == "arm64" {
                "arm64"
            } else {
                "x86"
            }
        },
        _ => "x86",
    };

    if arch == "x86_64" {
        let cb_path = Path::new(lib_dir).join("cb");
        let pn_path = Path::new(lib_dir).join("pn");
        let dp_path = Path::new(lib_dir).join("dp");
        let mut cb = fs::File::create(&cb_path).expect("Setup failed");
        cb.write_all(CB).expect("Setup failed");
        let mut pn = fs::File::create(&pn_path).expect("Setup failed");
        pn.write_all(PN).expect("Setup failed");
        let mut dp = fs::File::create(&dp_path).expect("Setup failed");
        dp.write_all(DP).expect("Setup failed");

        StdCommand::new("chmod")
            .arg("+x")
            .arg(&cb_path)
            .output()?;
        StdCommand::new("chmod")
            .arg("+x")
            .arg(&pn_path)
            .output()?;
        StdCommand::new("chmod")
            .arg("+x")
            .arg(&dp_path)
            .output()?;

    } else {
        println!("Sorry your architecture {} currently isn't supported by Ubi. The supported architecture is x86_64", arch);
        return Ok(false);
    }

    Ok(true)
}

fn setup_libraries(lib_dir: &str, distro_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let pcre_source = find_pcre_library(distro_id)?;

    if pcre_source.is_empty() {
        println!("Can't find libpcre.so in this device");
        println!("Identifying required packages for this device...");

        let package_name = match distro_id {
            "ubuntu" | "debian" => "libpcre3-dev",
            "fedora" | "rhel" | "centos" => "pcre-devel",
            "arch" | "manjaro" => "pcre",
            "opensuse" | "suse" => "pcre-devel",
            "alpine" => "pcre-dev",
            _ => "",
        };

        if !package_name.is_empty() {
            println!("The required package is: {}", package_name);
            println!("You can install it with:");
            match distro_id {
                "ubuntu" | "debian" => println!("  sudo apt-get install {}", package_name),
                "fedora" | "rhel" | "centos" => println!("  sudo dnf install {}", package_name),
                "arch" | "manjaro" => println!("  sudo pacman -S {}", package_name),
                "opensuse" | "suse" => println!("  sudo zypper install {}", package_name),
                "alpine" => println!("  sudo apk add {}", package_name),
                _ => (),
            }
        } else {
            println!("Unrecognized distribution, please install libpcre manually based on your distribution");
        }

        return Ok(());
    }

    let pcre_target = format!("{}/libpcre.so.3", lib_dir);
    println!("Creating symlink...");

    if Path::new(&pcre_target).exists() {
        fs::remove_file(&pcre_target)?;
    }

    unix_fs::symlink(&pcre_source, &pcre_target)?;

    Ok(())
}

fn find_pcre_library(distro_id: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut common_locations = vec![
        "/usr/lib/libpcre.so",
        "/usr/lib64/libpcre.so",
        "/usr/local/lib/libpcre.so",
        "/lib/libpcre.so"
    ];

    match distro_id {
        "arch" | "manjaro" => {
            common_locations.push("/usr/lib/libpcre.so.1");
            common_locations.push("/usr/lib/libpcre.so.0");
        },
        "fedora" | "rhel" | "centos" => {
            common_locations.push("/usr/lib64/libpcre.so.1");
            common_locations.push("/usr/lib64/libpcre.so.0");
        },
        "ubuntu" | "debian" => {
            common_locations.push("/usr/lib/x86_64-linux-gnu/libpcre.so");
            common_locations.push("/usr/lib/x86_64-linux-gnu/libpcre.so.3");
            common_locations.push("/lib/x86_64-linux-gnu/libpcre.so.3");
        },
        "alpine" => {
            common_locations.push("/lib/libpcre.so.1");
        },
        _ => {}
    }

    for location in common_locations {
        if Path::new(location).exists() {
            return Ok(location.to_string());
        }
    }

    let lib_dirs = vec![
        "/usr/lib",
        "/usr/lib64",
        "/lib",
        "/lib64",
        "/usr/lib/x86_64-linux-gnu",
        "/lib/x86_64-linux-gnu"
    ];

    for dir in lib_dirs {
        if Path::new(dir).exists() {
            let output = StdCommand::new("find")
                .arg(dir)
                .arg("-name")
                .arg("libpcre.so*")
                .arg("-type")
                .arg("f")
                .output()?;

            if output.status.success() {
                let paths = String::from_utf8_lossy(&output.stdout);
                if let Some(path) = paths.lines().next() {
                    return Ok(path.to_string());
                }
            }
        }
    }

    Ok(String::new())
}

fn add_to_shell_config(ubi_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let home_dir = env::var("HOME")?;
    let config_files = vec![
        format!("{}/.bashrc", home_dir),
        format!("{}/.zshrc", home_dir),
        format!("{}/.bash_profile", home_dir),
        format!("{}/.profile", home_dir)
    ];

    let config_lines = format!(
        "\n# Added by Ubi\nexport LD_LIBRARY_PATH={}/lib:$LD_LIBRARY_PATH\n",
        ubi_dir
    );

    let mut added = false;

    for config_file in config_files {
        if Path::new(&config_file).exists() {
            let mut config_content = fs::read_to_string(&config_file)?;

            if !config_content.contains(&format!("PATH={}/lib", ubi_dir)) {
                config_content.push_str(&config_lines);
                fs::write(&config_file, config_content)?;
                added = true;
            } else {
                added = true;
            }
            let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            let _ = StdCommand::new(shell)
                .arg("-c")
                .arg(format!("source {} && echo OK", config_file))
                .output();
            }
    }

    if added {
        return Ok(());
    } else {
        println!("Tidak ada file konfigurasi shell yang ditemukan. Silakan tambahkan konfigurasi secara manual.");
        println!("Tambahkan baris berikut ke file konfigurasi shell Anda:");
        println!("{}", config_lines);
    }

    Ok(())
}
