use std::path::PathBuf;
use hyper_tls::HttpsConnector;
use hyper::{Uri, Client};
use anyhow::{anyhow, Result};
use dirs::home_dir;
use tokio::fs;
use sha2::{Sha256, Digest};

fn chomp_cache_dir () -> PathBuf {
    let mut path = home_dir().unwrap();
    path.push(".chomp");
    path.push("cache");
    path
}

pub async fn clear_cache () -> Result<()> {
    fs::remove_dir_all(chomp_cache_dir()).await?;
    Ok(())
}

pub async fn prep_cache () -> Result<()> {
    match fs::create_dir(chomp_cache_dir()).await { _ => Ok(()) }
}

#[inline(always)]
fn u4_to_hex_char (c: u8) -> char {
    return if c < 10 { c + 48 } else { c + 87 } as char;
}

fn hash_uri (uri_str: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(uri_str.as_bytes());
    let result = hasher.finalize();
    let mut out_hash = String::with_capacity(64);
    for c in result {
        out_hash.push(u4_to_hex_char(c & 0xF));
        out_hash.push(u4_to_hex_char(c >> 4));
    }
    out_hash
}

async fn from_cache (cache_key: &str) -> Option<String> {
    let mut path = chomp_cache_dir();
    path.push(cache_key);
    match fs::read_to_string(&path).await {
        Ok(cached) => Some(cached),
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => None,
            _ => panic!("File error {}", e)
        }
    }
}

async fn write_cache (cache_key: &str, source: &str) -> Result<()> {
    let mut path = chomp_cache_dir();
    path.push(cache_key);
    fs::write(&path, source).await?;
    Ok(())
}

pub async fn fetch_uri_cached (uri_str: &str, uri: Uri) -> Result<String> {
    let hash = hash_uri(uri_str);
    if let Some(cached) = from_cache(&hash).await {
        return Ok(cached);
    }

    println!("\x1b[34;1mFetch\x1b[0m {}", &uri_str);
    let https = HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https);

    let res = client.get(uri).await?;
    if res.status() != 200 {
        return Err(anyhow!("{} for extension URL {}", res.status(), uri_str));
    }

    let body_bytes = hyper::body::to_bytes(res.into_body()).await?;
    let result = String::from_utf8(body_bytes.to_vec()).unwrap();
    write_cache(&hash, &result).await?;
    Ok(result)
}
