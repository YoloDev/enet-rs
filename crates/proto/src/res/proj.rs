use derive_more::IsVariant;
use enum_kinds::EnumKind;
use serde::{
  de::{Unexpected, Visitor},
  Deserialize, Deserializer,
};
use std::fmt;

pub trait EnetItem {
  fn number(&self) -> u32;
  fn name(&self) -> &str;
  fn is_subscribable(&self) -> bool;
}

macro_rules! impl_item {
  ($ty:ty : $id:ident => $sub:expr) => {
    impl EnetItem for $ty {
      #[inline]
      fn number(&self) -> u32 {
        self.number
      }

      #[inline]
      fn name(&self) -> &str {
        &self.name
      }

      #[inline]
      fn is_subscribable(&self) -> bool {
        let $id = self;
        $sub
      }
    }
  };

  ($ty:ty => $sub:literal) => {
    impl_item!($ty : _v => $sub);
  }
}

// TODO: https://github.com/serde-rs/serde/pull/1902
#[derive(Debug, Deserialize, EnumKind, IsVariant)]
#[serde(tag = "TYPE", rename_all = "UPPERCASE")]
#[enum_kind(ProjectItemKind)]
pub enum ProjectItem {
  #[serde(alias = "Scene")]
  Scene(ProjectScene),
  Binaer(ProjectBinaer),
  Dimmer(ProjectDimmer),
  Jalousie(ProjectJalousie),
  None(ProjectNone),
}

impl ProjectItem {
  #[inline]
  pub fn kind(&self) -> ProjectItemKind {
    ProjectItemKind::from(self)
  }
}

impl EnetItem for ProjectItem {
  fn number(&self) -> u32 {
    match self {
      ProjectItem::Scene(v) => v.number(),
      ProjectItem::Binaer(v) => v.number(),
      ProjectItem::Dimmer(v) => v.number(),
      ProjectItem::Jalousie(v) => v.number(),
      ProjectItem::None(v) => v.number(),
    }
  }

  fn name(&self) -> &str {
    match self {
      ProjectItem::Scene(v) => v.name(),
      ProjectItem::Binaer(v) => v.name(),
      ProjectItem::Dimmer(v) => v.name(),
      ProjectItem::Jalousie(v) => v.name(),
      ProjectItem::None(v) => v.name(),
    }
  }

  fn is_subscribable(&self) -> bool {
    match self {
      ProjectItem::Scene(v) => v.is_subscribable(),
      ProjectItem::Binaer(v) => v.is_subscribable(),
      ProjectItem::Dimmer(v) => v.is_subscribable(),
      ProjectItem::Jalousie(v) => v.is_subscribable(),
      ProjectItem::None(v) => v.is_subscribable(),
    }
  }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct ProjectScene {
  pub number: u32,
  pub name: String,
  pub dimmable: bool,
}
impl_item!(ProjectScene => false);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct ProjectBinaer {
  pub number: u32,
  pub name: String,
  #[serde(default = "get_true", deserialize_with = "deserialize_programmable")]
  pub programmable: bool,
}
impl_item!(ProjectBinaer : v => v.programmable);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct ProjectDimmer {
  pub number: u32,
  pub name: String,
}
impl_item!(ProjectDimmer => true);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct ProjectJalousie {
  pub number: u32,
  pub name: String,
}
impl_item!(ProjectJalousie => true);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct ProjectNone {
  pub number: u32,
  pub name: String,
}
impl_item!(ProjectNone => false);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct ProjectList {
  pub number: u32,
  pub name: String,
  #[serde(default)]
  pub items_order: Vec<u32>,
  pub visible: bool,
}

struct ProgrammableVisitor;

impl<'de> Visitor<'de> for ProgrammableVisitor {
  type Value = bool;

  fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
    formatter.write_str("bool")
  }

  #[inline]
  fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
  where
    E: serde::de::Error,
  {
    Ok(v)
  }

  #[inline]
  fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
  where
    E: serde::de::Error,
  {
    match v {
      "false" | "FALSE" => Ok(false),
      "true" | "TRUE" => Ok(true),
      _ => Err(E::invalid_value(Unexpected::Str(v), &self)),
    }
  }
}

fn deserialize_programmable<'de, D>(serializer: D) -> Result<bool, D::Error>
where
  D: Deserializer<'de>,
{
  serializer.deserialize_any(ProgrammableVisitor)
}

#[inline]
fn get_true() -> bool {
  true
}
