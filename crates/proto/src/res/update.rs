use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct ItemUpdateValue {
  #[serde(deserialize_with = "serde_aux::field_attributes::deserialize_number_from_string")]
  pub number: u32,
  pub value: String,
  pub state: String,
  pub setpoint: String,
}
