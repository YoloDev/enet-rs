use bytes::{BufMut, BytesMut};
use thiserror::Error;

use crate::RequestEnvelope;

use super::DELIMETER;

#[derive(Default)]
pub struct EnetEncoder;

impl EnetEncoder {
  #[inline]
  pub const fn new() -> Self {
    Self
  }

  pub fn encode(
    &mut self,
    item: &RequestEnvelope,
    buf: &mut BytesMut,
  ) -> Result<(), EnetEncoderError> {
    serde_json::to_writer(buf.writer(), item)?;
    buf.put_slice(DELIMETER.as_bytes());

    Ok(())
  }
}

#[non_exhaustive]
#[derive(Debug, Error)]
pub enum EnetEncoderError {
  #[error("Failed to encode eNet message.")]
  JsonError(#[from] serde_json::Error),
}
