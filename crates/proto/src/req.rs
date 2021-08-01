mod value;

use crate::ProtocolVersion;
use derive_more::{Constructor, From, IsVariant, TryInto};
use enum_kinds::EnumKind;
use serde::Serialize;
use std::{convert::TryFrom, time::SystemTime};
use value::ItemSetValue;

mod sealed {
  pub trait Sealed {}
}

pub trait RequestType: Into<Request> + TryFrom<Request> + sealed::Sealed {
  fn protocol_version(&self) -> ProtocolVersion;
}

macro_rules! impl_request_type {
  ($t:ty => $e:expr) => {
    impl sealed::Sealed for $t {}
    impl RequestType for $t {
      #[inline]
      fn protocol_version(&self) -> ProtocolVersion {
        $e
      }
    }
  };
}

#[derive(Serialize, Debug, Constructor)]
#[serde(rename_all = "UPPERCASE")]
pub struct VersionReq;
impl_request_type!(VersionReq => ProtocolVersion::ZeroZeroThree);

#[derive(Serialize, Debug, Constructor)]
#[serde(rename_all = "UPPERCASE")]
pub struct GetChannelInfoAllReq;
impl_request_type!(GetChannelInfoAllReq => ProtocolVersion::ZeroZeroThree);

#[derive(Serialize, Debug, Constructor)]
#[serde(rename_all = "UPPERCASE")]
pub struct ItemValueSignInReq {
  pub items: Vec<u32>,
}
impl_request_type!(ItemValueSignInReq => ProtocolVersion::ZeroZeroThree);

#[derive(Serialize, Debug, Constructor)]
#[serde(rename_all = "UPPERCASE")]
pub struct ItemValueSignOutReq {
  pub items: Vec<u32>,
}
impl_request_type!(ItemValueSignOutReq => ProtocolVersion::ZeroZeroThree);

#[derive(Serialize, Debug, Constructor)]
#[serde(rename_all = "UPPERCASE")]
pub struct BlockListReq {
  #[serde(rename = "LIST-RANGE")]
  pub list_range: u32,
}
impl_request_type!(BlockListReq => ProtocolVersion::ZeroZeroThree);

#[derive(Serialize, Debug, Constructor)]
#[serde(rename_all = "UPPERCASE")]
pub struct ProjectListReq;
impl_request_type!(ProjectListReq => ProtocolVersion::ZeroZeroThree);

#[derive(Serialize, Debug, Constructor)]
#[serde(rename_all = "UPPERCASE")]
pub struct ItemValueSetReq {
  pub values: Vec<ItemSetValue>,
}
impl_request_type!(ItemValueSetReq => ProtocolVersion::ZeroZeroThree);

#[derive(Debug, Serialize, From, IsVariant, TryInto, EnumKind)]
#[enum_kind(RequestKind)]
#[serde(tag = "CMD")]
pub enum Request {
  #[serde(rename = "VERSION_REQ")]
  Version(VersionReq),

  #[serde(rename = "GET_CHANNEL_INFO_ALL_REQ")]
  GetChannelInfoAll(GetChannelInfoAllReq),

  #[serde(rename = "ITEM_VALUE_SIGN_IN_REQ")]
  ItemValueSignIn(ItemValueSignInReq),

  #[serde(rename = "ITEM_VALUE_SIGN_OUT_REQ")]
  ItemValueSignOut(ItemValueSignOutReq),

  #[serde(rename = "BLOCK_LIST_REQ")]
  BlockList(BlockListReq),

  #[serde(rename = "PROJECT_LIST_GET")]
  ProjectList(ProjectListReq),

  #[serde(rename = "ITEM_VALUE_SET")]
  ItemValueSet(ItemValueSetReq),
}

impl Request {
  #[inline]
  pub fn kind(&self) -> RequestKind {
    RequestKind::from(self)
  }
}

impl sealed::Sealed for Request {}
impl RequestType for Request {
  fn protocol_version(&self) -> ProtocolVersion {
    match self {
      Request::Version(v) => v.protocol_version(),
      Request::GetChannelInfoAll(v) => v.protocol_version(),
      Request::ItemValueSignIn(v) => v.protocol_version(),
      Request::ItemValueSignOut(v) => v.protocol_version(),
      Request::BlockList(v) => v.protocol_version(),
      Request::ProjectList(v) => v.protocol_version(),
      Request::ItemValueSet(v) => v.protocol_version(),
    }
  }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct RequestEnvelope {
  #[serde(flatten)]
  pub body: Request,
  pub protocol: ProtocolVersion,
  #[serde(serialize_with = "serialize_enet_timestamp")]
  pub timestamp: SystemTime,
}

impl RequestEnvelope {
  pub fn new(request: impl RequestType) -> Self {
    let protocol = request.protocol_version();
    Self::_new(request.into(), protocol)
  }

  #[inline(never)]
  fn _new(request: Request, protocol: ProtocolVersion) -> Self {
    Self {
      body: request,
      protocol,
      timestamp: SystemTime::now(),
    }
  }
}

fn serialize_enet_timestamp<S>(value: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
where
  S: serde::Serializer,
{
  let s = value
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_secs()
    .to_string();

  s.serialize(serializer)
}
