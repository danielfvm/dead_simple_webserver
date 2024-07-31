extern crate serde_derive;
extern crate serde_json;

extern crate dead_simple_webserver;

use std::collections::HashMap;

use dead_simple_webserver::*;
use serde_derive::Serialize;
use serde_json::json;

#[derive(Serialize)]
struct Message {
    username: String,
    message: String,
}

fn chat(req: Request<Vec<Message>>) -> Response {
    let data: HashMap<String, String> =
        serde_json::from_slice(req.body.as_slice()).unwrap_or(HashMap::new());

    let username = data.get("username");
    let message = data.get("message");

    if username.is_none() || message.is_none() {
        return Response::ERROR(WebError::BAD_REQUEST);
    }

    req.shared_data.lock().unwrap().push(Message {
        username: username.unwrap().to_string(),
        message: message.unwrap().to_string(),
    });

    history(req)
}

fn history(req: Request<Vec<Message>>) -> Response {
    Response::JSON(json! {
        {
            "messages": *req.shared_data.lock().unwrap()
        }
    })
}

#[tokio::main]
async fn main() {
    let messages: Vec<Message> = Vec::new();

    WebService::new("127.0.0.1:8000", messages)
        .register("/", Method::GET, &|_| {
            Response::HTML(std::fs::read_to_string("chat.html").unwrap())
        })
        .register("/chat", Method::POST, &chat)
        .register("/history", Method::GET, &history)
        .listen(false)
        .await;
}
