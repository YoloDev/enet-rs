use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolVersion {
  ZeroZeroThree,

  Unknown(SmolStr),
}

impl fmt::Display for ProtocolVersion {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::ZeroZeroThree => f.write_str("0.03"),
      Self::Unknown(v) => f.write_str(v.as_str()),
    }
  }
}

impl Serialize for ProtocolVersion {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    match self {
      Self::ZeroZeroThree => "0.03".serialize(serializer),
      Self::Unknown(v) => v.serialize(serializer),
    }
  }
}

struct ProtocolVisitor;

impl<'de> serde::de::Visitor<'de> for ProtocolVisitor {
  type Value = ProtocolVersion;

  fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
    formatter.write_str("protocol version")
  }

  fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
  where
    E: serde::de::Error,
  {
    match v {
      "0.03" => Ok(ProtocolVersion::ZeroZeroThree),
      _ => Ok(ProtocolVersion::Unknown(v.into())),
    }
  }
}

impl<'de> Deserialize<'de> for ProtocolVersion {
  #[inline]
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    deserializer.deserialize_str(ProtocolVisitor)
  }
}
