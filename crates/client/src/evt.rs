use std::{
  collections::BTreeMap,
  convert::TryFrom,
  ops::ControlFlow,
  time::{Duration, SystemTime},
};

use crate::{
  conn::{Connection, RecvError},
  dev::{DeviceValue, DeviceWriter},
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
  Update(Vec<ItemUpdateValue>),
}

impl<A: ToSocketAddrs + Clone> EventActor<A> {
  fn new(addr: A, recv: mpsc::UnboundedReceiver<ActorMessage>, writers: Vec<DeviceWriter>) -> Self {
    let writers = writers.into_iter().map(|w| (w.index, w)).collect();

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
          event!(target: "enet-client::evt", Level::WARN, msg.kind = ?msg.kind(), "received wrong message kind on event socket - starting connection anew");
          return ControlFlow::Continue(backoff.next_backoff());
        }
      };

      self.update_values(update.values);
    }
  }

  fn handle_msg(&mut self, msg: ActorMessage) {
    match msg {
      ActorMessage::Update(values) => {
        event!(target: "enet-client::evt", Level::DEBUG, "received update for values via actor message");
        event!(target: "enet-client::evt", Level::INFO, "update: {:?}", values);
        self.update_values(values);
      }
    }
  }

  fn update_values(&mut self, values: Vec<ItemUpdateValue>) {
    for value in values {
      let num = value.number;
      let writer = match self.writers.get_mut(&num) {
        None => {
          event!(target: "enet-client::evt", Level::WARN, value.number, %value.value, %value.state, %value.setpoint, "received update for unknown number");
          continue;
        }
        Some(v) => v,
      };

      event!(target: "enet-client::evt", Level::DEBUG, value.number, %value.value, %value.state, %value.setpoint, device.kind = ?writer.desc.kind, device.name = %writer.desc.name, "received update for value");
      let value = match DeviceValue::try_from(value) {
        Ok(v) => v,
        Err(value) => {
          event!(target: "enet-client::evt", Level::WARN, value.number, %value.value, %value.state, %value.setpoint, device.kind = ?writer.desc.kind, device.name = %writer.desc.name, "failed to convert to DeviceValue");
          continue;
        }
      };

      writer.writer.write(value);
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

  pub(crate) fn update_values(&mut self, values: Vec<ItemUpdateValue>) -> Result<(), ()> {
    self
      .sender
      .send(ActorMessage::Update(values))
      .map_err(|_| ())
  }
}
