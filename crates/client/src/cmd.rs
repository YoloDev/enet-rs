use crate::{
  conn::{Connection, RecvError, SendError},
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
  time::Duration,
};
use thiserror::Error;
use tokio::{
  net::ToSocketAddrs,
  sync::{mpsc, oneshot},
};
use tracing::{event, Level};

struct CommandActor<A>
where
  A: ToSocketAddrs + Clone + Send + Sync,
{
  conn: Option<Connection>,
  addr: A,
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
        $res(oneshot::Sender<Result<$ty, CommandError>>),
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

      fn error(self, error: CommandError) -> Result<(), CommandError> {
        match self {
          $(
            Self::$res(sender) => sender.send(Err(error)).map_err(Result::unwrap_err),
          )*
        }
      }
    }

    $(
      impl From<oneshot::Sender<Result<$ty, CommandError>>> for ResponseListener {
        #[inline]
        fn from(sender: oneshot::Sender<Result<$ty, CommandError>>) -> Self {
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

impl<A> CommandActor<A>
where
  A: ToSocketAddrs + Clone + Send + Sync,
{
  fn new(conn: Connection, addr: A, recv: mpsc::Receiver<ActorMessage>) -> Self {
    Self {
      conn: Some(conn),
      addr,
      recv,
      response_listener: None,
    }
  }

  async fn run(mut self) {
    loop {
      let result = if let Some(conn) = self.conn.as_mut() {
        let sleep = tokio::time::sleep(Duration::from_secs(15));

        tokio::select! {
          enet = conn.recv() => self.handle_enet(enet).await,
          cmd = self.recv.recv() => self.handle_cmd(cmd).await,
          _ = sleep => self.sleep().await,
        }
      } else {
        let cmd = self.recv.recv().await;
        self.handle_cmd(cmd).await
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
        if let Some(listener) = self.response_listener.take() {
          let _ = listener.error(ConnectionClosed.into());
        }
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
          let _ = listener.error(msg.into());
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
        let conn = match self.conn.as_mut() {
          Some(conn) => conn,
          None => {
            event!(target: "enet-client::cmd", Level::INFO, "Establishing new connection to eNet gateway.");
            let conn = match Connection::new(self.addr.clone()).await {
              Ok(conn) => conn,
              Err(e) => {
                event!(target: "enet-client::cmd", Level::ERROR, "Failed to establish connection to eNet gateway: {:?}", e);
                return Err(());
              }
            };

            self.conn = Some(conn);
            self.conn.as_mut().unwrap()
          }
        };

        let kind = req.body.kind();
        event!(target: "enet-client::cmd", Level::INFO, message.kind = ?kind, "Sending message");
        match conn.send(&req).await {
          Ok(()) => (),
          Err(e) => {
            event!(target: "enet-client::cmd", Level::WARN, message.kind = ?kind, "Message failed to send");
            if let Some(listener) = self.response_listener.take() {
              let _ = listener.error(e.into());
            }
          }
        }
      }
    }

    Ok(())
  }

  async fn sleep(&mut self) -> Result<(), ()> {
    event!(target: "enet-client::cmd", Level::INFO, "Closing command connection after 15 seconds of innactivity.");
    self.conn.take(); // drop connection

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
  pub(crate) async fn new(
    addr: impl ToSocketAddrs + Clone + Send + Sync + 'static,
  ) -> Result<Self, ConnectError> {
    let conn = Connection::new(addr.clone()).await?;
    let (sender, recv) = mpsc::channel(10);
    tokio::spawn(CommandActor::new(conn, addr, recv).run());

    Ok(Self { sender })
  }

  async fn send<C>(&mut self, command: C) -> Result<C::Response, CommandError>
  where
    C: Command,
    oneshot::Sender<Result<C::Response, CommandError>>: Into<ResponseListener>,
  {
    let envelope = RequestEnvelope::new(command);
    let (sender, receiver) = oneshot::channel::<Result<C::Response, CommandError>>();
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

  WrongResponse(Response),
}

impl From<Response> for CommandError {
  fn from(r: Response) -> CommandError {
    CommandError::WrongResponse(r)
  }
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
