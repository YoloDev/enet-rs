use std::{
  collections::BTreeMap,
  convert::TryFrom,
  time::{Duration, SystemTime},
};

use crate::{
  con::{Connection, RecvError},
  dev::{DeviceValue, DeviceWriter},
  ConnectError,
};
use enet_proto::{ItemValueSignInReq, RequestEnvelope, Response};
use tokio::{net::ToSocketAddrs, sync::oneshot};
use tracing::{event, Level};

struct EventActor<A: ToSocketAddrs + Clone> {
  addr: A,
  probe: oneshot::Receiver<()>,
  writers: BTreeMap<u32, DeviceWriter>,
}

impl<A: ToSocketAddrs + Clone> EventActor<A> {
  fn new(addr: A, probe: oneshot::Receiver<()>, writers: Vec<DeviceWriter>) -> Self {
    let writers = writers.into_iter().map(|w| (w.index, w)).collect();

    Self {
      addr,
      probe,
      writers,
    }
  }

  async fn run(self) {
    let addr = self.addr;
    let mut writers = self.writers;
    let mut probe = self.probe;
    let mut conn = Connection::new(addr.clone()).await.unwrap();
    let mut then = SystemTime::now();

    // let subscribe_msg = ItemValueSignInReq::new(writers.keys().collect());
    // let subscribe_msg = Request::from(subscribe_msg);
    let subscribe_msg =
      RequestEnvelope::new(ItemValueSignInReq::new(writers.keys().copied().collect()));
    let _ = conn.send(&subscribe_msg).await;

    loop {
      let duration = SystemTime::now().duration_since(then).unwrap();
      let wait_time = Duration::from_secs(60 * 5) - duration;

      let msg = tokio::select! {
        _ = (&mut probe) => return,
        enet = conn.recv() => enet,
        _ = tokio::time::sleep(wait_time) => {
          let subscribe_msg =
            RequestEnvelope::new(ItemValueSignInReq::new(writers.keys().copied().collect()));
          let _ = conn.send(&subscribe_msg).await;
          then = SystemTime::now();
          continue;
        }
      };

      event!(target: "enet-client::evt", Level::DEBUG, "received message on evt connection");
      let msg = match msg {
        Result::Ok(v) => v,
        Result::Err(RecvError::Closed(_)) => {
          event!(target: "enet-client::evt", Level::ERROR, "connection closed");
          return;
        }
        Result::Err(error) => {
          event!(target: "enet-client::evt", Level::WARN, ?error, "error when receiving event");
          continue;
        }
      };

      let update = match msg {
        Response::ItemUpdate(upd) => upd,
        Response::ItemValueSignIn(_) => continue,
        _ => {
          event!(target: "enet-client::evt", Level::WARN, msg.kind = ?msg.kind(), "received wrong message kind on event socket");
          continue;
        }
      };

      for value in update.values {
        let num = value.number;
        let writer = match writers.get_mut(&num) {
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
}

pub(crate) struct EventHandler {
  #[allow(dead_code)]
  handle: oneshot::Sender<()>,
}

impl EventHandler {
  pub(crate) async fn new(
    addr: impl ToSocketAddrs + Clone + Send + Sync + 'static,
    writers: Vec<DeviceWriter>,
  ) -> Result<Self, ConnectError> {
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(EventActor::new(addr, receiver, writers).run());

    Ok(Self { handle: sender })
  }
}
