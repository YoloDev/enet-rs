//! Client for interacting with eNet gateways.

macro_rules! bail {
  ($err:expr) => {
    return Err($err.into())
  };
}

pub mod cmd;
mod conn;
pub mod dev;
mod enc;
mod evt;
mod room;

use std::convert::TryFrom;

pub use cmd::SetValuesCommandError;
pub use conn::ConnectError;
use dev::DeviceState;
pub use dev::{BinaryDevice, Device, DimmerDevice, EnetDevice};
pub use enet_proto::{ClickDuration, ItemSetValue, ItemValueRes, SetValue};

use crate::{dev::DeviceDesc, room::RoomDesc};
use cmd::CommandHandler;
use evt::EventHandler;
use thiserror::Error;
use tokio::net::ToSocketAddrs;
use tracing::{event, instrument, Level};

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

  pub fn device(&self, number: u32) -> Option<&Device> {
    self.devices.iter().find(|d| d.number() == number)
  }

  pub async fn set_value(
    &mut self,
    number: u32,
    value: SetValue,
  ) -> Result<(), SetValuesCommandError> {
    let values = vec![ItemSetValue { number, value }];
    self.set_values(values).await
  }

  pub async fn set_values(
    &mut self,
    values: impl IntoIterator<Item = ItemSetValue>,
  ) -> Result<(), SetValuesCommandError> {
    let values: Vec<ItemSetValue> = values.into_iter().collect();
    let new_states = values
      .iter()
      .map(|v| (v.number, DeviceState::from(v.value)))
      .collect();

    self.commands.set_values(values).await?;
    let _ = self.events.update_values(new_states);

    Ok(())
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
