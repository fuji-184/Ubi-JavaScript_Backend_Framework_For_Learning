use rquickjs::{Runtime, Context, Function, Value};
use smol::{net::TcpListener, lock::RwLock, io, io::{AsyncReadExt, AsyncWriteExt}};
use std::sync::Arc;
use clap::{Command, Arg};
use std::collections::{VecDeque, HashMap};

#[derive(Debug, Clone)]
pub struct RouteData {
    pub content_type: String,
    pub response: String,
}

#[derive(Debug, Clone)]
pub struct Routes {
    routes: HashMap<String, RouteData>,
}

impl Routes {
    pub fn new() -> Self {
        Routes {
            routes: HashMap::new(),
        }
    }

    pub async fn add_route(&mut self, path: String, content_type: String, response: String) {
        self.routes.insert(
            path,
            RouteData {
                content_type,
                response,
            },
        );
    }

    pub fn get_response<'a>(&'a self, path: &str) -> &'a RouteData {
        self.routes.get(path).unwrap_or(&DEFAULT_ROUTE)
    }
}

lazy_static::lazy_static! {
    static ref DEFAULT_ROUTE: RouteData = RouteData {
        content_type: "text/plain".to_string(),
        response: "404 Not Found".to_string(),
    };
}

pub struct BufferPool {
    pool: RwLock<VecDeque<Vec<u8>>>,
    buffer_size: usize,
    max_buffers: usize
}

impl BufferPool {
    pub fn new(buffer_size: usize, max_buffers: usize) -> Self {
        BufferPool {
            pool: RwLock::new(VecDeque::new()),
            buffer_size,
            max_buffers
        }
    }

    pub async fn get(&self) -> Vec<u8> {
        let mut pool = self.pool.write().await;

        pool.pop_front().unwrap_or_else(|| vec![0; self.buffer_size])
    }

    pub async fn release(&self, buffer: Vec<u8>) {
        let mut pool = self.pool.write().await;

        if pool.len() < self.max_buffers {
            pool.push_back(buffer);
        }
    }

    pub async fn add(&self) {
        let mut pool = self.pool.write().await;

        for _ in 0..self.max_buffers {
            pool.push_back(vec![0; self.buffer_size]);
        }
    }
}

pub async fn handle_request(routes: Arc<RwLock<Routes>>, mut stream: smol::net::TcpStream, buffer_pool: Arc<BufferPool>) -> io::Result<()> {
    let mut buffer = buffer_pool.get().await;
    let n = stream.read(&mut buffer).await?;

    let request = String::from_utf8_lossy(&buffer[..n]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    let routes_ref = routes.read().await;
    let route_data = routes_ref.get_response(path);

    let mut http_response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
        route_data.content_type,
        route_data.response.len()
    )
    .into_bytes();
    http_response.extend_from_slice(route_data.response.as_bytes());

    stream.write_all(&http_response).await?;
    stream.flush().await?;

    buffer_pool.release(buffer).await;

    Ok(())
}

pub async fn start_server(routes: Arc<RwLock<Routes>>, buffer_pool: Arc<BufferPool>, port: u16) -> io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    println!("The server is running on port {}", port);

    loop {
        let (stream, _) = listener.accept().await?;
        let routes_ref = Arc::clone(&routes);
        let buffer_pool_ref = Arc::clone(&buffer_pool);

        smol::spawn(async move {
            if let Err(e) = handle_request(routes_ref, stream, buffer_pool_ref).await {
                eprintln!("Error handling request: {}", e);
            }
        })
        .detach();
    }
}

pub async fn create_context(routes: Arc<RwLock<Routes>>, buffer_pool: Arc<BufferPool>) -> Context {
    let ctx = Context::full(&Runtime::new().unwrap()).unwrap();

    ctx.with(|ctx| {
        let globals = ctx.globals();

        let routes_ref = Arc::clone(&routes);
        let get_fn = Function::new(ctx.clone(), move |path: String, content_type: String, response: String| {
            smol::block_on(async {
                let mut routes_clone = routes_ref.write().await;
                routes_clone.add_route(path, content_type, response).await;
            });
        })
        .unwrap();
        globals.set("get", get_fn).unwrap();

        let listen_fn = Function::new(ctx.clone(), move |port: u16| {
            let routes_clone = Arc::clone(&routes);
            let buffer_pool_ref = Arc::clone(&buffer_pool);
            smol::spawn(async move {
                if let Err(e) = start_server(routes_clone, buffer_pool_ref, port).await {
                    eprintln!("Server error: {}", e);
                }
            })
            .detach();
        })
        .unwrap();
        globals.set("listen", listen_fn).unwrap();
    });
    ctx
}

fn main() -> io::Result<()> {
    let matches = Command::new("ubi")
        .version("1.0")
        .author("Fuji <fujisantoso134@gmail.com>")
        .about("Create JavaScript backend easily")
        .arg(
            Arg::new("script")
                .value_name("FILE")
                .help("The JavaScript file to run")
                .required(true),
        )
        .get_matches();

    smol::block_on(async {
        let routes = Arc::new(RwLock::new(Routes::new()));

        let buffer_pool = Arc::new(BufferPool::new(1024, 100));
        buffer_pool.add().await;

        let ctx = create_context(routes, buffer_pool).await;

        let script_default = String::from("main.js");

        let script_path = matches.get_one::<String>("script").unwrap_or(&script_default);

        ctx.with(|ctx| {
            match ctx.eval_file::<Value, &str>(script_path) {
                Ok(_) => {}
                Err(e) => eprintln!("Error: {}", e),
            }
        });

        loop {
            smol::Timer::after(std::time::Duration::from_secs(60)).await;
        }
    })
}

