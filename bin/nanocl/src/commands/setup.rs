use std::time::Duration;

use users::get_group_by_name;

use bollard_next::container::StartContainerOptions;
use nanocld_client::stubs::state::StateDeployment;

use crate::utils;
use crate::error::CliError;
use crate::models::{SetupOpts, NanocldArgs};

pub async fn exec_setup(options: &SetupOpts) -> Result<(), CliError> {
  println!("Installing nanocl daemon on your system");

  let docker_host = options
    .docker_host
    .as_deref()
    .unwrap_or("unix:///var/run/docker.sock")
    .to_owned();

  let state_dir = options
    .state_dir
    .as_deref()
    .unwrap_or("/var/lib/nanocl")
    .to_owned();

  let conf_dir = options
    .conf_dir
    .as_deref()
    .unwrap_or("/etc/nanocl")
    .to_owned();

  let gateway = match &options.gateway {
    None => {
      let gateway =
        utils::network::get_default_ip().map_err(|err| CliError::Custom {
          msg: format!("Cannot find default gateway: {err}"),
        })?;
      println!("Using default gateway: {}", gateway);
      gateway.to_string()
    }
    Some(gateway) => gateway.clone(),
  };

  let advertise_addr = match &options.advertise_addr {
    None => gateway.clone(),
    Some(advertise_addr) => advertise_addr.clone(),
  };

  let group = options.group.as_deref().unwrap_or("nanocl");

  let hosts = options
    .deamon_hosts
    .clone()
    .unwrap_or(vec!["unix:///run/nanocl/nanocl.sock".into()]);

  let gid = get_group_by_name(group).ok_or(CliError::Custom {
    msg: format!(
      "Error cannot find group: {group}\n\
    You can create it with: sudo groupadd {group}\n\
    And be sure to add yourself to it: sudo usermod -aG {group} $USER\n\
    Then update your current session: newgrp {group}\n\
    And try again"
    ),
  })?;

  let hostname = if let Some(hostname) = &options.hostname {
    hostname.to_owned()
  } else {
    let hostname =
      utils::network::get_hostname().map_err(|err| CliError::Custom {
        msg: format!("Cannot find hostname: {err}"),
      })?;
    println!("Using default hostname: {hostname}");
    hostname
  };

  let args = NanocldArgs {
    docker_host,
    state_dir,
    conf_dir,
    gateway,
    hosts,
    gid: gid.gid(),
    hostname,
    advertise_addr,
  };

  let installer = utils::state::compile(
    include_str!("../../installer.yml"),
    &args.clone().into(),
  )?;

  println!("{installer}");

  let value =
    serde_yaml::from_str::<serde_yaml::Value>(&installer).map_err(|err| {
      CliError::Custom {
        msg: format!("Cannot parse installer: {err}"),
      }
    })?;

  let deployment =
    serde_yaml::from_value::<StateDeployment>(value).map_err(|err| {
      CliError::Custom {
        msg: format!("Cannot parse installer: {err}"),
      }
    })?;

  let cargoes = deployment.cargoes.ok_or(CliError::Custom {
    msg: "Cannot find cargoes in installer".into(),
  })?;

  let docker = utils::docker::connect(&args.docker_host)?;

  for cargo in cargoes {
    let image = cargo.container.image.clone().ok_or(CliError::Custom {
      msg: format!("Cannot find image in cargo {}", cargo.name),
    })?;
    let mut image_detail = image.split(':');
    let from_image = image_detail.next().ok_or(CliError::Custom {
      msg: format!("Cannot find image in cargo {}", cargo.name),
    })?;
    let tag = image_detail.next().ok_or(CliError::Custom {
      msg: format!("Cannot find image tag in cargo {}", cargo.name),
    })?;
    utils::docker::install_image(from_image, tag, &docker).await?;
    let container = utils::docker::create_cargo_container(
      &cargo,
      &deployment.namespace.clone().unwrap_or("system".into()),
      &docker,
    )
    .await?;
    docker
      .start_container(&container.id, None::<StartContainerOptions<String>>)
      .await?;
    ntex::time::sleep(Duration::from_secs(2)).await;
  }

  println!("Congratz! Nanocl is now ready to use!");
  Ok(())
}
