use crate::engines::CmdPool;
use async_std::fs;
use async_std::process::ExitStatus;
use futures::future::BoxFuture;
use std::collections::BTreeMap;
use std::env;
use uuid::Uuid;

// Custom node loader to mimic current working directory despite loading from a tmp file
const NODE_CMD: &str = "node --no-warnings --loader \"data:text/javascript,import{readFileSync}from'fs';export function resolve(u,c,d){if(u.endsWith('[cm]'))return{url:u,format:'module'};return d(u,c);}export function load(u,c,d){if(u.endsWith('[cm]'))return{source:readFileSync(process.env.CHOMP_MAIN),format:'module'};return d(u,c)}export{load as getFormat,load as getSource}\" [cm]";

pub fn node_runner(
  cmd_pool: &mut CmdPool,
  run: String,
  env: &mut BTreeMap<String, String>,
) -> BoxFuture<'static, ExitStatus> {
  let uuid = Uuid::new_v4();
  let mut tmp_file = env::temp_dir();
  tmp_file.push(&format!("{}.mjs", uuid.to_simple().to_string()));
  env.insert("CHOMP_MAIN".to_string(), tmp_file.to_str().unwrap().to_string());
  env.insert("CHOMP_PATH".to_string(), std::env::args().next().unwrap().to_string());
  let child_future = cmd_pool.get_next(NODE_CMD.to_string(), env);
  Box::pin(async move {
    fs::write(&tmp_file, run)
      .await
      .expect("unable to write temporary file");
    let mut child = child_future.await;
    let result = child.status().await.expect("Child process error");
    fs::remove_file(&tmp_file)
      .await
      .expect("unable to cleanup tmp file");
    result
  })
}
