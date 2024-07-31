#![allow(unused_braces)]
mod deadsimple;

use deadsimple::*;
use render::html;
use serde_json::json;

fn root_handler(req: Request<()>) -> Response {
    Response::HTML(html! {
        <h1>{format!("Hello, World! {:?}", req.args)}</h1>
    })
}

fn test_handler(req: Request<()>) -> Response {
    std::thread::sleep(std::time::Duration::from_secs(1));

    let val = req.params["krajsy"].clone();

    Response::JSON(json!({ 
        "test": val 
    }))
}

#[tokio::main]
async fn main() {
    WebService::new("127.0.0.1:8000", ())
        .register("/", Method::GET, &root_handler)
        .register("/test/{krajsy}/give", Method::GET, &test_handler)
        .register("/hello", Method::GET, &|_| {
            Response::HTML(include_str!("test.html").to_string())
        })
        .register("/komari", Method::GET, &|_| {
            Response::JPG(include_bytes!("./komari.jpg").to_vec())
        })
        .register("404", Method::GET, &|_| {
            Response::HTML("404 :(".to_string())
        })
        .listen(true)
        .await;
}
