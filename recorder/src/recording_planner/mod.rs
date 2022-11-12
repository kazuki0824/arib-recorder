use serde_derive::{Deserialize, Serialize};
use ulid::Ulid;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) enum PlanId {
    Word(Ulid),
    Series(Ulid),
    None,
}
