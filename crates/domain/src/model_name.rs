//! How a video model is named, in parts, and picking one level at a time
//! (#329): provider, then model, then version, then variant. A flat list
//! of "Kling 3.0 Standard · Higgsfield", "Kling 3.0 1080p · Kling" and the
//! like is long and repeats itself; split, each choice is short.

use std::fmt;

use crate::{ClipModelRef, Provider};

/// A model's name in parts, e.g. Kling · 3.0 · Standard. Its full name is
/// the parts joined by spaces: "Kling 3.0 Standard".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelName {
    family: String,
    version: String,
    variant: String,
}

impl ModelName {
    /// `version` and `variant` may be empty (a model with no version, a
    /// version with no variants).
    pub fn new(family: &str, version: &str, variant: &str) -> Self {
        Self {
            family: family.trim().to_owned(),
            version: version.trim().to_owned(),
            variant: variant.trim().to_owned(),
        }
    }

    /// The model, as its maker names it: Kling, Seedance, Gemini Omni.
    pub fn family(&self) -> &str {
        &self.family
    }

    /// Its version: 3.0, 2.5 Turbo, 1.2 Flash, H3; empty when it has none.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The variant of that version: Standard, Pro, 1080p, Motion Control;
    /// empty when it has none.
    pub fn variant(&self) -> &str {
        &self.variant
    }

    /// The part of `level` (the provider is not one of the name's).
    pub fn part(&self, level: ModelLevel) -> &str {
        match level {
            ModelLevel::Provider => "",
            ModelLevel::Family => &self.family,
            ModelLevel::Version => &self.version,
            ModelLevel::Variant => &self.variant,
        }
    }
}

impl fmt::Display for ModelName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts = [&self.family, &self.version, &self.variant];
        let mut first = true;
        for part in parts.into_iter().filter(|part| !part.is_empty()) {
            if !first {
                f.write_str(" ")?;
            }
            f.write_str(part)?;
            first = false;
        }
        Ok(())
    }
}

/// Whether the full name is `other`, without building it.
impl PartialEq<str> for ModelName {
    fn eq(&self, other: &str) -> bool {
        let mut rest = other;
        let mut first = true;
        for part in [&self.family, &self.version, &self.variant] {
            if part.is_empty() {
                continue;
            }
            if !first {
                match rest.strip_prefix(' ') {
                    Some(after) => rest = after,
                    None => return false,
                }
            }
            match rest.strip_prefix(part.as_str()) {
                Some(after) => rest = after,
                None => return false,
            }
            first = false;
        }
        rest.is_empty()
    }
}

impl PartialEq<&str> for ModelName {
    fn eq(&self, other: &&str) -> bool {
        *self == **other
    }
}

/// A name with no parts but its model: for test models and the like.
impl From<&str> for ModelName {
    fn from(name: &str) -> Self {
        Self::new(name, "", "")
    }
}

/// A level of the pick, in the order it is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelLevel {
    Provider,
    Family,
    Version,
    Variant,
}

impl ModelLevel {
    pub const ALL: [ModelLevel; 4] = [
        ModelLevel::Provider,
        ModelLevel::Family,
        ModelLevel::Version,
        ModelLevel::Variant,
    ];

    /// The stable name its texts are found under.
    pub fn code(self) -> &'static str {
        match self {
            ModelLevel::Provider => "provider",
            ModelLevel::Family => "model",
            ModelLevel::Version => "version",
            ModelLevel::Variant => "variant",
        }
    }
}

/// A model offered for picking: its id and its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelOption<'a> {
    pub id: &'a ClipModelRef,
    pub name: &'a ModelName,
}

impl ModelOption<'_> {
    fn provider(&self) -> Provider {
        self.id.provider()
    }

    /// Whether it agrees with `other` on every level before `level`.
    fn under(&self, other: &ModelOption<'_>, level: ModelLevel) -> bool {
        ModelLevel::ALL
            .into_iter()
            .take_while(|before| *before != level)
            .all(|before| self.same(other, before))
    }

    fn same(&self, other: &ModelOption<'_>, level: ModelLevel) -> bool {
        match level {
            ModelLevel::Provider => self.provider() == other.provider(),
            level => self.name.part(level) == other.name.part(level),
        }
    }
}

/// What one level offers, with `picked` its current choice; the provider
/// level lists providers, the others a name's parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelChoices {
    pub level: ModelLevel,
    pub providers: Vec<Provider>,
    pub parts: Vec<String>,
}

impl LevelChoices {
    /// How many choices it has.
    pub fn len(&self) -> usize {
        match self.level {
            ModelLevel::Provider => self.providers.len(),
            _ => self.parts.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The choices each level offers given `picked`, in the order the models
/// come (a provider's default first): every provider; the models of the
/// picked one's provider; the versions of its model there; the variants
/// of that version. A level is left out when it has nothing to choose and
/// nothing to say: the variant of a version without variants. `None`
/// when `picked` is not among `options`.
pub fn model_levels(
    options: &[ModelOption<'_>],
    picked: &ClipModelRef,
) -> Option<Vec<LevelChoices>> {
    let current = options.iter().find(|option| option.id == picked)?;
    let mut levels = Vec::new();
    for level in ModelLevel::ALL {
        let under: Vec<&ModelOption<'_>> = options
            .iter()
            .filter(|option| option.under(current, level))
            .collect();
        let mut choices = LevelChoices {
            level,
            providers: Vec::new(),
            parts: Vec::new(),
        };
        for option in under {
            match level {
                ModelLevel::Provider => {
                    if !choices.providers.contains(&option.provider()) {
                        choices.providers.push(option.provider());
                    }
                }
                level => {
                    let part = option.name.part(level);
                    if !choices.parts.iter().any(|known| known == part) {
                        choices.parts.push(part.to_owned());
                    }
                }
            }
        }
        let nothing =
            level == ModelLevel::Variant && choices.parts.iter().all(|part| part.is_empty());
        if !nothing {
            levels.push(choices);
        }
    }
    Some(levels)
}

/// The value one level is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelPick<'a> {
    Provider(Provider),
    Part(ModelLevel, &'a str),
}

impl LevelPick<'_> {
    fn level(&self) -> ModelLevel {
        match self {
            LevelPick::Provider(_) => ModelLevel::Provider,
            LevelPick::Part(level, _) => *level,
        }
    }

    fn matches(&self, option: &ModelOption<'_>) -> bool {
        match self {
            LevelPick::Provider(provider) => option.provider() == *provider,
            LevelPick::Part(level, part) => option.name.part(*level) == *part,
        }
    }
}

/// The model picked when `pick` sets one level and `from` was the model
/// picked before (`None`: nothing picked, such as a channel following the
/// default). The levels before the one set stay; among the models they
/// and the pick allow, the one that keeps most of the levels after it, in
/// order, wins (Higgsfield's Kling 3.0 Standard, set to the Kling
/// provider, becomes Kling's Kling 3.0 720p); ties go to the first one
/// listed. `None` when no model fits.
pub fn pick_model<'a>(
    options: &[ModelOption<'a>],
    from: Option<&ClipModelRef>,
    pick: LevelPick<'_>,
) -> Option<&'a ClipModelRef> {
    let current = from.and_then(|from| options.iter().find(|option| option.id == from));
    let level = pick.level();
    let fits = options.iter().filter(|option| {
        pick.matches(option) && current.is_none_or(|current| option.under(current, level))
    });
    // The levels after the one set that it keeps, counted from the first
    // until one differs.
    let kept = |option: &ModelOption<'_>| -> usize {
        current.map_or(0, |current| {
            ModelLevel::ALL
                .into_iter()
                .skip_while(|after| *after != level)
                .skip(1)
                .take_while(|after| option.same(current, *after))
                .count()
        })
    };
    let mut best: Option<(&ModelOption<'a>, usize)> = None;
    for option in fits {
        let score = kept(option);
        if best.is_none_or(|(_, top)| score > top) {
            best = Some((option, score));
        }
    }
    best.map(|(option, _)| option.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Vec<(ClipModelRef, ModelName)> {
        let model = |provider, id: &str, family, version, variant| {
            (
                ClipModelRef::new(provider, id).unwrap(),
                ModelName::new(family, version, variant),
            )
        };
        vec![
            model(Provider::Higgsfield, "h/k30s", "Kling", "3.0", "Standard"),
            model(Provider::Higgsfield, "h/k26p", "Kling", "2.6", "Pro"),
            model(
                Provider::Higgsfield,
                "h/k25",
                "Kling",
                "2.5 Turbo",
                "Standard",
            ),
            model(Provider::Higgsfield, "h/sd20", "Seedance", "2.0", ""),
            model(Provider::Higgsfield, "h/mmh3", "MiniMax", "H3", ""),
            model(Provider::Kling, "k/30-720", "Kling", "3.0", "720p"),
            model(Provider::Kling, "k/30-1080", "Kling", "3.0", "1080p"),
            model(Provider::Kling, "k/26-720", "Kling", "2.6", "720p"),
            model(Provider::Kling, "k/26-1080", "Kling", "2.6", "1080p"),
            model(
                Provider::Gemini,
                "g/720",
                "Gemini Omni",
                "1.2 Flash",
                "720p",
            ),
            model(
                Provider::Gemini,
                "g/1080",
                "Gemini Omni",
                "1.2 Flash",
                "1080p",
            ),
        ]
    }

    fn options(catalog: &[(ClipModelRef, ModelName)]) -> Vec<ModelOption<'_>> {
        catalog
            .iter()
            .map(|(id, name)| ModelOption { id, name })
            .collect()
    }

    fn id(model: &str) -> ClipModelRef {
        let provider = match &model[..2] {
            "h/" => Provider::Higgsfield,
            "k/" => Provider::Kling,
            _ => Provider::Gemini,
        };
        ClipModelRef::new(provider, model).unwrap()
    }

    #[test]
    fn a_name_is_its_parts_joined_and_skips_empty_ones() {
        assert_eq!(
            ModelName::new("Kling", "2.5 Turbo", "Standard").to_string(),
            "Kling 2.5 Turbo Standard"
        );
        assert_eq!(
            ModelName::new("Seedance", "2.0", "").to_string(),
            "Seedance 2.0"
        );
        assert_eq!(ModelName::from("Fake Range").to_string(), "Fake Range");
        assert_eq!(ModelName::new(" Wan ", " 2.6", "").to_string(), "Wan 2.6");
    }

    #[test]
    fn a_name_equals_its_full_spelling_only() {
        let name = ModelName::new("Kling", "2.5 Turbo", "Standard");
        assert_eq!(name, "Kling 2.5 Turbo Standard");
        assert_ne!(name, "Kling 2.5 Turbo");
        assert_ne!(name, "Kling 2.5 Turbo Standard ");
        assert_ne!(name, "Kling2.5 Turbo Standard");
        assert_eq!(ModelName::new("Wan", "2.6", ""), "Wan 2.6");
    }

    #[test]
    fn levels_list_what_the_picked_models_provider_model_and_version_offer() {
        let catalog = catalog();
        let levels = model_levels(&options(&catalog), &id("k/30-1080")).unwrap();
        assert_eq!(levels.len(), 4);
        assert_eq!(
            levels[0].providers,
            [Provider::Higgsfield, Provider::Kling, Provider::Gemini]
        );
        assert_eq!(levels[1].parts, ["Kling"]);
        assert_eq!(levels[2].parts, ["3.0", "2.6"]);
        assert_eq!(levels[3].parts, ["720p", "1080p"]);
    }

    #[test]
    fn a_version_without_variants_has_no_variant_level() {
        let catalog = catalog();
        let levels = model_levels(&options(&catalog), &id("h/sd20")).unwrap();
        assert_eq!(
            levels.iter().map(|level| level.level).collect::<Vec<_>>(),
            [
                ModelLevel::Provider,
                ModelLevel::Family,
                ModelLevel::Version
            ]
        );
        assert_eq!(levels[1].parts, ["Kling", "Seedance", "MiniMax"]);
        assert_eq!(levels[2].parts, ["2.0"]);
    }

    #[test]
    fn a_model_not_offered_has_no_levels() {
        let catalog = catalog();
        assert_eq!(model_levels(&options(&catalog), &id("h/gone")), None);
    }

    #[test]
    fn another_provider_keeps_the_model_and_version_it_also_offers() {
        let catalog = catalog();
        let options = options(&catalog);
        let picked = pick_model(
            &options,
            Some(&id("h/k30s")),
            LevelPick::Provider(Provider::Kling),
        );
        assert_eq!(picked, Some(&id("k/30-720")));
        let picked = pick_model(
            &options,
            Some(&id("k/26-1080")),
            LevelPick::Provider(Provider::Higgsfield),
        );
        assert_eq!(picked, Some(&id("h/k26p")));
    }

    #[test]
    fn another_provider_without_the_model_takes_its_first() {
        let catalog = catalog();
        let options = options(&catalog);
        let picked = pick_model(
            &options,
            Some(&id("h/sd20")),
            LevelPick::Provider(Provider::Gemini),
        );
        assert_eq!(picked, Some(&id("g/720")));
    }

    #[test]
    fn another_version_keeps_the_variant_where_it_has_it() {
        let catalog = catalog();
        let options = options(&catalog);
        let picked = pick_model(
            &options,
            Some(&id("k/30-1080")),
            LevelPick::Part(ModelLevel::Version, "2.6"),
        );
        assert_eq!(picked, Some(&id("k/26-1080")));
        // Higgsfield's 2.5 Turbo has Standard, not Pro.
        let picked = pick_model(
            &options,
            Some(&id("h/k26p")),
            LevelPick::Part(ModelLevel::Version, "2.5 Turbo"),
        );
        assert_eq!(picked, Some(&id("h/k25")));
    }

    #[test]
    fn a_pick_stays_within_the_levels_before_it() {
        let catalog = catalog();
        let options = options(&catalog);
        // Kling 3.0 is offered by Higgsfield too, but the provider stays.
        let picked = pick_model(
            &options,
            Some(&id("k/26-720")),
            LevelPick::Part(ModelLevel::Version, "3.0"),
        );
        assert_eq!(picked, Some(&id("k/30-720")));
        // Seedance is not Kling's.
        let picked = pick_model(
            &options,
            Some(&id("k/26-720")),
            LevelPick::Part(ModelLevel::Family, "Seedance"),
        );
        assert_eq!(picked, None);
    }

    #[test]
    fn a_pick_from_nothing_takes_the_first_that_fits() {
        let catalog = catalog();
        let options = options(&catalog);
        assert_eq!(
            pick_model(&options, None, LevelPick::Provider(Provider::Kling)),
            Some(&id("k/30-720"))
        );
        assert_eq!(
            pick_model(&options, None, LevelPick::Provider(Provider::X)),
            None
        );
    }

    #[test]
    fn a_variant_pick_is_that_model() {
        let catalog = catalog();
        let options = options(&catalog);
        let picked = pick_model(
            &options,
            Some(&id("g/720")),
            LevelPick::Part(ModelLevel::Variant, "1080p"),
        );
        assert_eq!(picked, Some(&id("g/1080")));
    }
}
