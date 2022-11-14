use log::{info, warn};
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Error;
use std::path::PathBuf;
use ulid::Ulid;

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub(crate) enum PlanUnit {
    Word(String),
    Series(String),
}

pub(crate) struct RulesResolverBase {
    inner: HashMap<Ulid, PlanUnit>,
    save_file_location: PathBuf,
}

impl RulesResolverBase {
    pub fn new(location: Option<PathBuf>) -> Result<Self, Error> {
        let path = location
            .unwrap_or(PathBuf::from("./q_rules.json"))
            .canonicalize()?;

        //Import all the previously stored schedules
        let schedules = if path.exists() {
            let str = std::fs::read(&path)?;
            match serde_json::from_slice::<HashMap<Ulid, PlanUnit>>(&str) {
                Ok(items) => Some(items),
                Err(e) => {
                    warn!("q_rules parse error.");
                    warn!("{}", e);
                    None
                }
            }
        } else {
            None
        };
        let schedules = schedules.unwrap_or_else(|| {
            info!("No valid q_rules.json is found. It'll be created or overwritten just before exiting.");
            HashMap::new()
        });
        Ok(Self {
            inner: schedules,
            save_file_location: path,
        })
    }
    fn save(&mut self) {
        //Export remaining tasks
        let path = self
            .save_file_location
            .canonicalize()
            .unwrap_or(PathBuf::from("./q_rules.json"));
        let result = match serde_json::to_string(&self.inner) {
            Ok(str) => std::fs::write(&path, str),
            Err(e) => panic!("Serialization failed. {}", e),
        };
        if result.is_ok() {
            println!("q_rules is saved in {}.", path.display())
        }
    }
    pub(crate) fn try_insert(&mut self, plan: PlanUnit) -> Option<Ulid> {
        let found = self.inner.values().any(|f| *f == plan);

        if found {
            None
        } else {
            loop {
                let candidate = Ulid::new();
                if !self.inner.contains_key(&candidate) {
                    self.inner.insert(candidate, plan);
                    return Some(candidate);
                }
            }
        }
    }
    pub(crate) fn try_remove(&mut self, id: &Ulid) -> Option<PlanUnit> {
        self.inner.remove(id)
    }
    pub(crate) fn get_key_value(&mut self, k: &Ulid) -> Option<(&Ulid, &PlanUnit)> {
        self.inner.get_key_value(k)
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&Ulid, &PlanUnit)> {
        self.inner.iter()
    }
}

impl Drop for RulesResolverBase {
    fn drop(&mut self) {
        self.save()
    }
}
