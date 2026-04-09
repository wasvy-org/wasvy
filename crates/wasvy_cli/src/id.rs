use std::{any, fmt};

use crate::named::Named;

#[derive(Eq, PartialEq, Clone, Hash)]
pub struct Id {
    id: any::TypeId,
    name: String,
}

impl<T> From<&T> for Id
where
    T: Named + 'static,
{
    fn from(value: &T) -> Self {
        Self {
            id: any::TypeId::of::<T>(),
            name: value.name().to_string(),
        }
    }
}

impl Named for Id {
    fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Id").field(&self.name).finish()
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}
