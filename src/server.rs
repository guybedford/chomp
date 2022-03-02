// Chomp Task Runner
// Copyright (C) 2022  Guy Bedford

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

// const websocket = new WebSocket('ws://localhost:8080/watch'); websocket.onmessage = evt => console.log(evt.data);

use crate::chompfile::ServerOptions;
use futures::{future, FutureExt, StreamExt};
use hyper::{header, Body, Response, StatusCode};
use notify::DebouncedEvent;
use percent_encoding::percent_decode_str;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::fs::File;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{mpsc, RwLock};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_util::codec::{BytesCodec, FramedRead};
use warp::ws::{Message, WebSocket, Ws};
use warp::Filter;

async fn client_connection(ws: WebSocket, state: State) {
    let (sender, mut receiver) = ws.split();
    let (client_sender, client_rcv) = mpsc::unbounded_channel();
    let client_rcv = UnboundedReceiverStream::new(client_rcv);
    tokio::task::spawn(client_rcv.forward(sender).map(|result| {
        if let Err(e) = result {
            eprintln!("error sending websocket msg: {}", e);
        }
    }));
    client_sender.send(Ok(Message::text("Connected"))).unwrap();
    let id = {
        let clients_vec = &mut state.write().await.clients;
        let id = if clients_vec.len() > 0 {
            clients_vec.last().unwrap().id + 1
        } else {
            1
        };
        let client = Client {
            sender: client_sender,
            id,
        };
        clients_vec.push(client);
        id
    };
    while let Some(body) = receiver.next().await {
        let message = match body {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("error reading message on websocket: {}", e);
                break;
            }
        };
        match message.to_str() {
            Ok(msg) => {
                println!("got message {}", msg);
            }
            _ => {
                // println!("got non string message");
            }
        }
    }
    {
        let clients_vec = &mut state.write().await.clients;
        let idx = clients_vec
            .iter()
            .enumerate()
            .find(|(_, client)| client.id == id)
            .unwrap()
            .0;
        clients_vec.remove(idx);
    }
}

pub struct Client {
    sender: mpsc::UnboundedSender<std::result::Result<Message, warp::Error>>,
    id: u32,
}

pub struct StateStruct {
    clients: Vec<Client>,
    file_hashes: BTreeMap<String, String>,
}

impl StateStruct {
    fn new() -> StateStruct {
        StateStruct {
            clients: Vec::new(),
            file_hashes: BTreeMap::new(),
        }
    }
}

pub type State = Arc<RwLock<StateStruct>>;

pub enum FileEvent {
    WatchFile(PathBuf),
}

async fn check_watcher(mut rx: UnboundedReceiver<DebouncedEvent>, root: &PathBuf, state: State) {
    loop {
        match rx.recv().await {
            Some(evt) => match evt {
                DebouncedEvent::NoticeWrite(_)
                | DebouncedEvent::NoticeRemove(_)
                | DebouncedEvent::Chmod(_)
                | DebouncedEvent::Remove(_) => {}
                DebouncedEvent::Create(path)
                | DebouncedEvent::Write(path)
                | DebouncedEvent::Rename(_, path) => {
                    let path_str = match path.strip_prefix(root) {
                        Ok(path) => path.to_str().unwrap(),
                        Err(_) => continue,
                    };
                    let _ = revalidate(&path, &path_str, state.clone(), true).await;
                }
                DebouncedEvent::Rescan => panic!("Unhandled: Watcher Rescan"),
                DebouncedEvent::Error(err, maybe_path) => {
                    panic!("Unhandled: Watcher Error {:?} {:?}", err, maybe_path)
                }
            },
            None => {}
        }
    }
}

async fn revalidate(
    path: &PathBuf,
    path_str: &str,
    state: State,
    broadcast_updates: bool,
) -> (Option<String>, bool) {
    let source = match fs::read(path).await {
        Ok(src) => src,
        Err(_) => return (None, true),
    };
    let hash = crate::http_client::hash(&source[0..]);
    let mut state = state.write().await;
    if let Some(existing_hash) = state.file_hashes.get(path_str) {
        if hash.eq(existing_hash) {
            return (Some(hash), false);
        }
    }
    state
        .file_hashes
        .insert(path_str.to_string(), hash.to_string());
    if broadcast_updates {
        for client in state.clients.iter() {
            client
                .sender
                .send(Ok(Message::text(path_str.replace('\\', "/"))))
                .expect("error sending websocket");
        }
    }
    (Some(hash), true)
}

fn not_found(resource: &str) -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(
            header::CONTENT_TYPE,
            header::HeaderValue::from_str("text/plain").unwrap(),
        )
        .body(Body::from(format!("\"{}\" Not Found", resource)))
        .unwrap()
}

async fn file_serve(path: &PathBuf, root: &PathBuf, hash: Option<String>) -> Response<Body> {
    // Serve a file by asynchronously reading it by chunks using tokio-util crate.
    if let Ok(file) = File::open(path).await {
        let stream = FramedRead::new(file, BytesCodec::new());
        let body = Body::wrap_stream(stream);
        let mut res = Response::new(body);
        let guess = mime_guess::from_path(path);
        if let Some(mime) = guess.first() {
            let headers_mut = res.headers_mut();
            headers_mut.insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_str(mime.essence_str()).unwrap(),
            );
            headers_mut.insert(
                header::ETAG,
                header::HeaderValue::from_str(&hash.unwrap()).unwrap(),
            );
            headers_mut.insert(
                header::CACHE_CONTROL,
                header::HeaderValue::from_str("must-revalidate").unwrap(),
            );
        }
        return res;
    }
    not_found(
        &path
            .strip_prefix(root)
            .expect("unexpected path")
            .to_str()
            .unwrap()
            .replace('\\', "/"),
    )
}

// TODO: gloss
async fn index_page(path: &mut PathBuf, root: &PathBuf) -> Option<Response<Body>> {
    path.push("index.html");
    match fs::metadata(&path).await {
        Ok(_) => {}
        Err(_) => {
            path.pop();
            let mut entries = std::fs::read_dir(&path)
                .unwrap()
                .map(|res| res.map(|e| e.path()))
                .collect::<Result<Vec<_>, std::io::Error>>()
                .unwrap();
            entries.sort();
            let mut listing = String::from("<!doctype html><body><ul>");
            for entry in entries {
                let name = entry
                    .strip_prefix(&path)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let relpath = entry
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let item = format!("<li><a href=\"{}\">{}</a></li>", relpath, name);
                listing.push_str(&item);
            }
            listing.push_str("</ul>");
            let mut res = Response::new(Body::from(listing));
            *res.status_mut() = hyper::StatusCode::OK;
            res.headers_mut().insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_str("text/html").unwrap(),
            );
            return Some(res);
        }
    };
    None
}

pub async fn serve(
    opts: ServerOptions,
    watch_receiver: UnboundedReceiver<DebouncedEvent>,
    watch_sender: UnboundedSender<FileEvent>,
) {
    let state: State = Arc::new(RwLock::new(StateStruct::new()));
    let watcher_state = state.clone();
    let state_clone = state.clone();
    let root = match fs::canonicalize(&opts.root).await {
        Ok(canonical) => canonical,
        Err(_) => {
            eprintln!("Unable to find the root server path {}", &opts.root);
            return;
        }
    };
    let root_str = root.to_str().unwrap();
    let root = if root_str.starts_with(r"\\?\") {
        PathBuf::from(String::from(&root_str[4..]))
    } else {
        root
    };
    let watcher_root = root.clone();
    let static_assets = warp::path::tail()
        .and(warp::any().map(move || root.clone()))
        .and(warp::any().map(move || state.clone()))
        .and(warp::any().map(move || watch_sender.clone()))
        .and(warp::filters::header::optional::<String>("if-none-match"))
        .then(
            |path: warp::path::Tail,
             root: PathBuf,
             state: State,
             sender: UnboundedSender<FileEvent>,
             validate_hash: Option<String>| async move {
                let subpath = percent_decode_str(path.as_str())
                    .decode_utf8_lossy()
                    .into_owned();
                let mut path = PathBuf::from(&root);
                path.push(&subpath);

                let is_dir = match fs::metadata(&path).await {
                    Ok(metadata) => metadata.is_dir(),
                    Err(_) => if !path.ends_with(".html") {
                        path.set_extension("html");
                        match fs::metadata(&path).await {
                            Ok(metadata) => metadata.is_dir(),
                            Err(_) => false
                        }
                    } else { false }
                };
                if is_dir {
                    if let Some(res) = index_page(&mut path, &root).await {
                        return res;
                    }
                }
                let (hash, add_watch) = revalidate(&path, &subpath, state, false).await;
                if add_watch {
                    let _ = sender.send(FileEvent::WatchFile(path.clone())).is_ok();
                }
                let (cached, etag) = match hash {
                    Some(hash) => match validate_hash {
                        Some(validate_hash) => (validate_hash == hash, Some(hash)),
                        None => (false, Some(hash)),
                    },
                    None => (false, None),
                };
                if cached {
                    let mut res = Response::new(Body::empty());
                    *res.status_mut() = hyper::StatusCode::NOT_MODIFIED;
                    return res;
                } else {
                    file_serve(&path, &root, etag).await
                }
            },
        );

    let websocket = warp::path("watch")
        .and(warp::ws())
        .and(warp::any().map(move || state_clone.clone()))
        .map(|ws: Ws, state: State| ws.on_upgrade(move |socket| client_connection(socket, state)));

    let routes = websocket
        .or(static_assets)
        .with(warp::cors().allow_any_origin());

    println!(
        "Serving \x1b[1m{}\x1b[0m on port \x1b[36m{}\x1b[0m...",
        opts.root, opts.port
    );
    future::join(
        check_watcher(watch_receiver, &watcher_root, watcher_state),
        warp::serve(routes).run(([127, 0, 0, 1], opts.port)),
    )
    .await;
}
