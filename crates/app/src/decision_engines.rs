//! Which decision engine answers the typed questions behind theme ranking
//! and cut suggestions (issue #164). The user picks one in Settings › API
//! keys; the features ask the chosen engine for its provider (the key to
//! read, the budget, the cost record) and never name one themselves.

use std::sync::Arc;

use bardo_domain::{DecisionEngine, DecisionEngineId, UserProfile};

use crate::{AppError, Bardo};

/// The decision engines this build offers, in the order Settings lists
/// them. Never empty.
#[derive(Clone)]
pub(crate) struct DecisionEngines(Vec<Arc<dyn DecisionEngine>>);

impl DecisionEngines {
    pub(crate) fn new(engines: Vec<Arc<dyn DecisionEngine>>) -> Self {
        assert!(!engines.is_empty(), "Bardo needs a decision engine");
        Self(engines)
    }

    fn ids(&self) -> Vec<DecisionEngineId> {
        self.0.iter().map(|engine| engine.id()).collect()
    }

    /// The engine `id` names, or the first one offered when this build has
    /// no such engine.
    pub(crate) fn get(&self, id: DecisionEngineId) -> &Arc<dyn DecisionEngine> {
        self.0
            .iter()
            .find(|engine| engine.id() == id)
            .unwrap_or(&self.0[0])
    }
}

impl Bardo {
    /// The engines Settings offers.
    pub fn decision_engines(&self) -> Vec<DecisionEngineId> {
        self.decision_engines.ids()
    }

    /// The engine that answers now: the profile's choice while this build
    /// offers it, else the first one offered.
    pub fn decision_engine(&self) -> DecisionEngineId {
        self.decision_engines.get(self.profile.decision_engine).id()
    }

    /// Picks the engine for later rankings and cut suggestions and
    /// remembers it; jobs already queued keep theirs. On failure the
    /// current engine stays.
    pub fn set_decision_engine(&mut self, engine: DecisionEngineId) -> Result<(), AppError> {
        if engine == self.profile.decision_engine {
            return Ok(());
        }
        let updated = UserProfile {
            decision_engine: engine,
            ..self.profile.clone()
        };
        self.profiles.save(&updated)?;
        self.profile = updated;
        Ok(())
    }
}
