//! PROTOTYPE — pure plan comparison and activation-state logic for issue #81.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactKind {
    Native,
    Wasm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanShape {
    pub module_id: &'static str,
    pub system_set: u64,
    pub invocation: u64,
    pub scheduling: u64,
}

pub const ACTIVE_PLAN: PlanShape = PlanShape {
    module_id: "wasvy.example:counter",
    system_set: 0x51,
    invocation: 0x19,
    scheduling: 0x73,
};

pub const INVOCATION_CHANGED_PLAN: PlanShape = PlanShape {
    invocation: 0x20,
    ..ACTIVE_PLAN
};

pub const SCHEDULING_CHANGED_PLAN: PlanShape = PlanShape {
    scheduling: 0x74,
    ..ACTIVE_PLAN
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionTransition {
    ReuseExecutors,
    ReplaceExecutors,
    ReplanSchedules,
    RejectIdentity,
}

pub fn assess_successor(active: PlanShape, candidate: PlanShape) -> ExecutionTransition {
    if active.module_id != candidate.module_id {
        return ExecutionTransition::RejectIdentity;
    }
    if active.system_set != candidate.system_set || active.scheduling != candidate.scheduling {
        return ExecutionTransition::ReplanSchedules;
    }
    if active.invocation != candidate.invocation {
        return ExecutionTransition::ReplaceExecutors;
    }
    ExecutionTransition::ReuseExecutors
}

#[derive(Clone, Copy, Debug)]
pub struct PrototypeState {
    pub generation: u64,
    pub artifact_kind: ArtifactKind,
    pub executor_installations: u64,
    pub last_assessment: Option<ExecutionTransition>,
    pub last_action: &'static str,
}

impl Default for PrototypeState {
    fn default() -> Self {
        Self {
            generation: 1,
            artifact_kind: ArtifactKind::Native,
            executor_installations: 1,
            last_assessment: None,
            last_action: "started with the built-in Native Artifact",
        }
    }
}

impl PrototypeState {
    pub fn publish_dispatch_compatible(&mut self, kind: ArtifactKind) {
        let assessment = assess_successor(ACTIVE_PLAN, ACTIVE_PLAN);
        assert_eq!(assessment, ExecutionTransition::ReuseExecutors);
        self.generation += 1;
        self.artifact_kind = kind;
        self.last_assessment = Some(assessment);
        self.last_action = match kind {
            ArtifactKind::Native => "atomically published a Native Generation",
            ArtifactKind::Wasm => "atomically published a Wasm Generation",
        };
    }

    pub fn inspect(&mut self, candidate: PlanShape, description: &'static str) {
        self.last_assessment = Some(assess_successor(ACTIVE_PLAN, candidate));
        self.last_action = description;
    }
}
