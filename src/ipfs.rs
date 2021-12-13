use ipfs_api_backend_hyper::{IpfsApi, IpfsClient, KeyType, TryFromUri};
const APPKEY: &str = "appname";

async fn write_ipfs() {
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
}