use rquickjs::{Runtime, Context, Function, Value, Error};
use smol::{net::TcpListener, lock::Mutex, io, io::{AsyncReadExt, AsyncWriteExt}};
use std::collections::HashMap;
use std::sync::Arc;
use clap::{Command, Arg};

#[derive(Debug, Clone)]
pub struct Routes {
    routes: HashMap<String, String>
}

impl Routes {
    pub fn new() -> Self {
        Routes {
            routes: HashMap::new(),
        }
    }

    pub async fn add_route(&mut self, path: String, response: String) {
        self.routes.insert(path, response);
    }

    pub async fn get_response(&self, path: &str) -> String {
        self.routes.get(path).cloned().unwrap_or_else(|| "404 Not Found".to_string())
    }
}

pub async fn handle_request(routes: Arc<Mutex<Routes>>, mut stream: smol::net::TcpStream) -> io::Result<()> {
    let mut buffer = [0; 1024];
    let n = stream.read(&mut buffer).await?;

    let request = String::from_utf8_lossy(&buffer[..n]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    let routes_ref = routes.lock().await;

    let response = routes_ref.get_response(path).await;
    let http_response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
        response.len(),
        response
    );
    stream.write_all(http_response.as_bytes()).await?;
    stream.flush().await?;

    Ok(())
}

pub async fn start_server(routes: Arc<Mutex<Routes>>, port: u16) -> io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    println!("The server is running on port {}", port);

    loop {
        let (stream, _) = listener.accept().await?;
        let routes_ref = Arc::clone(&routes);
        smol::spawn(async move {
            if let Err(e) = handle_request(routes_ref, stream).await {
                eprintln!("Error handling request: {}", e);
            }
        }).detach();
    }
}

pub async fn create_context(routes: Arc<Mutex<Routes>>) -> (Runtime, Context) {
    let rt = Runtime::new().unwrap();
    let ctx = Context::full(&rt).unwrap();

    ctx.with(|ctx| {
        let globals = ctx.globals();

        let routes_ref = Arc::clone(&routes);
        let get_fn = Function::new(ctx.clone(), move |path: String, response: String| {
            smol::block_on(async {
                let mut routes_clone = routes_ref.lock().await;
                routes_clone.add_route(path, response).await;
            });
        }).unwrap();
        globals.set("get", get_fn).unwrap();

        let listen_fn = Function::new(ctx.clone(), move |port: u16| {
            let routes_clone = Arc::clone(&routes);
            smol::spawn(async move {
                if let Err(e) = start_server(routes_clone, port).await {
                    eprintln!("Server error: {}", e);
                }
            }).detach();
        }).unwrap();
        globals.set("listen", listen_fn).unwrap();
    });
    (rt, ctx)
}

fn main() -> io::Result<()> {
    let matches = Command::new("ubi")
        .version("1.0")
        .author("Fuji fujisantoso134@gmail.com")
        .about("Create JavaScript backend easily")
        .arg(Arg::new("script")
             .value_name("FILE")
             .help("The JavaScript file to run")
             .required(true))
        .get_matches();

    smol::block_on(async {
        let routes = Arc::new(Mutex::new(Routes::new()));
        let (rt, ctx) = create_context(Arc::clone(&routes)).await;

        let script_default = String::from("main.js");

        let script_path = match matches.get_one::<String>("script") {
            Some(isi) => isi,
            None => {
                eprintln!("{}", String::from("Failed to read command"));
                &script_default
            }
        };

        ctx.with(|ctx| {
            match ctx.eval_file::<Value, &str>(script_path) {
                Ok(_) => {},
                Err(e) => eprintln!("Error: {}", e),
            }
        });

        loop {
            smol::Timer::after(std::time::Duration::from_secs(60)).await;
        }
    })
}

