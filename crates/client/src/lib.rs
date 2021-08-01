macro_rules! bail {
  ($err:expr) => {
    return Err($err.into())
  };
}

pub mod cmd;
mod con;
pub mod dev;
mod enc;
mod evt;
mod room;

use std::convert::TryFrom;

pub use con::ConnectError;

use cmd::CommandHandler;
use evt::EventHandler;
use thiserror::Error;
use tokio::net::ToSocketAddrs;
use tracing::{event, instrument, Level};

use crate::{
  dev::{Device, DeviceDesc},
  room::RoomDesc,
};

pub struct EnetClient {
  #[allow(dead_code)]
  commands: CommandHandler,
  #[allow(dead_code)]
  events: EventHandler,
  #[allow(dead_code)]
  rooms: Vec<RoomDesc>,
  devices: Vec<Device>,
}

impl EnetClient {
  #[instrument(level = "info", target = "enet-client", skip(addr), err)]
  pub async fn new<A>(addr: A) -> Result<Self, ClientConnectError>
  where
    A: ToSocketAddrs + Clone + Send + Sync + 'static,
  {
    let mut commands = CommandHandler::new(addr.clone()).await?;
    let version = commands.get_version().await?;
    event!(target: "enet-client", Level::INFO, %version.firmware, %version.hardware, %version.enet, "connected to eNet Gateway");

    let channel_types = commands.get_channel_info().await?;
    let project = commands.get_project().await?;
    let rooms = project
      .lists
      .into_iter()
      .filter(|l| l.visible)
      .map(RoomDesc::from)
      .collect::<Vec<_>>();

    let (writers, devices) = project
      .items
      .into_iter()
      .enumerate()
      .filter(|(idx, _)| channel_types.devices.get(*idx) == Some(&1))
      .filter_map(|(idx, item)| DeviceDesc::try_from(item).ok().map(|v| (idx, v)))
      .map(|(idx, desc)| Device::new(desc, idx as u32))
      .unzip();

    let devices: Vec<_> = devices;
    event!(target: "enet-client", Level::INFO, rooms.len = %rooms.len(), devices.len = %devices.len(), "got project info");

    let events = EventHandler::new(addr, writers).await?;

    Ok(Self {
      commands,
      events,
      rooms,
      devices,
    })
  }

  pub fn devices(&self) -> &[Device] {
    &self.devices
  }
}

#[non_exhaustive]
#[derive(Debug, Error)]
#[error("Failed to connect to gateway.")]
pub enum ClientConnectError {
  Connect(#[from] ConnectError),
  GetVersionCommand(#[from] cmd::GetVersionCommandError),
  GetChannelInfoCommand(#[from] cmd::GetChannelInfoCommandError),
  GetProjectCommand(#[from] cmd::GetProjectCommandError),
}
