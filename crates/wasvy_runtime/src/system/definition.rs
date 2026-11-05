use std::{fmt, marker::PhantomData, sync::Arc};

use anyhow::Result;
use bevy_ecs::{
    prelude::*,
    system::{BoxedSystem, IntoResult},
};
use std::collections::HashSet;

use crate::{
    schedule::ModScheduleName,
    system::{
        ModSystemBuild, ModSystemContext, ModSystemName, ModSystemParam, param::ModParamBuilder,
    },
};

/// A loader produces re-initializable system definitions that can be applied to any world as needed by the runtime.
///
/// For example:
/// - [Self::new] converts a native Bevy system into a definition
/// - `wasvy_wasm` resolves the systems inside a `.wasm` asset as [ModSystemDefinition]s
///
/// The runtime can add or remove copies of this definition when the mod is hot-reloaded.
#[derive(Clone)]
pub struct ModSystemDefinition {
    pub name: ModSystemName,
    pub schedules: HashSet<ModScheduleName>,
    pub before: HashSet<ModSystemName>,
    pub after: HashSet<ModSystemName>,
    builder: Arc<dyn ModSystemBuild>,
}

impl ModSystemDefinition {
    /// Convert a Bevy system function into a definition. This has no ordering constraints or schedule.
    ///
    /// Every copy of the system is built for the access it runs in: its queries only match entities in that access and
    /// its [RuntimeCommands](crate::commands::RuntimeCommands) spawn into it. See [ModSystemParam] for the parameters a
    /// system can use.
    ///
    /// Only functions and closures without system input are supported, not piped, exclusive or already built systems.
    pub fn new<M, F>(system: F) -> Self
    where
        M: 'static,
        F: SystemParamFunction<M, In = (), Out: IntoResult<()>> + Clone,
        F::Param: ModSystemParam,
    {
        let name = std::any::type_name::<F>();
        Self::custom(
            name,
            NativeSystem {
                system,
                marker: PhantomData,
            },
        )
    }

    /// Build a custom definition. Creates an unscheduled definition with no ordering constraints.
    pub fn custom(name: impl Into<ModSystemName>, builder: impl ModSystemBuild) -> Self {
        Self {
            name: name.into(),
            schedules: HashSet::new(),
            before: HashSet::new(),
            after: HashSet::new(),
            builder: Arc::new(builder),
        }
    }

    pub fn in_schedules(mut self, schedules: impl IntoIterator<Item = ModScheduleName>) -> Self {
        self.schedules.extend(schedules);
        self
    }
}

impl fmt::Debug for ModSystemDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModSystemDefinition")
            .field("name", &self.name)
            .field("schedules", &self.schedules)
            .field("before", &self.before)
            .field("after", &self.after)
            .finish_non_exhaustive()
    }
}

// Safety: passes on invariants
unsafe impl ModSystemBuild for ModSystemDefinition {
    fn build(&self, context: ModSystemContext<'_>) -> Result<BoxedSystem> {
        self.builder.build(context)
    }
}

/// Builds fresh copies of a native Bevy system, see [ModSystemDefinition::new]
struct NativeSystem<M, F> {
    system: F,
    marker: PhantomData<fn() -> M>,
}

// Safety:
// - The system is built from the context's world
// - Every parameter is built for the context's access, see ModSystemParam
// - Bevy's Commands do not implement ModSystemParam, so commands can only be queued with RuntimeCommands
unsafe impl<M, F> ModSystemBuild for NativeSystem<M, F>
where
    M: 'static,
    F: SystemParamFunction<M, In = (), Out: IntoResult<()>> + Clone,
    F::Param: ModSystemParam,
{
    fn build(&self, context: ModSystemContext<'_>) -> Result<BoxedSystem> {
        let system = ModParamBuilder(&context.access)
            .build_state(context.world)
            .build_any_system(self.system.clone())
            // Built systems are named after their parameters otherwise
            .with_name(std::any::type_name::<F>());
        Ok(Box::new(system))
    }
}
