use crate::enc::{EnetDecoder, EnetDecoderError, EnetEncoder, EnetEncoderError};
use enet_proto::{RequestEnvelope, Response};
use futures::{SinkExt, StreamExt};
use thiserror::Error;
use tokio::{
  io,
  net::{
    tcp::{OwnedReadHalf, OwnedWriteHalf},
    TcpStream, ToSocketAddrs,
  },
};
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing::instrument;

pub(crate) struct Connection {
  reader: FramedRead<OwnedReadHalf, EnetDecoder>,
  writer: FramedWrite<OwnedWriteHalf, EnetEncoder>,
}

impl Connection {
  pub(crate) async fn new(addr: impl ToSocketAddrs) -> Result<Self, ConnectError> {
    let stream = TcpStream::connect(addr).await?;
    let (reader, writer) = stream.into_split();

    Ok(Self {
      reader: FramedRead::new(reader, EnetDecoder::new()),
      writer: FramedWrite::new(writer, EnetEncoder::new()),
    })
  }

  #[instrument(level = "debug", target = "enet-client::con", skip(self, message), err)]
  pub(crate) async fn send(&mut self, message: &RequestEnvelope) -> Result<(), SendError> {
    Ok(self.writer.send(message).await?)
  }

  #[instrument(level = "debug", target = "enet-client::con", skip(self), err)]
  pub(crate) async fn recv(&mut self) -> Result<Response, RecvError> {
    match self.reader.next().await {
      Some(result) => Ok(result?),
      None => bail!(ConnectionClosed),
    }
  }
}

#[non_exhaustive]
#[derive(Debug, Error)]
#[error("Failed to connect to gateway.")]
pub enum ConnectError {
  FailedToConnect(#[from] io::Error),
}

#[non_exhaustive]
#[derive(Debug, Error)]
#[error("Failed to send message.")]
pub enum SendError {
  FailedToSend(#[from] EnetEncoderError),
}

#[non_exhaustive]
#[derive(Debug, Error)]
#[error("Failed to receive response.")]
pub enum RecvError {
  DecoderError(#[from] EnetDecoderError),

  Closed(#[from] ConnectionClosed),
}

#[derive(Debug, Error)]
#[error("Connection closed.")]
pub struct ConnectionClosed;
