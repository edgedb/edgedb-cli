use std::fmt;
use std::sync::Arc;

use crate::server::version::{Version, VersionSlot};


#[derive(Debug, Clone)]
pub struct DistributionRef(Arc<dyn Distribution>);

pub trait Distribution: downcast_rs::DowncastSync + fmt::Debug {
    fn version_slot(&self) -> &VersionSlot;
    fn version(&self) -> &Version<String>;
    fn into_ref(self) -> DistributionRef where Self: Sized {
        DistributionRef(Arc::new(self))
    }
}

downcast_rs::impl_downcast!(Distribution);

impl DistributionRef {
    pub fn version_slot(&self) -> &VersionSlot {
        self.0.version_slot()
    }
    pub fn version(&self) -> &Version<String> {
        self.0.version()
    }
    pub fn downcast_ref<T: Distribution>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
}
