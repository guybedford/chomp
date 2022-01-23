use hyper_tls::HttpsConnector;
use hyper::{Uri, Client};
use anyhow::{anyhow, Result};

pub async fn clear_cache () -> Result<()> {
    Ok(())
}

pub async fn fetch_uri_cached (uri_str: &str, uri: Uri) -> Result<String> {
    let https = HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https);

    let res = client.get(uri).await?;
    if res.status() != 200 {
        return Err(anyhow!("{} for extension URL {}", res.status(), uri_str));
    }

    let body_bytes = hyper::body::to_bytes(res.into_body()).await?;
    Ok(String::from_utf8(body_bytes.to_vec()).unwrap())
}
