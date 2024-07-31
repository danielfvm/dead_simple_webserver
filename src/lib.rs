use std::{
    collections::{HashMap, VecDeque},
    fmt,
    io::{prelude::*, BufReader},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
};

use strum::EnumProperty;
use strum_macros;

#[derive(Debug, PartialEq, Eq, Hash, strum_macros::EnumString, strum_macros::IntoStaticStr)]
pub enum Method {
    GET,
    POST,
    PUT,
    PATCH,
    DELETE,
    HEAD,
    OPTIONS,
    TRACE,
}

#[derive(strum_macros::EnumProperty, Debug)]
#[allow(dead_code)]
pub enum Response {
    #[strum(props(content_type = "text/html"))]
    HTML(String),

    #[strum(props(content_type = "text/xml"))]
    XML(String),

    #[strum(props(content_type = "image/svg+xml"))]
    SVG(String),

    #[strum(props(content_type = "application/javascript"))]
    JS(String),

    #[strum(props(content_type = "application/json"))]
    JSON(serde_json::Value),

    #[strum(props(content_type = "text/plain"))]
    TEXT(String),

    #[strum(props(content_type = "text/css"))]
    CSS(String),

    #[strum(props(content_type = "image/png"))]
    PNG(Vec<u8>),

    #[strum(props(content_type = "image/jpeg"))]
    JPG(Vec<u8>),

    #[strum(props(content_type = "image/gif"))]
    GIF(Vec<u8>),

    #[strum(props(content_type = "image/webp"))]
    WEBP(Vec<u8>),

    ERROR(WebError),
}

type Callback<T> = dyn Fn(Request<T>) -> Response + Send + Sync + 'static;

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, PartialEq)]
#[allow(non_camel_case_types)]
#[allow(dead_code)]
pub enum WebError {
    BAD_REQUEST = 400,
    NOT_FOUND = 404,
    INTERNAL_SERVER_ERROR = 500,
}

pub struct CallbackPathManager<T: 'static> {
    handlers: Vec<Vec<(String, &'static Callback<T>)>>,
}

impl<T: 'static> CallbackPathManager<T> {
    pub fn new() -> Self {
        Self {
            handlers: (0..Method::TRACE as usize).map(|_| Vec::new()).collect(),
        }
    }

    fn register(&mut self, method: Method, pattern: &str, handler: &'static Callback<T>) {
        self.handlers[method as usize].push((pattern.to_string(), handler));
    }

    fn extract(path: &str, pattern: &str) -> HashMap<String, String> {
        let path_tokens = path.split("/").collect::<Vec<_>>();
        let pattern_tokens = pattern.split("/").collect::<Vec<_>>();
        let mut params = HashMap::new();
        for (path_token, pattern_token) in path_tokens.into_iter().zip(pattern_tokens) {
            let wildcard = pattern_token.starts_with("{") && pattern_token.ends_with("}");
            if wildcard {
                let name = pattern_token
                    .strip_prefix("{")
                    .unwrap()
                    .strip_suffix("}")
                    .unwrap();
                params.insert(name.to_string(), path_token.to_string());
            }
        }
        params
    }

    fn compare(path: &str, pattern: &str) -> bool {
        let path_tokens = path.split("/").collect::<Vec<_>>();
        let pattern_tokens = pattern.split("/").collect::<Vec<_>>();

        if path_tokens.len() != pattern_tokens.len() {
            return false;
        }

        for (path_token, pattern_token) in path_tokens.into_iter().zip(pattern_tokens) {
            let wildcard = pattern_token.starts_with("{") && pattern_token.ends_with("}");
            if path_token != pattern_token && !wildcard {
                return false;
            }
        }

        return true;
    }

    fn find(
        &self,
        method: Method,
        path: &str,
    ) -> Option<(&'static Callback<T>, HashMap<String, String>)> {
        let path = path.split("?").collect::<Vec<_>>()[0];
        self.handlers[method as usize]
            .iter()
            .find(|(pattern, _)| CallbackPathManager::<T>::compare(path, pattern))
            .and_then(|(pattern, handler)| {
                Some((*handler, CallbackPathManager::<T>::extract(path, pattern)))
            })
    }
}

pub struct WebService<'a, T: 'static> {
    addr: &'a str,
    path_manager: CallbackPathManager<T>,
    shared_data: Arc<Mutex<T>>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct Request<'a, T: 'static> {
    pub shared_data: Arc<Mutex<T>>,
    pub params: HashMap<String, String>,
    pub args: HashMap<String, String>,
    pub stream: &'a TcpStream,
    pub body: Vec<u8>,
}

impl<'a, T: Send + Sync> WebService<'a, T> {
    pub fn new(addr: &'a str, shared_data: T) -> Self {
        Self {
            addr,
            path_manager: CallbackPathManager::<T>::new(),
            shared_data: Arc::new(Mutex::new(shared_data)),
        }
    }

    pub fn register(
        mut self,
        pattern: &str,
        method: Method,
        handler: &'static Callback<T>,
    ) -> Self {
        self.path_manager.register(method, pattern, handler);
        self
    }

    fn handle_connection(&mut self, mut stream: TcpStream) {
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut req = httparse::Request::new(&mut headers);
        let mut data = vec![]; 
        loop {
            let mut buffer = [0; 2048];
            if let Ok(n) = stream.read(&mut buffer) {
                data.extend_from_slice(&buffer[..n]);

                if n == 2048 {
                    continue;
                }
            }

            break;
        }

        req.parse(&data).unwrap();

        if req.method.is_none() || req.path.is_none() || req.headers.is_empty() {
            let _ = stream.write_all("HTTP/1.1 500 INTERNAL SERVER ERROR".as_bytes());
            return;
        }

        // Extract body
        let body_start = data.windows(4).position(|window| window == b"\r\n\r\n");
        let body = if let Some(body_start) = body_start {
            data[body_start + 4..].to_vec()
        } else {
            vec![]
        };


        // TODO: https://lib.rs/crates/httparse
        let method = req.method.unwrap().parse().unwrap_or(Method::GET);
        let path_and_args = req.path.unwrap_or(&"/");
        let mut path = path_and_args;
        let mut args = HashMap::new();

        // Extract params
        if path_and_args.contains("?") {
            let path_and_params = path_and_args.split('?').collect::<Vec<_>>();
            path = path_and_params[0];

            args = path_and_params[1]
                .split('&')
                .map(|param| {
                    let mut name_value = param.split('=').collect::<VecDeque<_>>();
                    (name_value.pop_front(), name_value.pop_front())
                })
                .filter_map(|(name, value)| match (name, value) {
                    (Some(name), Some(value)) => Some((name.to_string(), value.to_string())),
                    _ => None,
                })
                .collect::<HashMap<_, _>>();
        }

        let handler = self
            .path_manager
            .find(method, path)
            .or(self.path_manager.find(Method::GET, "404"));

        if let Some((handler, params)) = handler {
            let shared_data = self.shared_data.clone();

            tokio::spawn(async move {
                let response = handler(Request {
                    shared_data,
                    args,
                    params,
                    stream: &stream,
                    body,
                });

                let _ = if let Some(content_type) = response.get_str("content_type") {
                    let _ = stream.write_all(
                        format!("HTTP/1.1 200 OK\r\nContent-Type: {}\r\n\r\n", content_type)
                            .as_bytes(),
                    );

                    match response {
                        Response::HTML(html) => stream.write_all(html.as_bytes()),
                        Response::JS(text) => stream.write_all(text.as_bytes()),
                        Response::XML(text) => stream.write_all(text.as_bytes()),
                        Response::CSS(text) => stream.write_all(text.as_bytes()),
                        Response::TEXT(text) => stream.write_all(text.as_bytes()),
                        Response::JSON(json) => stream.write_all(json.to_string().as_bytes()),
                        Response::PNG(bytes) => stream.write_all(&bytes),
                        Response::JPG(bytes) => stream.write_all(&bytes),
                        Response::GIF(bytes) => stream.write_all(&bytes),
                        Response::WEBP(bytes) => stream.write_all(&bytes),
                        Response::SVG(text) => stream.write_all(text.as_bytes()),
                        _ => stream.write_all("HTTP/1.1 500 INTERNAL SERVER ERROR".as_bytes()),
                    }
                } else {
                    stream.write_all("HTTP/1.1 500 INTERNAL SERVER ERROR".as_bytes())
                };
            });
        } else {
            let _ = stream.write_all("HTTP/1.1 404 NOT FOUND".as_bytes());
        }
    }

    pub async fn listen(&mut self, open_in_browser: bool) {
        let listener = TcpListener::bind(self.addr).unwrap();
        let url = format!("http://{}", self.addr);

        if open_in_browser {
            let _ = webbrowser::open(url.as_str());
        }

        println!("Listening on {}", url);

        while let Ok((stream, _socket)) = listener.accept() {
            self.handle_connection(stream);
        }
    }
}
