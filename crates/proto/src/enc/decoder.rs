use super::DELIMETER;
use crate::Response;
use bytes::BytesMut;
use lazy_static::lazy_static;
use regex::bytes::{Regex, RegexBuilder};
use thiserror::Error;
use tracing::{event, Level};

lazy_static! {
  static ref DELIMETER_REGEX: Regex = RegexBuilder::new(DELIMETER).unicode(false).build().unwrap();
}

#[derive(Default)]
pub struct EnetDecoder {
  // Stored index of the next index to examine for the delimiter character.
  // This is used to optimize searching.
  // For example, if `decode` was called with `abc`, it would hold `3`,
  // because that is the next index to examine.
  // The next time `decode` is called with `abcde}`, the method will
  // only look at `de}` before returning.
  next_index: usize,
}

impl EnetDecoder {
  #[inline]
  pub const fn new() -> Self {
    Self { next_index: 0 }
  }

  pub fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Response>, EnetDecoderError> {
    match DELIMETER_REGEX.find_at(buf, self.next_index) {
      None => {
        // no match was found
        let mut end_chars_matching = 0usize;
        let mut end_iter = buf.iter().rev();
        let mut pat_iter = DELIMETER.as_bytes().iter().rev();
        while end_iter.next() == pat_iter.next() && end_chars_matching < DELIMETER.len() - 1 {
          end_chars_matching += 1;
        }

        self.next_index = buf.len() - end_chars_matching;
        Ok(None)
      }

      Some(m) => {
        let range = m.range();
        self.next_index = 0;
        let chunk_with_delimeter = buf.split_to(range.end);
        let chunk = &chunk_with_delimeter[..chunk_with_delimeter.len() - DELIMETER.len()];
        let item = parse(chunk)?;
        Ok(Some(item))
      }
    }
  }
}

fn parse(buf: &[u8]) -> Result<Response, EnetDecoderError> {
  if let Ok(utf8) = std::str::from_utf8(buf) {
    event!(target: "enet-proto::enc::decoder", Level::TRACE, "parsing enet data: {}", utf8);
  }

  // Due to some of the enet messages having duplicate keys, we "sanitize" the input by deserializing to serde_json::Value first
  // let value: serde_json::Value = serde_json::from_slice(buf)?;
  let response = serde_json::from_slice(buf)?;
  Ok(response)
}

#[non_exhaustive]
#[derive(Debug, Error)]
pub enum EnetDecoderError {
  #[error("Failed to decode eNet message.")]
  JsonError(#[from] serde_json::Error),
}
