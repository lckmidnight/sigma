use futures::{StreamExt, TryStreamExt};

use k8s_openapi::api::core::v1::Pod;

use kube::{
    api::{Api, ListParams, ResourceExt},
    core::WatchEvent,
    Client,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::try_default().await?;

    let pods: Api<Pod> = Api::default_namespaced(client);
    let list_params = ListParams::default()
        .fields("metadata.name=sigma-node")
        .timeout(10);
    let mut stream = pods.watch(&list_params, "0").await?.boxed();
    while let Some(status) = stream.try_next().await? {
        match status {
            WatchEvent::Added(s) => println!("Added {}", s.name()),
            WatchEvent::Modified(s) => println!("Modified: {}", s.name()),
            WatchEvent::Deleted(s) => println!("Deleted {}", s.name()),
            WatchEvent::Error(s) => println!("{}", s),
            _ => (),
        }
    }

    Ok(())
}
