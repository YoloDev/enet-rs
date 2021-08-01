use enet_proto::{ItemUpdateValue, ProjectItem};
use eventuals::{Eventual, EventualReader, EventualWriter};
use std::{cmp::Ordering, convert::TryFrom, fmt, num::NonZeroU8, str::FromStr, sync::Arc};
use thiserror::Error;

pub(crate) struct DeviceDesc {
  pub name: String,
  pub number: u32,
  pub kind: DeviceKind,
}

impl TryFrom<ProjectItem> for DeviceDesc {
  type Error = ProjectItem;

  fn try_from(value: ProjectItem) -> Result<Self, Self::Error> {
    match value {
      ProjectItem::Binaer(v) if v.programmable => Ok(DeviceDesc {
        name: v.name,
        number: v.number,
        kind: DeviceKind::Binary,
      }),

      ProjectItem::Dimmer(v) => Ok(DeviceDesc {
        name: v.name,
        number: v.number,
        kind: DeviceKind::Dimmer,
      }),

      ProjectItem::Jalousie(v) => Ok(DeviceDesc {
        name: v.name,
        number: v.number,
        kind: DeviceKind::Blinds,
      }),

      _ => Err(value),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceKind {
  Binary,
  Dimmer,
  Blinds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceValue {
  Undefined,
  Off,
  On(OnValue),
  AllOff,
  AllOn,
}

impl DeviceValue {
  pub fn is_on(&self) -> bool {
    matches!(self, DeviceValue::On(_) | DeviceValue::AllOn)
  }

  pub fn value(&self) -> Option<OnValue> {
    match self {
      DeviceValue::On(v) => Some(*v),
      DeviceValue::AllOn => Some(OnValue::new(100).unwrap()),
      _ => None,
    }
  }
}

impl fmt::Display for DeviceValue {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      DeviceValue::Undefined => f.write_str("undefined"),
      DeviceValue::Off => f.write_str("off"),
      DeviceValue::On(v) => fmt::Display::fmt(v, f),
      DeviceValue::AllOff => f.write_str("all off"),
      DeviceValue::AllOn => f.write_str("all on"),
    }
  }
}

impl Default for DeviceValue {
  #[inline]
  fn default() -> Self {
    Self::Undefined
  }
}

impl TryFrom<ItemUpdateValue> for DeviceValue {
  type Error = ItemUpdateValue;

  fn try_from(value: ItemUpdateValue) -> Result<Self, Self::Error> {
    match &*value.state {
      "UNDEFINED" => Ok(DeviceValue::Undefined),
      "OFF" => Ok(DeviceValue::Off),
      "ON" => match OnValue::from_str(&value.value) {
        Ok(v) => Ok(DeviceValue::On(v)),
        Err(_) => Err(value),
      },
      "ALL_OFF" => Ok(DeviceValue::AllOff),
      "ALL_ON" => Ok(DeviceValue::AllOn),
      _ => Err(value),
    }
  }
}

impl PartialOrd for DeviceValue {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    match (self, other) {
      (DeviceValue::Undefined, DeviceValue::Undefined) => Some(Ordering::Equal),
      (DeviceValue::Undefined, _) => None,
      (_, DeviceValue::Undefined) => None,
      (DeviceValue::Off, DeviceValue::Off) => Some(Ordering::Equal),
      (DeviceValue::Off, DeviceValue::On(_)) => Some(Ordering::Less),
      (DeviceValue::Off, DeviceValue::AllOff) => None,
      (DeviceValue::Off, DeviceValue::AllOn) => None,
      (DeviceValue::On(_), DeviceValue::Off) => Some(Ordering::Greater),
      (DeviceValue::On(lhs), DeviceValue::On(rhs)) => Some(lhs.cmp(rhs)),
      (DeviceValue::On(_), DeviceValue::AllOff) => None,
      (DeviceValue::On(_), DeviceValue::AllOn) => None,
      (DeviceValue::AllOff, DeviceValue::Off) => None,
      (DeviceValue::AllOff, DeviceValue::On(_)) => None,
      (DeviceValue::AllOff, DeviceValue::AllOff) => Some(Ordering::Equal),
      (DeviceValue::AllOff, DeviceValue::AllOn) => Some(Ordering::Less),
      (DeviceValue::AllOn, DeviceValue::Off) => None,
      (DeviceValue::AllOn, DeviceValue::On(_)) => None,
      (DeviceValue::AllOn, DeviceValue::AllOff) => Some(Ordering::Greater),
      (DeviceValue::AllOn, DeviceValue::AllOn) => Some(Ordering::Equal),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OnValue(NonZeroU8);

impl fmt::Display for OnValue {
  #[inline]
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Display::fmt(&self.0, f)
  }
}

impl OnValue {
  pub fn new(value: u8) -> Option<OnValue> {
    match NonZeroU8::new(value) {
      None => None,
      Some(_) if value > 100 => None,
      Some(v) => Some(OnValue(v)),
    }
  }

  #[inline]
  pub fn get(&self) -> u8 {
    self.0.get()
  }
}

impl FromStr for OnValue {
  type Err = ParseOnValueError;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    if s.len() > 3 || s.is_empty() {
      return Err(ParseOnValueError);
    }

    let mut v = 0u8;
    for b in s.bytes() {
      if !(b'0'..=b'9').contains(&b) {
        return Err(ParseOnValueError);
      }

      v *= 10;
      v += b - b'0';
    }

    match OnValue::new(v) {
      None => Err(ParseOnValueError),
      Some(v) => Ok(v),
    }
  }
}

pub(crate) struct DeviceWriter {
  pub(crate) index: u32,
  pub(crate) desc: Arc<DeviceDesc>,
  pub(crate) writer: EventualWriter<DeviceValue>,
}

#[derive(Clone)]
pub struct Device {
  pub(crate) desc: Arc<DeviceDesc>,
  pub(crate) value: Eventual<DeviceValue>,
}

impl Device {
  pub(crate) fn new(desc: DeviceDesc, index: u32) -> (DeviceWriter, Self) {
    let desc = Arc::new(desc);
    let (writer, value) = Eventual::new();

    let writer = DeviceWriter {
      index,
      desc: desc.clone(),
      writer,
    };
    let device = Device { desc, value };

    (writer, device)
  }

  pub fn name(&self) -> &str {
    &self.desc.name
  }

  pub fn number(&self) -> u32 {
    self.desc.number
  }

  pub fn kind(&self) -> DeviceKind {
    self.desc.kind
  }

  pub fn subscribe(&self) -> EventualReader<DeviceValue> {
    self.value.subscribe()
  }
}

#[derive(Debug, Error)]
#[non_exhaustive]
#[error("Failed to parse 'on' value. Must be 1..=100")]
pub struct ParseOnValueError;
