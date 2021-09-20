mod de;
mod proj;
mod update;

use crate::ProtocolVersion;
use derive_more::{From, IsVariant};
use enum_kinds::EnumKind;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::convert::TryFrom;
use tracing::{event, Level};

pub use proj::*;
pub use update::ItemUpdateValue;

const FIELD_NAME_PROTOCOL: &str = "PROTOCOL";
const FIELD_NAME_KIND: &str = "CMD";

mod sealed {
  pub trait Sealed {}
}

pub trait ResponseType: Into<Response> + TryFrom<Response> + sealed::Sealed {
  fn protocol_version(&self) -> ProtocolVersion;
}

macro_rules! impl_response_type {
  ($t:ty => $e:expr) => {
    impl sealed::Sealed for $t {}
    impl ResponseType for $t {
      #[inline]
      fn protocol_version(&self) -> ProtocolVersion {
        $e
      }
    }

    impl $t {
      fn deserialize_response<'de, D>(deserializer: D) -> Result<Response, D::Error>
      where
        D: Deserializer<'de>
      {
        <Self as Deserialize<'de>>::deserialize(deserializer).map(Into::into)
      }
    }
  };
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct VersionRes {
  pub firmware: String,
  pub hardware: String,
  pub enet: String,
}
impl_response_type!(VersionRes => ProtocolVersion::ZeroZeroThree);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct GetChannelInfoAllRes {
  pub devices: Vec<u32>,
}
impl_response_type!(GetChannelInfoAllRes => ProtocolVersion::ZeroZeroThree);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct ItemValueRes {}
impl_response_type!(ItemValueRes => ProtocolVersion::ZeroZeroThree);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct ItemValueSignInRes {}
impl_response_type!(ItemValueSignInRes => ProtocolVersion::ZeroZeroThree);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct ProjectListRes {
  pub project_id: String,
  pub items: Vec<proj::ProjectItem>,
  pub lists: Vec<proj::ProjectList>,
}
impl_response_type!(ProjectListRes => ProtocolVersion::ZeroZeroThree);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct ItemUpdateInd {
  pub values: Vec<ItemUpdateValue>,
}
impl_response_type!(ItemUpdateInd => ProtocolVersion::ZeroZeroThree);

#[derive(Debug)]
pub struct UnknownRes {
  pub kind: String,
  pub protocol: String,
  pub values: Value,
}

#[derive(Debug, From, IsVariant, EnumKind)]
#[enum_kind(ResponseKind)]
pub enum Response {
  Version(VersionRes),

  GetChannelInfoAll(GetChannelInfoAllRes),

  ItemValueSignIn(ItemValueSignInRes),

  ItemValue(ItemValueRes),

  ProjectList(ProjectListRes),

  ItemUpdate(ItemUpdateInd),

  Unknown(UnknownRes),
}

macro_rules! try_into {
  ($variant:ident => $ty:ty) => {
    impl TryFrom<Response> for $ty {
      type Error = Response;

      #[inline]
      fn try_from(value: Response) -> Result<Self, Self::Error> {
        match value {
          Response::$variant(v) => Ok(v),
          _ => Err(value),
        }
      }
    }
  };
}

try_into!(Version => VersionRes);
try_into!(GetChannelInfoAll => GetChannelInfoAllRes);
try_into!(ProjectList => ProjectListRes);
try_into!(ItemUpdate => ItemUpdateInd);
try_into!(ItemValue => ItemValueRes);
try_into!(ItemValueSignIn => ItemValueSignInRes);

impl Response {
  #[inline]
  pub fn kind(&self) -> ResponseKind {
    ResponseKind::from(self)
  }
}

macro_rules! match_response {
  ($kind:ident, $protocol:ident, $deserializer:ident => {
    $(($k:ident, $v:ident) => $t:ty),*$(,)?
  }) => {
    match (&$kind, &$protocol) {
      $(
        (de::ResponseKind::$k, ProtocolVersion::$v) => <$t>::deserialize_response($deserializer),
      )+
      (de::ResponseKind::Unknown(_), _) | (_, ProtocolVersion::Unknown(_)) => {
        reconstruct($kind, $protocol, $deserializer)
      }
    }
  };
}

impl Response {
  fn deserialize<'de, D>(
    kind: de::ResponseKind,
    protocol: ProtocolVersion,
    deserializer: D,
  ) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    match_response!(kind, protocol, deserializer => {
      (Version, ZeroZeroThree) => VersionRes,
      (GetChannelInfoAll, ZeroZeroThree) => GetChannelInfoAllRes,
      (ItemValue, ZeroZeroThree) => ItemValueRes,
      (ItemValueSignIn, ZeroZeroThree) => ItemValueSignInRes,
      (ProjectList, ZeroZeroThree) => ProjectListRes,
      (ItemUpdate, ZeroZeroThree) => ItemUpdateInd,
    })
  }
}

fn reconstruct<'de, D>(
  kind: de::ResponseKind,
  protocol: ProtocolVersion,
  deserializer: D,
) -> Result<Response, D::Error>
where
  D: Deserializer<'de>,
{
  let json = serde_json::Value::deserialize(deserializer)?;

  event!(target: "enet-proto::res", Level::WARN, %kind, %protocol, %json, "received unknown response type");
  Ok(Response::Unknown(UnknownRes {
    kind: kind.to_string(),
    protocol: protocol.to_string(),
    values: json,
  }))
}
