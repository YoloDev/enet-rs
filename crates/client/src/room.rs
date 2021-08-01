use enet_proto::ProjectList;

#[allow(dead_code)]
pub(crate) struct RoomDesc {
  pub number: u32,
  pub name: String,
  pub items: Vec<u32>,
}

impl From<ProjectList> for RoomDesc {
  fn from(v: ProjectList) -> Self {
    Self {
      number: v.number,
      name: v.name,
      items: v.items_order,
    }
  }
}
