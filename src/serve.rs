use http::uri::Scheme;
use ipfs_api_backend_hyper::{IpfsApi, IpfsClient, KeyType, TryFromUri};
use std::io::Cursor;

const APPKEY: &str = "appname";

pub struct ServeOptions {
    pub port: u32,
    pub ipfs: bool,
}

pub async fn serve(opts: ServeOptions) -> Result<(), std::io::Error> {
    if opts.ipfs {
        println!("Publishing to IPFS...");
    } else {
        println!("Serving http://localhost:{}...", opts.port);
    }

    let client = IpfsClient::from_ipfs_config().unwrap_or_else(|| {
        IpfsClient::from_host_and_port(Scheme::HTTP, "localhost", 45005).unwrap()
    });
    let data = Cursor::new("Hello World!");
    let hash = match client.add(data).await {
        Ok(res) => res.hash,
        Err(e) => {
            eprintln!("Error adding file: {}", e);
            std::process::exit(1);
        }
    };

    let key = match client.key_gen(&APPKEY, KeyType::Ed25519, 0).await {
        Ok(keypair) => {
            println!("created key {} for {}", keypair.id, keypair.name);
            keypair.id
        }
        Err(e) => {
            eprintln!("error creating key for {}: {}", APPKEY, e);
            return Ok(());
        }
    };

    let _publish = match client
        .name_publish(&hash, true, None, None, Some(&key))
        .await
    {
        Ok(publish) => {
            println!("published {} to: /ipns/{}", hash, &publish.name);
            publish
        }
        Err(e) => {
            eprintln!("error publishing name: {}", e);
            return Ok(());
        }
    };

    Ok(())
}
