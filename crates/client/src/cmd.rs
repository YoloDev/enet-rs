use crate::{
  con::{Connection, RecvError, SendError},
  ConnectError,
};
use enet_proto::{
  GetChannelInfoAllReq, GetChannelInfoAllRes, ItemSetValue, ItemValueRes, ItemValueSetReq,
  ProjectListReq, ProjectListRes, RequestEnvelope, RequestType, Response, VersionReq, VersionRes,
};
use paste::paste;
use std::{
  convert::{TryFrom, TryInto},
  fmt,
};
use thiserror::Error;
use tokio::{
  net::ToSocketAddrs,
  sync::{mpsc, oneshot},
};
use tracing::{event, Level};

struct CommandActor {
  conn: Connection,
  recv: mpsc::Receiver<ActorMessage>,
  response_listener: Option<ResponseListener>,
}

enum ActorMessage {
  Send(RequestEnvelope, ResponseListener),
}

macro_rules! define_response_listener {
  ($($res:ident($ty:ty)),*$(,)?) => {
    enum ResponseListener {
      $(
        $res(oneshot::Sender<Result<$ty, SendError>>),
      )*
    }

    impl ResponseListener {
      fn accept(self, res: Response) -> Result<(), (Option<Self>, Response)> {
        match self {
          $(
            Self::$res(sender) => {
              match res.try_into() {
                Ok(msg) => {
                  match sender.send(Ok(msg)) {
                    Ok(()) => Ok(()),
                    Err(msg) => Err((None, msg.unwrap().into())),
                  }
                }

                Err(res) => Err((Some(Self::$res(sender)), res))
              }
            }
          )*
        }
      }

      fn error(self, error: SendError) -> Result<(), SendError> {
        match self {
          $(
            Self::$res(sender) => sender.send(Err(error)).map_err(Result::unwrap_err),
          )*
        }
      }
    }

    $(
      impl From<oneshot::Sender<Result<$ty, SendError>>> for ResponseListener {
        #[inline]
        fn from(sender: oneshot::Sender<Result<$ty, SendError>>) -> Self {
          Self::$res(sender)
        }
      }
    )*
  };
}

define_response_listener! {
  Version(VersionRes),
  GetChannelInfoAll(GetChannelInfoAllRes),
  GetProject(ProjectListRes),
  ItemValue(ItemValueRes),
}

impl CommandActor {
  fn new(conn: Connection, recv: mpsc::Receiver<ActorMessage>) -> Self {
    Self {
      conn,
      recv,
      response_listener: None,
    }
  }

  async fn run(mut self) {
    loop {
      let result = tokio::select! {
        enet = self.conn.recv() => self.handle_enet(enet).await,
        cmd = self.recv.recv() => self.handle_cmd(cmd).await,
      };

      match result {
        Ok(()) => (),
        Err(()) => return,
      }
    }
  }

  async fn handle_enet(&mut self, msg: Result<Response, RecvError>) -> Result<(), ()> {
    let msg = match msg {
      Ok(v) => v,
      Err(e) => {
        event!(target: "enet-client::cmd", Level::ERROR, error = ?e, "connection closed");
        return Err(());
      }
    };

    event!(target: "enet-client::cmd", Level::INFO, message.kind = ?msg.kind(), "received message");
    match self.response_listener.take() {
      None => {
        event!(target: "enet-client::cmd", Level::WARN, message.kind = ?msg.kind(), "no listener available");
        Ok(())
      }
      Some(listener) => match listener.accept(msg) {
        Ok(()) => Ok(()),
        Err((None, msg)) => {
          event!(target: "enet-client::cmd", Level::INFO, message.kind = ?msg.kind(), "listener closed");
          Ok(())
        }
        Err((Some(listener), msg)) => {
          event!(target: "enet-client::cmd", Level::WARN, message.kind = ?msg.kind(), "wrong listener available");
          self.response_listener = Some(listener);
          Ok(())
        }
      },
    }
  }

  async fn handle_cmd(&mut self, msg: Option<ActorMessage>) -> Result<(), ()> {
    let msg = if let Some(msg) = msg {
      msg
    } else {
      return Err(());
    };

    match msg {
      ActorMessage::Send(req, res) => {
        self.response_listener = Some(res);
        let kind = req.body.kind();
        event!(target: "enet-client::cmd", Level::INFO, message.kind = ?kind, "sending message");
        match self.conn.send(&req).await {
          Ok(()) => (),
          Err(e) => {
            event!(target: "enet-client::cmd", Level::WARN, message.kind = ?kind, "message failed to send");
            if let Some(listener) = self.response_listener.take() {
              let _ = listener.error(e);
            }
          }
        }
      }
    }
    Ok(())
  }
}

trait Command: RequestType {
  type Response: TryFrom<Response>;
}

pub(crate) struct CommandHandler {
  sender: mpsc::Sender<ActorMessage>,
}

impl CommandHandler {
  pub(crate) async fn new(addr: impl ToSocketAddrs) -> Result<Self, ConnectError> {
    let conn = Connection::new(addr).await?;
    let (sender, recv) = mpsc::channel(10);
    tokio::spawn(CommandActor::new(conn, recv).run());

    Ok(Self { sender })
  }

  async fn send<C>(&mut self, command: C) -> Result<C::Response, CommandError>
  where
    C: Command,
    oneshot::Sender<Result<C::Response, SendError>>: Into<ResponseListener>,
  {
    let envelope = RequestEnvelope::new(command);
    let (sender, receiver) = oneshot::channel::<Result<C::Response, SendError>>();
    let msg = ActorMessage::Send(envelope, sender.into());
    self.sender.send(msg).await?;

    Ok(receiver.await??)
  }
}

macro_rules! define_command {
  ($name:ident$((
    $($arg_i:ident : $arg_t:ty),*$(,)?
  ))? => $req:ty => $res:ty) => {
    impl Command for $req {
      type Response = $res;
    }

    paste! {
      #[non_exhaustive]
      #[derive(Debug, Error)]
      pub enum [<$name:camel CommandError>] {
        Command(#[from] CommandError),
      }

      impl fmt::Display for [<$name:camel CommandError>] {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
          f.write_str("Failed to send '")?;
          f.write_str(stringify!($name))?;
          f.write_str("' command.")
        }
      }

      impl CommandHandler {
        #[allow(dead_code)]
        pub(crate) async fn $name(
          &mut self,
          $($($arg_i: $arg_t,)*)?
        ) -> Result<$res, [<$name:camel CommandError>]> {
          let req = $req::new ($(
            $($arg_i,)*
          )?);

          Ok(self.send(req).await?)
        }
      }
    }
  };
}

define_command!(get_version => VersionReq => VersionRes);
define_command!(get_channel_info => GetChannelInfoAllReq => GetChannelInfoAllRes);
define_command!(get_project => ProjectListReq => ProjectListRes);
define_command!(set_values(values: Vec<ItemSetValue>) => ItemValueSetReq => ItemValueRes);

#[non_exhaustive]
#[derive(Debug, Error)]
#[error("Failed to send command.")]
pub enum CommandError {
  SendError(#[from] SendError),

  RecvError(#[from] RecvError),

  ConnectionClosed(#[from] ConnectionClosed),

  NoResponse(#[from] NoResponse),
}

impl From<mpsc::error::SendError<ActorMessage>> for CommandError {
  #[inline]
  fn from(_: mpsc::error::SendError<ActorMessage>) -> Self {
    CommandError::ConnectionClosed(ConnectionClosed)
  }
}

impl From<oneshot::error::RecvError> for CommandError {
  #[inline]
  fn from(_: oneshot::error::RecvError) -> Self {
    CommandError::NoResponse(NoResponse)
  }
}

#[derive(Debug, Error)]
#[error("Connection closed.")]
pub struct ConnectionClosed;

#[derive(Debug, Error)]
#[error("Actor did not respond closed.")]
pub struct NoResponse;
