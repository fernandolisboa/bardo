//! Ad frameworks (docs/spec/marketing-videos.md): the shapes an ad's
//! script can take, as a problem and its solution, a demonstration, a
//! before and after and so on.
//!
//! The catalog is data, like the visual styles: a framework is a stable
//! code, its name and the steps a script written in it goes through, in
//! the English a model reads. An ad angle names one, and the ad script
//! follows its steps.
//!
//! There is no testimonial framework: an ad never presents an invented
//! customer.

use std::fmt;

/// A shape an ad's script can take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdFramework {
    ProblemSolution,
    Demonstration,
    BeforeAfter,
    HowItWorks,
    StoryBehind,
    OldWayNewWay,
    ThreeReasons,
    MythFact,
    Offer,
}

impl AdFramework {
    /// In the order pickers list them.
    pub const ALL: [AdFramework; 9] = [
        AdFramework::ProblemSolution,
        AdFramework::Demonstration,
        AdFramework::BeforeAfter,
        AdFramework::HowItWorks,
        AdFramework::StoryBehind,
        AdFramework::OldWayNewWay,
        AdFramework::ThreeReasons,
        AdFramework::MythFact,
        AdFramework::Offer,
    ];

    /// The stable code it is stored under, and the one a model answers
    /// with.
    pub fn code(self) -> &'static str {
        match self {
            AdFramework::ProblemSolution => "problem-solution",
            AdFramework::Demonstration => "demonstration",
            AdFramework::BeforeAfter => "before-after",
            AdFramework::HowItWorks => "how-it-works",
            AdFramework::StoryBehind => "story-behind",
            AdFramework::OldWayNewWay => "old-way-new-way",
            AdFramework::ThreeReasons => "three-reasons",
            AdFramework::MythFact => "myth-fact",
            AdFramework::Offer => "offer",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|framework| framework.code() == code)
    }

    /// Its name as a model reads it.
    pub fn name(self) -> &'static str {
        match self {
            AdFramework::ProblemSolution => "Problem and solution",
            AdFramework::Demonstration => "Demonstration",
            AdFramework::BeforeAfter => "Before and after",
            AdFramework::HowItWorks => "How it works",
            AdFramework::StoryBehind => "The story behind it",
            AdFramework::OldWayNewWay => "Old way and new way",
            AdFramework::ThreeReasons => "Three reasons",
            AdFramework::MythFact => "Myth and fact",
            AdFramework::Offer => "Offer",
        }
    }

    /// What a script written in it goes through, in order. Every one ends
    /// on what to do next.
    pub fn steps(self) -> &'static [&'static str] {
        match self {
            AdFramework::ProblemSolution => &[
                "Name a problem the viewer has, in the words they would use",
                "Show what it costs them, or why the usual fix falls short",
                "Present the product as the way out",
                NEXT_STEP,
            ],
            AdFramework::Demonstration => &[
                "Show the product at work on a real task, right away",
                "Follow the task to its result",
                "Point out what made it easy or possible",
                NEXT_STEP,
            ],
            AdFramework::BeforeAfter => &[
                "Show how things are before the product",
                "Show how they are after it",
                "Name the product as what changed them",
                NEXT_STEP,
            ],
            AdFramework::HowItWorks => &[
                "Say what the product does, in one sentence",
                "Walk through how it works in two or three steps",
                "Show the result",
                NEXT_STEP,
            ],
            AdFramework::StoryBehind => &[
                "Open on why the product was made: the moment or the frustration behind it",
                "Tell what was built, and for whom",
                "Show what it does for them now",
                NEXT_STEP,
            ],
            AdFramework::OldWayNewWay => &[
                "Show the old way of doing the job, and where it hurts",
                "Show the new way, with the product",
                "Name the difference that matters most",
                NEXT_STEP,
            ],
            AdFramework::ThreeReasons => &[
                "Open by promising three reasons",
                "Give the first reason",
                "Give the second reason",
                "Give the third reason, the strongest",
                NEXT_STEP,
            ],
            AdFramework::MythFact => &[
                "State a common belief about the problem or about products like this one",
                "Give the fact that corrects it",
                "Present the product as the choice that follows from the fact",
                NEXT_STEP,
            ],
            AdFramework::Offer => &[
                "Lead with the offer itself",
                "Say what the product is and who it is for",
                "Give the strongest reason to take the offer, with nothing the brief does not \
                 state",
                NEXT_STEP,
            ],
        }
    }

    /// The framework as one entry of the list a model picks from: its
    /// code, its name and its steps.
    pub fn line(self) -> String {
        format!(
            "{} ({}): {}.",
            self.code(),
            self.name(),
            self.steps().join("; ")
        )
    }

    /// Every framework, one per line, as a model picks from them.
    pub fn catalog() -> String {
        Self::ALL
            .into_iter()
            .map(|framework| format!("- {}", framework.line()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// How every framework ends.
const NEXT_STEP: &str = "Say what to do next";

impl fmt::Display for AdFramework {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn codes_round_trip_and_are_distinct() {
        let codes: HashSet<&str> = AdFramework::ALL
            .into_iter()
            .map(AdFramework::code)
            .collect();
        assert_eq!(codes.len(), AdFramework::ALL.len());
        for framework in AdFramework::ALL {
            assert_eq!(AdFramework::from_code(framework.code()), Some(framework));
        }
        assert_eq!(AdFramework::from_code("testimonial"), None);
    }

    #[test]
    fn every_framework_has_a_name_and_steps_that_end_on_what_to_do_next() {
        let names: HashSet<&str> = AdFramework::ALL
            .into_iter()
            .map(AdFramework::name)
            .collect();
        assert_eq!(names.len(), AdFramework::ALL.len());
        for framework in AdFramework::ALL {
            let steps = framework.steps();
            assert!(steps.len() >= 3, "{framework}");
            assert!(steps.iter().all(|step| !step.is_empty()), "{framework}");
            assert_eq!(steps.last(), Some(&NEXT_STEP), "{framework}");
        }
    }

    #[test]
    fn no_framework_presents_a_customer() {
        // An ad never invents a testimonial, so no framework asks for one.
        for framework in AdFramework::ALL {
            let text = framework.line().to_lowercase();
            for word in ["testimonial", "review", "customer says"] {
                assert!(!text.contains(word), "{framework}: {word}");
            }
        }
    }

    #[test]
    fn the_catalog_lists_every_framework_by_its_code() {
        let catalog = AdFramework::catalog();
        assert_eq!(catalog.lines().count(), AdFramework::ALL.len());
        for framework in AdFramework::ALL {
            assert!(catalog.contains(&format!("- {} (", framework.code())));
        }
        assert!(catalog.contains(
            "- before-after (Before and after): Show how things are before the product; Show \
             how they are after it; Name the product as what changed them; Say what to do next."
        ));
    }
}
