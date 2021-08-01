use serde::{ser::SerializeStruct, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct ItemSetValue {
  #[serde(flatten)]
  pub value: SetValue,

  pub number: u32,
}

// TODO: This should use the same kind of structure as DeviceValue
#[derive(Debug, Clone, Copy)]
pub enum SetValue {
  On(ClickDuration),

  Off(ClickDuration),

  Dimm(u8),

  Blinds(u8),
}

impl Serialize for SetValue {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    fn serialize_state<S>(state: &str, serializer: S) -> Result<S::Ok, S::Error>
    where
      S: serde::Serializer,
    {
      let mut s = serializer.serialize_struct("SetValue", 1)?;
      s.serialize_field("STATE", state)?;
      s.end()
    }

    fn serialize_state_long<S>(state: &str, serializer: S) -> Result<S::Ok, S::Error>
    where
      S: serde::Serializer,
    {
      let mut s = serializer.serialize_struct("SetValue", 2)?;
      s.serialize_field("STATE", state)?;
      s.serialize_field("LONG_CLICK", "ON")?;
      s.end()
    }

    fn serialize_value<S>(state: &str, value: &u8, serializer: S) -> Result<S::Ok, S::Error>
    where
      S: serde::Serializer,
    {
      let mut s = serializer.serialize_struct("SetValue", 2)?;
      s.serialize_field("STATE", state)?;
      s.serialize_field("VALUE", value)?;
      s.end()
    }

    match self {
      SetValue::On(ClickDuration::Short) => serialize_state("ON", serializer),
      SetValue::On(ClickDuration::Long) => serialize_state_long("ON", serializer),
      SetValue::Off(ClickDuration::Short) => serialize_state("OFF", serializer),
      SetValue::Off(ClickDuration::Long) => serialize_state_long("OFF", serializer),
      SetValue::Dimm(v) => serialize_value("VALUE_DIMM", v, serializer),
      SetValue::Blinds(v) => serialize_value("VALUE_BLINDS", v, serializer),
    }
  }
}

#[derive(Debug, Clone, Copy)]
pub enum ClickDuration {
  Short,
  Long,
}
