//! Every model Bardo offers is named in parts (#329): model, version and
//! variant, picked one at a time. No two models of a provider may share
//! all three, or one of them could never be picked.

use std::collections::HashSet;

use bardo_ai::{byteplus, google_clips, higgsfield, higgsfield_plan, kling, kling_takes};
use bardo_domain::{ClipModelRef, ModelName};

fn offered() -> Vec<(ClipModelRef, ModelName)> {
    let clips = [
        higgsfield::models(),
        higgsfield_plan::models(),
        kling::models(),
        google_clips::models(),
    ]
    .into_iter()
    .flatten()
    .map(|model| (model.id, model.name));
    let takes = [
        higgsfield::take_models(),
        kling_takes::take_models(),
        byteplus::take_models(),
    ]
    .into_iter()
    .flatten()
    .map(|model| (model.id, model.name));
    clips.chain(takes).collect()
}

#[test]
fn every_model_has_a_model_and_a_version() {
    for (id, name) in offered() {
        assert!(!name.family().is_empty(), "{id} has no model name");
        assert!(!name.version().is_empty(), "{id} has no version");
    }
}

#[test]
fn no_two_models_of_a_provider_share_a_name() {
    let mut clip_names = HashSet::new();
    for (id, name) in [
        higgsfield::models(),
        higgsfield_plan::models(),
        kling::models(),
        google_clips::models(),
    ]
    .into_iter()
    .flatten()
    .map(|model| (model.id, model.name))
    {
        assert!(
            clip_names.insert((id.provider(), name.clone())),
            "{name} twice on {}",
            id.provider()
        );
    }
    let mut take_names = HashSet::new();
    for (id, name) in [
        higgsfield::take_models(),
        kling_takes::take_models(),
        byteplus::take_models(),
    ]
    .into_iter()
    .flatten()
    .map(|model| (model.id, model.name))
    {
        assert!(
            take_names.insert((id.provider(), name.clone())),
            "{name} twice on {}",
            id.provider()
        );
    }
}

#[test]
fn the_parts_spell_the_names_people_know() {
    let names: Vec<String> = offered().iter().map(|(_, name)| name.to_string()).collect();
    for known in [
        "Kling 3.0 Standard",
        "Kling 2.5 Turbo Standard",
        "Hailuo 2.3 Standard",
        "MiniMax H3",
        "Kling 3.0 1080p",
        "Gemini Omni 1.2 Flash 720p",
        "Kling 3.0 Motion Control",
        "Kling 3.0 Motion Control 1080p",
        "Seedance 2.5 Video Edit",
    ] {
        assert!(names.iter().any(|name| name == known), "{known} missing");
    }
}
