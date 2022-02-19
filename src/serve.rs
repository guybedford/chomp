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

use hyper::service::{make_service_fn, service_fn};
use hyper::{header, Body, Method, Request, Response, Result, Server, StatusCode};
use std::convert::Infallible;
use std::net::SocketAddr;
use tokio::fs::File;
use tokio_util::codec::{BytesCodec, FramedRead};
use crate::chompfile::ServerOptions;

async fn handle(req: Request<Body>) -> Result<Response<Body>> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, _) => {
            let mut subpath = &req.uri().path()[1..];
            if subpath.len() == 0 {
                subpath = "index.html"
            }
            file_serve(subpath).await
        }
        _ => Ok(unsupported_method()),
    }
}

async fn file_serve(filename: &str) -> Result<Response<Body>> {
    // Serve a file by asynchronously reading it by chunks using tokio-util crate.
    if let Ok(file) = File::open(filename).await {
        let stream = FramedRead::new(file, BytesCodec::new());
        let body = Body::wrap_stream(stream);
        let mut res = Response::new(body);
        let guess = mime_guess::from_path(filename);
        if let Some(mime) = guess.first() {
            res.headers_mut()
                .insert(header::CONTENT_TYPE, header::HeaderValue::from_str(mime.essence_str()).unwrap());
        }
        return Ok(res);
    }
    Ok(not_found(filename))
}

fn unsupported_method() -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body("Unsupported Method".as_bytes().into())
        .unwrap()
}

fn not_found(path: &str) -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, header::HeaderValue::from_str("text/plain").unwrap())
        .body(Body::from(format!("\"{}\" Not Found", path)))
        .unwrap()
}

pub async fn serve(opts: ServerOptions) -> Result<()> {
    println!("Serving \x1b[1m{}\x1b[0m on \x1b[0;36mhttp://localhost:{}\x1b[0m...", opts.root, opts.port);

    let addr = SocketAddr::from(([127, 0, 0, 1], opts.port));

    let make_service = make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle)) });

    let server = Server::bind(&addr).serve(make_service);

    server.await?;

    Ok(())
}
