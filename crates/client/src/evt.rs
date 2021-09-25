use std::{
  collections::BTreeMap,
  ops::ControlFlow,
  str::FromStr,
  time::{Duration, SystemTime},
};

use crate::{
  conn::{Connection, RecvError},
  dev::{DeviceBrightness, DeviceState, DeviceWriter},
  ConnectError,
};
use backoff::{backoff::Backoff, ExponentialBackoff};
use enet_proto::{ItemUpdateValue, ItemValueSignInReq, RequestEnvelope, Response};
use tokio::{net::ToSocketAddrs, sync::mpsc};
use tracing::{event, Level};

struct EventActor<A: ToSocketAddrs + Clone> {
  addr: A,
  recv: mpsc::UnboundedReceiver<ActorMessage>,
  writers: BTreeMap<u32, DeviceWriter>,
}

enum ActorMessage {
  SetStates(Vec<(u32, DeviceState)>),
}

impl<A: ToSocketAddrs + Clone> EventActor<A> {
  fn new(addr: A, recv: mpsc::UnboundedReceiver<ActorMessage>, writers: Vec<DeviceWriter>) -> Self {
    let writers = writers.into_iter().map(|w| (w.index(), w)).collect();

    Self {
      addr,
      recv,
      writers,
    }
  }

  async fn run(mut self) {
    let mut backoff = ExponentialBackoff::default();

    loop {
      let sleep_time = self.main(&mut backoff).await;
      match sleep_time {
        ControlFlow::Break(()) => return,
        ControlFlow::Continue(None) => {
          event!(target: "enet-client::evt", Level::WARN, "ran out of retries - panicing");
          panic!("event connection ran out of retries.");
        }
        ControlFlow::Continue(Some(duration)) => tokio::time::sleep(duration).await,
      }
    }
  }

  async fn main(&mut self, backoff: &mut impl Backoff) -> ControlFlow<(), Option<Duration>> {
    let mut conn = match Connection::new(self.addr.clone()).await {
      Ok(conn) => conn,
      Err(e) => {
        event!(target: "enet-client::evt", Level::WARN, "failed to open event connection to enet: {:?}", e);
        return ControlFlow::Continue(backoff.next_backoff());
      }
    };

    let subscribe_req = ItemValueSignInReq::new(self.writers.keys().copied().collect());
    let subscribe_msg = RequestEnvelope::new(subscribe_req.clone());
    if let Err(e) = conn.send(&subscribe_msg).await {
      event!(target: "enet-client::evt", Level::WARN, "failed to send subscribe message to enet: {:?}", e);
      return ControlFlow::Continue(backoff.next_backoff());
    }

    let mut then = SystemTime::now();
    loop {
      let duration = SystemTime::now().duration_since(then).unwrap();
      let wait_time = Duration::from_secs(60 * 5) - duration;

      let msg = tokio::select! {
        v = self.recv.recv() => {
          match v {
            None =>
            return ControlFlow::Break(()),
            Some(v) => {
              self.handle_msg(v);
              continue;
            }
          }
        }
        enet = conn.recv() => enet,
        _ = tokio::time::sleep(wait_time) => {
          let subscribe_msg = RequestEnvelope::new(subscribe_req.clone());
          if let Err(e) = conn.send(&subscribe_msg).await {
            event!(target: "enet-client::evt", Level::WARN, "failed to send subscribe message to enet: {:?}", e);
            return ControlFlow::Continue(backoff.next_backoff());
          }
          then = SystemTime::now();
          continue;
        }
      };

      event!(target: "enet-client::evt", Level::DEBUG, "received message on evt connection");
      let msg = match msg {
        Result::Ok(v) => v,
        Result::Err(RecvError::Closed(_)) => {
          event!(target: "enet-client::evt", Level::ERROR, "connection closed");
          return ControlFlow::Continue(backoff.next_backoff());
        }
        Result::Err(error) => {
          event!(target: "enet-client::evt", Level::WARN, ?error, "error when receiving event");
          return ControlFlow::Continue(backoff.next_backoff());
        }
      };

      let update = match msg {
        Response::ItemUpdate(upd) => {
          backoff.reset();
          upd
        }
        Response::ItemValueSignIn(_) => {
          backoff.reset();
          continue;
        }
        _ => {
          event!(
            target: "enet-client::evt",
            Level::WARN,
            msg.kind = ?msg.kind(),
            "received wrong message kind on event socket - starting connection anew");
          return ControlFlow::Continue(backoff.next_backoff());
        }
      };

      self.update_values_from_enet(update.values);
    }
  }

  fn handle_msg(&mut self, msg: ActorMessage) {
    match msg {
      ActorMessage::SetStates(values) => {
        event!(target: "enet-client::evt", Level::DEBUG, "received update for values via actor message");
        self.update_device_states(values);
      }
    }
  }

  fn update_values_from_enet(&mut self, values: Vec<ItemUpdateValue>) {
    for value in values {
      let num = value.number;
      let writer = match self.writers.get_mut(&num) {
        None => {
          event!(
            target: "enet-client::evt",
            Level::WARN,
            value.number,
            %value.value,
            %value.state,
            %value.setpoint,
            "received update for unknown number");
          continue;
        }
        Some(v) => v,
      };

      event!(
        target: "enet-client::evt",
        Level::DEBUG,
        value.number,
        %value.value,
        %value.state,
        %value.setpoint,
        device.kind = ?writer.kind(),
        device.name = %writer.name(),
        "received update for value");

      match writer {
        DeviceWriter::Binary(w) => {
          if let Ok(state) = DeviceState::from_str(&*value.state) {
            w.state_writer.write(state);
          } else {
            event!(
              target: "enet-client::evt",
              Level::WARN,
              value.number,
              %value.value,
              %value.state,
              %value.setpoint,
              device.kind = ?w.kind(),
              device.name = %w.name(),
              "failed to convert '{}' to DeviceState",
              value.state);
          }
        }
        DeviceWriter::Dimmer(w) => {
          if let Ok(state) = DeviceState::from_str(&*value.state) {
            w.state_writer.write(state);
          } else {
            event!(
              target: "enet-client::evt",
              Level::WARN,
              value.number,
              %value.value,
              %value.state,
              %value.setpoint,
              device.kind = ?w.kind(),
              device.name = %w.name(),
              "failed to convert '{}' to DeviceState",
              value.state);
          }

          if let Ok(brightness) = DeviceBrightness::from_str(&*value.value) {
            w.brightness_writer.write(brightness);
          } else if &*value.value == "-1" {
            w.brightness_writer.write(DeviceBrightness::MIN);
          } else {
            event!(
              target: "enet-client::evt",
              Level::WARN,
              value.number,
              %value.value,
              %value.state,
              %value.setpoint,
              device.kind = ?w.kind(),
              device.name = %w.name(),
              "failed to convert '{}' to DeviceBrightness",
              value.value);
          }
        }
      }
    }
  }

  fn update_device_states(&mut self, values: Vec<(u32, DeviceState)>) {
    for (num, state) in values {
      let writer = match self.writers.get_mut(&num) {
        None => {
          event!(target: "enet-client::evt", Level::WARN, value.number = num, value.state = %state, "received update for unknown number");
          continue;
        }
        Some(v) => v,
      };

      event!(target: "enet-client::evt", Level::DEBUG, value.number = num, value.state = %state, device.kind = ?writer.kind(), device.name = %writer.name(), "received manual update for value");
      match writer {
        DeviceWriter::Binary(w) => w.state_writer.write(state),
        DeviceWriter::Dimmer(w) => w.state_writer.write(state),
      }
    }
  }
}

pub(crate) struct EventHandler {
  sender: mpsc::UnboundedSender<ActorMessage>,
}

impl EventHandler {
  pub(crate) async fn new(
    addr: impl ToSocketAddrs + Clone + Send + Sync + 'static,
    writers: Vec<DeviceWriter>,
  ) -> Result<Self, ConnectError> {
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(EventActor::new(addr, receiver, writers).run());

    Ok(Self { sender })
  }

  pub(crate) fn update_values(&mut self, values: Vec<(u32, DeviceState)>) -> Result<(), ()> {
    self
      .sender
      .send(ActorMessage::SetStates(values))
      .map_err(|_| ())
  }
}
