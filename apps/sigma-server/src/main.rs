use std::{env, time::Duration};

use anyhow::{Context, Result};

use bevy::{log::LogPlugin, prelude::*, tasks::prelude::*};

use async_compat::Compat;

use tokio::{signal, time::sleep};

use k8s_openapi::{
    api::core::v1::{EndpointAddress, EndpointPort, EndpointSubset, Endpoints, Pod, Service},
    apimachinery::pkg::util::intstr::IntOrString,
    serde_json,
};

use kube::{
    api::{Patch, PatchParams},
    Api, Client,
};

use tiny_http::{Method, Response, Server};

async fn assign_public_webrtc_url() -> Result<String> {
    let hostname = env::var("HOSTNAME")?;

    let client = Client::try_default().await?;

    let pod_api: Api<Pod> = Api::default_namespaced(client.clone());

    loop {
        match pod_api
            .get(hostname.as_str())
            .await?
            .status
            .as_ref()
            .context("pod status")?
            .pod_ip
            .as_ref()
        {
            Some(_) => break,
            None => {
                sleep(Duration::from_secs(1)).await;
            }
        };
    }

    let pod = pod_api.get(hostname.as_str()).await?;
    let node_name = pod
        .spec
        .as_ref()
        .context("pod spec")?
        .node_name
        .as_ref()
        .context("pod node name")?;
    let pod_status = pod.status.as_ref().context("pod status")?;
    let pod_host_ip = pod_status.host_ip.as_ref().context("pod host ip")?;

    let service_api: Api<Service> = Api::default_namespaced(client.clone());
    let service = service_api.get("sigma-server-rtc").await?;
    let service_ports = service
        .spec
        .as_ref()
        .context("service spec")?
        .ports
        .as_ref()
        .context("service ports")?;

    let endpoints_api: Api<Endpoints> = Api::default_namespaced(client);
    let mut endpoints = endpoints_api.get("sigma-server-rtc").await?;
    let endpoints_name = endpoints.metadata.name.as_ref().context("endpoints name")?;
    let endpoints_subsets = endpoints.subsets.get_or_insert(default());

    let endpoints_address = EndpointAddress {
        ip: pod_status.pod_ip.clone().context("pod ip")?,
        node_name: Some(node_name.clone()),
        ..default()
    };

    let rtc_port = endpoints_subsets
        .iter_mut()
        .find_map(|endpoints_subset| {
            endpoints_subset.ports.as_ref().and_then(|endpoints_port| {
                endpoints_port.iter().find_map(|enpoints_port| {
                    if !enpoints_port
                        .name
                        .as_ref()
                        .map(|endpoints_port_name| endpoints_port_name.starts_with("rtc-"))
                        .unwrap_or(false)
                    {
                        return None;
                    }

                    if let Some(endpoints_addresses) = endpoints_subset.addresses.as_ref() {
                        if endpoints_addresses
                            .iter()
                            .any(|a| a.node_name.as_ref() == Some(node_name))
                        {
                            return None;
                        }
                    }

                    endpoints_subset
                        .addresses
                        .get_or_insert_with(|| default())
                        .push(endpoints_address.clone());

                    Some(enpoints_port.port)
                })
            })
        })
        .unwrap_or_else(|| {
            let rtc_port = 4200
                + i32::try_from(
                    service_ports
                        .iter()
                        .filter(|service_port| {
                            service_port.target_port == Some(IntOrString::Int(4200))
                        })
                        .count(),
                )
                .unwrap();

            endpoints_subsets.push(EndpointSubset {
                ports: Some(vec![EndpointPort {
                    name: Some(format!("rtc-{}", rtc_port)),
                    port: rtc_port,
                    protocol: Some("UDP".to_string()),
                    ..default()
                }]),
                addresses: Some(vec![endpoints_address]),
                ..default()
            });

            rtc_port
        });

    let node_port = match service_ports
        .iter()
        .find(|service_port| service_port.port == rtc_port)
    {
        Some(service_port) => service_port.node_port.context("service node port")?,
        None => {
            let patched_service = service_api
                .patch(
                    service.metadata.name.as_ref().context("service name")?,
                    &PatchParams::apply("sigma-server"),
                    &Patch::Strategic(serde_json::json!({
                        "spec": {
                            "ports": [{
                                "name": format!("rtc-{}", rtc_port),
                                "port": rtc_port,
                                "targetPort": IntOrString::Int(4200),
                                "protocol": "UDP"
                            }]
                        }
                    })),
                )
                .await?;

            patched_service
                .spec
                .as_ref()
                .context("patched service spec")?
                .ports
                .as_ref()
                .context("patched service ports")?
                .iter()
                .find(|service_port| service_port.port == rtc_port)
                .context("patched service port")?
                .node_port
                .context("patched service node port")?
        }
    };

    endpoints_api
        .patch(
            endpoints_name,
            &PatchParams::apply("sigma-server"),
            &Patch::Strategic(serde_json::json!({ "subsets": endpoints_subsets })),
        )
        .await?;

    Ok(format!("http://{}:{}", pod_host_ip, node_port))
}

fn startup_system(io_task_pool: Res<IoTaskPool>) {
    let public_webrtc_url = io_task_pool
        .scope(|scope| {
            scope.spawn(Compat::new(async {
                assign_public_webrtc_url().await.unwrap()
            }))
        })
        .first()
        .unwrap()
        .clone();

    info!("public webrtc url: {}", public_webrtc_url);

    io_task_pool
        .spawn(async {
            let server = Server::http("0.0.0.0:8080").unwrap();

            for request in server.incoming_requests() {
                if *request.method() == Method::Get && request.url() == "/health" {
                    request.respond(Response::empty(200)).unwrap();
                } else {
                    request.respond(Response::empty(404)).unwrap();
                }
            }
        })
        .detach();

    io_task_pool
        .spawn(async {
            signal::ctrl_c().await.unwrap();

            info!("shutdown requested");

            // TODO: shutdown gracefully: remove port assignment, etc.
        })
        .detach();
}

fn main() {
    // NOTE: should not add console hook for web, since bevy alreayd go it (use features to disable by default, enable on release: beavy/.../conso...)

    App::new()
        .add_plugins(MinimalPlugins)
        .add_plugin(LogPlugin::default())
        .add_startup_system(startup_system)
        .run();
}
