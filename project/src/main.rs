#![allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    improper_ctypes
)]

#[macro_use]
extern crate may;
extern crate may_minihttp;

mod server;

const CONFIG: &str = include_str!("../config.json");

use may_minihttp::{HttpService, HttpServiceFactory, Request, Response};
use may_postgres::{types::ToSql, Client, Statement};
// use smallvec::SmallVec;
use lazy_static::lazy_static;

use ::std::{
    collections::HashMap,
    fs, io, io::BufRead,
    path::Path,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use rust_embed::RustEmbed;
use compact_str::{ToCompactString, format_compact, CompactString};
use regex::Regex;

#[derive(RustEmbed)]
#[folder = "build/"]
struct Frontend;

lazy_static! {
    static ref MIME_TYPES: HashMap<&'static str, &'static str> = {
        let mut m = HashMap::new();
        m.insert("jpg", "content_type: image/jpeg");
        m.insert("jpeg", "content_type: image/jpeg");
        m.insert("png", "content_type: image/png");
        m.insert("gif", "content_type: image/gif");
        m.insert("svg", "content_type: image/svg+xml");
        m.insert("webp", "content_type: image/webp");
        m.insert("ico", "content_type: image/x-icon");
        m.insert("bmp", "content_type: image/bmp");
        m.insert("tiff", "content_type: image/tiff");
        m.insert("mp4", "content_type: video/mp4");
        m.insert("mp3", "content_type: audio/mpeg");
        m.insert("ogg", "content_type: audio/ogg");
        m.insert("wav", "content_type: audio/wav");
        m.insert("html", "content_type: text/html");
        m.insert("css", "content_type: text/css");
        m.insert("js", "content_type: application/javascript");
        m.insert("json", "content_type: application/json");
        m.insert("xml", "content_type: application/xml");
        m.insert("pdf", "content_type: application/pdf");
        m.insert("zip", "content_type: application/zip");
        m.insert("txt", "content_type: text/plain");
        m
    };
}

#[derive(Debug, Serialize, Deserialize)]
struct PostgresConfig {
    host: String,
    port: u16,
    name: String,
    username: String,
    password: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppConfig {
    name: String,
    port: u16,
    postgres: PostgresConfig,
}

struct Context {
    db: PgConnection,
}

pub struct PgConnection {
    client: Client,
}

impl PgConnection {
    fn new(db_url: &str) -> Self {
        let client = may_postgres::connect(&db_url).unwrap();
        PgConnection { client }
    }

    fn query(self: &Self, stmt: &str) -> Result<Vec<serde_json::Value>, may_postgres::Error> {
        let prepare = self.client.prepare(stmt)?;
        //let _ = self.client.execute("prepare tes_stmt as select * from tes;", &[]).unwrap();
        let query = self.client.query_raw(&prepare, &[])?;

        let hasil: Vec<serde_json::Value> = query.map(|r| {
                        let r = r.unwrap();
                        let mut obj = serde_json::Map::new();
                        for (i, col) in r.columns().iter().enumerate() {
                let col_name = col.name().to_string();
                let col_type = col.type_();

                let col_value: serde_json::Value = if col_type == &postgres_types::Type::INT4 || col_type == &postgres_types::Type::INT8 {
                        r.get::<_, Option<i64>>(i)
                                .map(|v| serde_json::Value::Number(serde_json::Number::from(v)))
                                .unwrap_or(serde_json::Value::Null)
                        } else if col_type == &postgres_types::Type::FLOAT4 || col_type == &postgres_types::Type::FLOAT8 {
                        r.get::<_, Option<f64>>(i)
                                .map(|v| serde_json::Value::Number(serde_json::Number::from_f64(v).unwrap()))
                                .unwrap_or(serde_json::Value::Null)
                        } else if col_type == &postgres_types::Type::BOOL {
                        r.get::<_, Option<bool>>(i).map(serde_json::Value::Bool).unwrap_or(serde_json::Value::Null)
                        } else {
                        r.get::<_, Option<&str>>(i)
                                .map(|v| serde_json::Value::String(v.to_string()))
                                .unwrap_or(serde_json::Value::Null)
                        };

                obj.insert(col_name, col_value);
            }
                        serde_json::Value::Object(obj)
                        })
                    .collect();

        Ok(hasil)
    }
}

struct PgPool {
    clients: Vec<PgConnection>,
}

impl PgPool {
    fn new(db_url: &'static str, size: usize) -> PgPool {
        let clients = (0..size)
            .map(|_| may::go!(move || PgConnection::new(db_url)))
            .collect::<Vec<_>>();
        let mut clients: Vec<_> = clients.into_iter().map(|t| t.join().unwrap()).collect();
        clients.sort_by(|a, b| (a.client.id() % size).cmp(&(b.client.id() % size)));
        PgPool { clients }
    }

    fn get_connection(&self, id: usize) -> PgConnection {
        let len = self.clients.len();
        let connection = &self.clients[id % len];
        // assert_eq!(connection.client.id() % len, id % len);
        PgConnection {
            client: connection.client.clone(),
        }
    }
}


impl HttpService for Context {
    fn call(&mut self, req: Request, res: &mut Response) -> io::Result<()> {
        match req.path() {
            path if path.starts_with("/api") => {
                let mut isi = String::new();

                match req.method() {
                    "GET" => {
                        isi = match server::ROUTES.get(format_compact!("{}/get", path.strip_suffix("/").unwrap_or(path)).as_str()) {
                                                Some(handler) => {
                                                match handler(&self.db, req) {
                                                        Ok(response) =>response,
                                                        Err(e) => format!("Error: {}", e),
                                                    }
                                            }
                                                None => {
                                                    let url = format!("{}/{}", path.strip_suffix("/").unwrap_or(path), req.method().to_lowercase());

                                                    match match_url(&url) {
                                                        Some((handler, params)) => handler(&self.db, req, &params).unwrap(),
                                                        None =>  format!("404 Not Found"),
                                                    }
                                                }
                                            };

                    },
                    "POST" => {
                        isi = match server::ROUTES.get(format_compact!("{}/post", path.strip_suffix("/").unwrap_or(path)).as_str()) {
                                        Some(handler) => {
                                                match handler(&self.db, req) {
                                                Ok(response) => response,
                                                Err(e) => format!("Error: {}", e),
                                            }
                                            }
                                        None => {
                                            let url = format!("{}/{}", path.strip_suffix("/").unwrap_or(path), req.method().to_lowercase());

                                                    match match_url(&url) {
                                                        Some((handler, params)) => handler(&self.db, req, &params).unwrap(),
                                                        None =>  format!("404 Not Found"),
                                                    }

                                        },
                                    };

                    },
                    "UPDATE" => {
                        isi = match server::ROUTES.get(format_compact!("{}/update", path.strip_suffix("/").unwrap_or(path)).as_str()) {
                                                Some(handler) => {
                                                match handler(&self.db, req) {
                                                        Ok(response) =>response,
                                                        Err(e) => format!("Error: {}", e),
                                                    }
                                            }
                                                None => {
                                                    let url = format!("{}/{}", path.strip_suffix("/").unwrap_or(path), req.method().to_lowercase());

                                                    match match_url(&url) {
                                                        Some((handler, params)) => handler(&self.db, req, &params).unwrap(),
                                                        None =>  format!("404 Not Found"),
                                                    }

                                                },
                                            };

                    },
                    "DELETE" => {
                        isi = match server::ROUTES.get(format_compact!("{}/delete", path.strip_suffix("/").unwrap_or(path)).as_str()) {
                        Some(handler) => {
                                match handler(&self.db, req) {
                                        Ok(response) =>response,
                                        Err(e) => format!("Error: {}", e),
                                    }
                            }
                            None => {
                                let url = format!("{}/{}", path.strip_suffix("/").unwrap_or(path), req.method().to_lowercase());

                                                    match match_url(&url) {
                                                        Some((handler, params)) => handler(&self.db, req, &params).unwrap(),
                                                        None =>  format!("404 Not Found"),
                                                    }

                            },
                        };

                    },
                    _ => isi = "not found".to_string()
                }

                res.header("content-type: application/json").body_vec(isi.into_bytes());
            }
            path if path.starts_with("/parts") => {
                if !req.path().ends_with("/") {
                    // println!("path 1: {}", req.path().strip_prefix("/parts/").unwrap());
                    match Frontend::get(&format_compact!("{}/ui.js", req.path().strip_prefix("/parts/").unwrap())) {
                        Some(isi) => {
                            res.header("content-type: text/plain").body_vec(isi.data.to_vec());
                        }
                        None => {
                            res.header("content-type: text/plain").body("not found");
                        }
                    };
                } else {
                    let mut path = req.path().strip_prefix("/parts/").unwrap();
                    path = path.strip_prefix("/").unwrap_or(path);
                    // println!("path sebelum: {}, path 2: {}", req.path(), path);
                    let part = Frontend::get(&format_compact!("{}ui.js", path)).unwrap();
                    res.header("content-type: text/plain").body_vec(part.data.to_vec());
                }
            }
            "/favicon.ico" => {
                match fs::read(format_compact!("./static/{}", req.path())) {
                    Ok(contents) => {
                        res.header(get_mime_type(req.path().to_string()))
                            .body_vec(contents);
                        Ok::<(), io::Error>(())
                    }
                    Err(_) => {
                        res.status_code(404, "not found");
                        Ok(())
                    }
                }?;

            }
            path if path.starts_with("/static") => {
                let path = req.path();
                match fs::read(format_compact!(".{}", path)) {
                    Ok(contents) => {
                        res.header(get_mime_type(path.to_string()))
                            .body_vec(contents);
                        Ok::<(), io::Error>(())
                    }
                    Err(_) => {
                        res.status_code(404, "not found");
                        Ok(())
                    }
                }?;
                return Ok(());
            }
            _ => {
                if !req.path().ends_with("/") {
                    match Frontend::get(&format_compact!("{}/index.html", req.path().strip_prefix("/").unwrap())) {
                        Some(isi) => {
                            res.header("content-type: text/html").body_vec(isi.data.to_vec());
                        }
                        None => {
                            res.header("content-type: text/html").body("not found");
                        }
                    };
                } else {
                    let path = req.path().strip_prefix("/").unwrap();
                    let index = Frontend::get(&format_compact!("{}index.html", path)).unwrap();
                    res.header("content-type: text/html").body_vec(index.data.to_vec());
                }
            }
        }
        Ok(())
    }
}

struct Server {
    db_pool: PgPool,
}

impl HttpServiceFactory for Server {
    type Service = Context;

    fn new_service(&self, id: usize) -> Self::Service {
        Context {
               db: self.db_pool.get_connection(id),
        }
    }
}

fn get_mime_type(path: String) -> &'static str {
    let ext = Path::new(&path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    MIME_TYPES
        .get(ext)
        .unwrap_or(&"content_type: application/octet-stream")
}


pub fn parameterized_url(pattern: &str, url: &str) -> Option<std::collections::HashMap<String, String>> {
    let re_str = Regex::new(r":([^/]+)").unwrap();
    let regex_pattern = re_str.replace_all(pattern, r"([^/]+)");
    let full_regex = format!("^{}$", regex_pattern);
    let re = Regex::new(&full_regex).unwrap();

    let captures = re.captures(url)?;

    let mut params = std::collections::HashMap::new();
    for (i, cap) in re_str.captures_iter(pattern).enumerate() {
        if let Some(param) = cap.get(1) {
            params.insert(param.as_str().to_string(), captures[i + 1].to_string());
        }
    }

    Some(params)
}

fn match_url(url: &str) -> Option<(server::HandlerFn2, HashMap<String, String>)> {
    for (pattern, handler) in server::PARAMETERIZED_ROUTES.iter() {
        if let Some(params) = parameterized_url(pattern, url) {
            return Some((*handler, params));
        }
        println!("pattern : {}", pattern);
        println!("url : {}", url);
    }
    println!("failed to loop parameterized loop");
    None
}

fn main() -> io::Result<()> {

    may::config().set_pool_capacity(1000).set_stack_size(0x1000);

    let app_config: AppConfig = serde_json::from_str(CONFIG).unwrap();
    println!("{:?}", app_config);

    let db_url = format!(
        "postgresql://{}:{}@{}:{}/{}",
        app_config.postgres.username,
        app_config.postgres.password,
        app_config.postgres.host,
        app_config.postgres.port,
        app_config.postgres.name
    );
    println!("{}", db_url);

       let server = Server {
        db_pool: PgPool::new(db_url.leak(), num_cpus::get()),
    };

    server
        .start(format!("0.0.0.0:{}", app_config.port))
        .unwrap()
        .join()
        .unwrap();
    println!("Yuhuu, server listening on port : {}. Untuk mengakses url API backend diawali dengan /api, contohnya /api/users", app_config.port);

    Ok(())
}
