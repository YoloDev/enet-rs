use enet_proto::{ItemUpdateValue, ProjectItem, SetValue};
use eventuals::{Eventual, EventualReader, EventualWriter};
use std::{cmp::Ordering, convert::TryFrom, fmt, future::ready, str::FromStr, sync::Arc};
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

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceKind {
  Binary,
  Dimmer,
  Blinds,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceValue {
  Undefined,
  Off,
  On(DeviceBrightness),
  AllOff,
  AllOn,
}

impl From<DeviceState> for DeviceValue {
  fn from(state: DeviceState) -> Self {
    match state {
      DeviceState::Off => DeviceValue::Off,
      DeviceState::On => DeviceValue::On(DeviceBrightness::MAX),
      DeviceState::Unknown => DeviceValue::Undefined,
    }
  }
}

impl From<(DeviceState, DeviceBrightness)> for DeviceValue {
  fn from((state, brightness): (DeviceState, DeviceBrightness)) -> Self {
    match state {
      DeviceState::Off => DeviceValue::Off,
      DeviceState::On => DeviceValue::On(brightness),
      DeviceState::Unknown => DeviceValue::Undefined,
    }
  }
}

impl DeviceValue {
  pub fn is_on(&self) -> bool {
    matches!(self, DeviceValue::On(_) | DeviceValue::AllOn)
  }

  pub fn brightness(&self) -> Option<DeviceBrightness> {
    match self {
      DeviceValue::On(v) => Some(*v),
      DeviceValue::AllOn => Some(DeviceBrightness::MAX),
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
      "ON" => match DeviceBrightness::from_str(&value.value) {
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

pub(crate) struct BinaryDeviceWriter {
  pub(crate) index: u32,
  pub(crate) desc: Arc<DeviceDesc>,
  pub(crate) state_writer: EventualWriter<DeviceState>,
}

impl BinaryDeviceWriter {
  fn desc(&self) -> &DeviceDesc {
    &*self.desc
  }

  pub(crate) fn kind(&self) -> DeviceKind {
    self.desc().kind
  }

  pub(crate) fn name(&self) -> &str {
    &*self.desc().name
  }
}

pub(crate) struct DimmerDeviceWriter {
  pub(crate) index: u32,
  pub(crate) desc: Arc<DeviceDesc>,
  pub(crate) state_writer: EventualWriter<DeviceState>,
  pub(crate) brightness_writer: EventualWriter<DeviceBrightness>,
}

impl DimmerDeviceWriter {
  fn desc(&self) -> &DeviceDesc {
    &*self.desc
  }

  pub(crate) fn kind(&self) -> DeviceKind {
    self.desc().kind
  }

  pub(crate) fn name(&self) -> &str {
    &*self.desc().name
  }
}

pub(crate) enum DeviceWriter {
  Binary(BinaryDeviceWriter),
  Dimmer(DimmerDeviceWriter),
}

impl DeviceWriter {
  fn new_binary(
    desc: Arc<DeviceDesc>,
    index: u32,
    state_writer: EventualWriter<DeviceState>,
  ) -> Self {
    DeviceWriter::Binary(BinaryDeviceWriter {
      index,
      desc,
      state_writer,
    })
  }

  fn new_dimmer(
    desc: Arc<DeviceDesc>,
    index: u32,
    state_writer: EventualWriter<DeviceState>,
    brightness_writer: EventualWriter<DeviceBrightness>,
  ) -> Self {
    DeviceWriter::Dimmer(DimmerDeviceWriter {
      index,
      desc,
      state_writer,
      brightness_writer,
    })
  }

  pub(crate) fn index(&self) -> u32 {
    match self {
      DeviceWriter::Binary(w) => w.index,
      DeviceWriter::Dimmer(w) => w.index,
    }
  }

  fn desc(&self) -> &DeviceDesc {
    match self {
      DeviceWriter::Binary(w) => &*w.desc,
      DeviceWriter::Dimmer(w) => &*w.desc,
    }
  }

  pub(crate) fn kind(&self) -> DeviceKind {
    self.desc().kind
  }

  pub(crate) fn name(&self) -> &str {
    &*self.desc().name
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceState {
  Off,
  On,
  Unknown,
}

impl From<SetValue> for DeviceState {
  fn from(v: SetValue) -> Self {
    match v {
      SetValue::On(_) => Self::On,
      SetValue::Off(_) => Self::Off,
      SetValue::Dimm(0) => Self::Off,
      SetValue::Dimm(_) => Self::On,
      SetValue::Blinds(_) => todo!(),
    }
  }
}

impl fmt::Display for DeviceState {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      DeviceState::Off => f.write_str("OFF"),
      DeviceState::On => f.write_str("ON"),
      DeviceState::Unknown => f.write_str("UNKNOWN"),
    }
  }
}

impl FromStr for DeviceState {
  type Err = ParseDeviceStateError;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "ON" => Ok(Self::On),
      "OFF" => Ok(Self::Off),
      "UNKNOWN" | "UNDEFINED" => Ok(Self::Unknown),
      _ => Err(ParseDeviceStateError),
    }
  }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceBrightness(u8);

impl DeviceBrightness {
  pub const MIN: DeviceBrightness = DeviceBrightness(0);
  pub const MAX: DeviceBrightness = DeviceBrightness(100);

  pub const fn new(value: u8) -> Option<Self> {
    if value <= 100 {
      Some(Self(value))
    } else {
      None
    }
  }

  #[inline]
  pub const fn get(self) -> u8 {
    self.0
  }
}

impl fmt::Debug for DeviceBrightness {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Debug::fmt(&self.0, f)
  }
}

impl fmt::Display for DeviceBrightness {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Display::fmt(&self.0, f)
  }
}

impl FromStr for DeviceBrightness {
  type Err = ParseDeviceBrightnessError;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    if s.len() > 3 || s.is_empty() {
      return Err(ParseDeviceBrightnessError);
    }

    let mut v = 0u8;
    for b in s.bytes() {
      if !(b'0'..=b'9').contains(&b) {
        return Err(ParseDeviceBrightnessError);
      }

      v *= 10;
      v += b - b'0';
    }

    match DeviceBrightness::new(v) {
      None => Err(ParseDeviceBrightnessError),
      Some(v) => Ok(v),
    }
  }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceGroupState {
  AllOff,
  AllOn,
}

pub trait EnetDevice {
  fn name(&self) -> &str;
  fn number(&self) -> u32;
  fn kind(&self) -> DeviceKind;
  fn subscribe(&self) -> EventualReader<DeviceValue>;
}

#[derive(Clone)]
pub struct BinaryDevice {
  pub(crate) desc: Arc<DeviceDesc>,
  pub(crate) state: Eventual<DeviceState>,
}

impl BinaryDevice {
  fn new(desc: Arc<DeviceDesc>, state: Eventual<DeviceState>) -> Self {
    Self { desc, state }
  }

  pub fn subscribe_state(&self) -> EventualReader<DeviceState> {
    self.state.subscribe()
  }
}

impl EnetDevice for BinaryDevice {
  fn name(&self) -> &str {
    &*self.desc.name
  }

  fn number(&self) -> u32 {
    self.desc.number
  }

  fn kind(&self) -> DeviceKind {
    DeviceKind::Binary
  }

  fn subscribe(&self) -> EventualReader<DeviceValue> {
    eventuals::map(&self.state, |v| ready(v.into())).subscribe()
  }
}

#[derive(Clone)]
pub struct DimmerDevice {
  pub(crate) desc: Arc<DeviceDesc>,
  pub(crate) state: Eventual<DeviceState>,
  pub(crate) brightness: Eventual<DeviceBrightness>,
}

impl DimmerDevice {
  fn new(
    desc: Arc<DeviceDesc>,
    state: Eventual<DeviceState>,
    brightness: Eventual<DeviceBrightness>,
  ) -> Self {
    Self {
      desc,
      state,
      brightness,
    }
  }

  pub fn subscribe_state(&self) -> EventualReader<DeviceState> {
    self.state.subscribe()
  }

  pub fn subscribe_brightness(&self) -> EventualReader<DeviceBrightness> {
    self.brightness.subscribe()
  }
}

impl EnetDevice for DimmerDevice {
  fn name(&self) -> &str {
    &*self.desc.name
  }

  fn number(&self) -> u32 {
    self.desc.number
  }

  fn kind(&self) -> DeviceKind {
    DeviceKind::Dimmer
  }

  fn subscribe(&self) -> EventualReader<DeviceValue> {
    let joined = eventuals::join((&self.state, &self.brightness));
    let mapped = eventuals::map(joined, |v| ready(v.into()));
    mapped.subscribe()
  }
}

#[derive(Clone)]
pub enum Device {
  Binary(BinaryDevice),
  Dimmer(DimmerDevice),
}

impl EnetDevice for Device {
  fn name(&self) -> &str {
    match self {
      Device::Binary(d) => d.name(),
      Device::Dimmer(d) => d.name(),
    }
  }

  fn number(&self) -> u32 {
    match self {
      Device::Binary(d) => d.number(),
      Device::Dimmer(d) => d.number(),
    }
  }

  fn kind(&self) -> DeviceKind {
    match self {
      Device::Binary(d) => d.kind(),
      Device::Dimmer(d) => d.kind(),
    }
  }

  fn subscribe(&self) -> EventualReader<DeviceValue> {
    match self {
      Device::Binary(d) => d.subscribe(),
      Device::Dimmer(d) => d.subscribe(),
    }
  }
}

impl Device {
  pub(crate) fn new(desc: DeviceDesc, index: u32) -> (DeviceWriter, Self) {
    let desc = Arc::new(desc);
    match desc.kind {
      DeviceKind::Binary => Self::new_binary(desc, index),
      DeviceKind::Dimmer => Self::new_dimmer(desc, index),
      DeviceKind::Blinds => todo!(),
    }
  }

  fn new_binary(desc: Arc<DeviceDesc>, index: u32) -> (DeviceWriter, Self) {
    debug_assert_eq!(desc.kind, DeviceKind::Binary);

    let (state_writer, state) = Eventual::new();

    (
      DeviceWriter::new_binary(desc.clone(), index, state_writer),
      Self::Binary(BinaryDevice::new(desc, state)),
    )
  }

  fn new_dimmer(desc: Arc<DeviceDesc>, index: u32) -> (DeviceWriter, Self) {
    debug_assert_eq!(desc.kind, DeviceKind::Dimmer);

    let (state_writer, state) = Eventual::new();
    let (brightness_writer, brightness) = Eventual::new();

    (
      DeviceWriter::new_dimmer(desc.clone(), index, state_writer, brightness_writer),
      Self::Dimmer(DimmerDevice::new(desc, state, brightness)),
    )
  }
}

#[derive(Debug, Error)]
#[non_exhaustive]
#[error("Failed to parse 'on' value. Must be 0..=100.")]
pub struct ParseDeviceBrightnessError;

#[derive(Debug, Error)]
#[non_exhaustive]
#[error("Failed to parse state. Must be either 'ON' or 'OFF'.")]
pub struct ParseDeviceStateError;
