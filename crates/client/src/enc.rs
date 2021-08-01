use enet_proto::{RequestEnvelope, Response};
use thiserror::Error;
use tokio::io;
use tokio_util::codec::{Decoder, Encoder};

#[derive(Default)]
pub(crate) struct EnetEncoder(enet_proto::EnetEncoder);

impl EnetEncoder {
  pub(crate) fn new() -> Self {
    Self::default()
  }
}

impl<'a> Encoder<&'a RequestEnvelope> for EnetEncoder {
  type Error = EnetEncoderError;

  fn encode(
    &mut self,
    item: &'a RequestEnvelope,
    dst: &mut bytes::BytesMut,
  ) -> Result<(), Self::Error> {
    self.0.encode(item, dst).map_err(Into::into)
  }
}

#[derive(Default)]
pub(crate) struct EnetDecoder(enet_proto::EnetDecoder);

impl EnetDecoder {
  pub(crate) fn new() -> Self {
    Self::default()
  }
}

impl Decoder for EnetDecoder {
  type Item = Response;
  type Error = EnetDecoderError;

  fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
    self.0.decode(src).map_err(Into::into)
  }
}

#[non_exhaustive]
#[derive(Debug, Error)]
pub enum EnetEncoderError {
  #[error(transparent)]
  Wrapped(#[from] enet_proto::EnetEncoderError),

  #[error("Failed to encode eNet request")]
  Io(#[from] io::Error),
}

#[non_exhaustive]
#[derive(Debug, Error)]
pub enum EnetDecoderError {
  #[error(transparent)]
  Wrapped(#[from] enet_proto::EnetDecoderError),

  #[error("Failed to decode eNet response")]
  Io(#[from] io::Error),
}
