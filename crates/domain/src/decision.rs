//! The decision engine (CONTEXT.md): answers typed questions about a piece
//! of content, with probabilities and confidence. It ranks, scores and
//! gates; it never writes content (ADR-0001).
//!
//! Three question kinds exist: choice (one option from a set), score (a
//! level on an ordered scale) and yes/no. Nothing here names a provider;
//! adapters translate questions and answers to their own protocol.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::{ApiKey, ProviderFailure};

/// How sure the engine is of an answer, from 0 (a coin toss) to 1 (no
/// doubt). Derived from how concentrated the answer's probabilities are, so
/// it is independent of which option won.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Confidence(f64);

impl Confidence {
    /// Values outside 0..=1 are clamped; NaN reads as no confidence.
    pub fn new(value: f64) -> Self {
        if value.is_nan() {
            return Self(0.0);
        }
        Self(value.clamp(0.0, 1.0))
    }

    pub fn value(self) -> f64 {
        self.0
    }

    /// Whole percent, rounded to the nearest.
    pub fn percent(self) -> u8 {
        (self.0 * 100.0).round() as u8
    }

    /// The confidence of a yes/no answer: how far its probability sits from
    /// an even split. The same formula as a two-option choice.
    pub fn of_yes_no(probability_yes: f64) -> Self {
        Self::new((2.0 * probability_yes - 1.0).abs())
    }
}

/// Why a question cannot be asked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidQuestion {
    #[error("question ids must be unique: {0}")]
    DuplicateId(String),
    #[error("question {0} has an empty id or instructions")]
    Empty(String),
    #[error("choice {0} needs between 2 and {max} options", max = Question::MAX_OPTIONS)]
    Options(String),
    #[error("score {0} needs between 2 and {max} levels", max = Question::MAX_LEVELS)]
    Levels(String),
}

/// One typed question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    /// Pick one option. Each option has a key (returned as the answer) and
    /// an optional description of when it applies.
    Choice {
        instructions: String,
        options: Vec<(String, Option<String>)>,
    },
    /// Rate on ordered levels, lowest first. Each level is described.
    Score {
        instructions: String,
        levels: Vec<String>,
    },
    /// Yes or no, optionally with what each means.
    YesNo {
        instructions: String,
        yes: Option<String>,
        no: Option<String>,
    },
}

impl Question {
    pub const MAX_OPTIONS: usize = 255;
    pub const MAX_LEVELS: usize = 10;

    pub fn instructions(&self) -> &str {
        match self {
            Question::Choice { instructions, .. }
            | Question::Score { instructions, .. }
            | Question::YesNo { instructions, .. } => instructions,
        }
    }
}

/// Questions asked together about one state, each under an id the caller
/// picks; answers come back under the same ids. Asking several at once is
/// cheaper than one call per question.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Questions(Vec<(String, Question)>);

impl Questions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a question, checking its shape and that `id` is new.
    pub fn ask(
        mut self,
        id: impl Into<String>,
        question: Question,
    ) -> Result<Self, InvalidQuestion> {
        let id = id.into();
        if id.trim().is_empty() || question.instructions().trim().is_empty() {
            return Err(InvalidQuestion::Empty(id));
        }
        match &question {
            Question::Choice { options, .. } => {
                let distinct: HashSet<&str> = options.iter().map(|(key, _)| key.as_str()).collect();
                if options.len() < 2
                    || options.len() > Question::MAX_OPTIONS
                    || distinct.len() != options.len()
                {
                    return Err(InvalidQuestion::Options(id));
                }
            }
            Question::Score { levels, .. } => {
                if levels.len() < 2 || levels.len() > Question::MAX_LEVELS {
                    return Err(InvalidQuestion::Levels(id));
                }
            }
            Question::YesNo { .. } => {}
        }
        if self.0.iter().any(|(existing, _)| *existing == id) {
            return Err(InvalidQuestion::DuplicateId(id));
        }
        self.0.push((id, question));
        Ok(self)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Question)> {
        self.0.iter().map(|(id, question)| (id.as_str(), question))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&Question> {
        self.0
            .iter()
            .find(|(existing, _)| existing == id)
            .map(|(_, question)| question)
    }
}

/// The answer to a choice: the most likely option and every option's
/// probability.
#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceAnswer {
    pub choice: String,
    pub probabilities: HashMap<String, f64>,
    pub confidence: Confidence,
}

/// The answer to a score: a probability-weighted level, which can land
/// between levels, and each level's probability.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreAnswer {
    /// From 0 (the first level) to `levels - 1` (the last).
    pub level: f64,
    /// Probability of each level, lowest first.
    pub probabilities: Vec<f64>,
    pub confidence: Confidence,
}

impl ScoreAnswer {
    /// The level on a 0–100 scale, so scores with different level counts
    /// compare.
    pub fn normalized(&self) -> crate::Score {
        let top = self.probabilities.len().saturating_sub(1).max(1) as f64;
        let fraction = (self.level / top).clamp(0.0, 1.0);
        crate::Score::new((fraction * 100.0).round() as u8)
    }
}

/// The answer to a yes/no question.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct YesNoAnswer {
    pub probability_yes: f64,
    pub confidence: Confidence,
}

impl YesNoAnswer {
    pub fn new(probability_yes: f64) -> Self {
        let probability_yes = probability_yes.clamp(0.0, 1.0);
        Self {
            probability_yes,
            confidence: Confidence::of_yes_no(probability_yes),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Choice(ChoiceAnswer),
    Score(ScoreAnswer),
    YesNo(YesNoAnswer),
}

/// Every answer of one call, by question id, and which model gave them.
#[derive(Debug, Clone, PartialEq)]
pub struct Decisions {
    pub answers: HashMap<String, Answer>,
    /// The engine's model and version, for the record.
    pub model: String,
}

impl Decisions {
    pub fn score(&self, id: &str) -> Option<&ScoreAnswer> {
        match self.answers.get(id)? {
            Answer::Score(answer) => Some(answer),
            _ => None,
        }
    }

    pub fn choice(&self, id: &str) -> Option<&ChoiceAnswer> {
        match self.answers.get(id)? {
            Answer::Choice(answer) => Some(answer),
            _ => None,
        }
    }

    pub fn yes_no(&self, id: &str) -> Option<&YesNoAnswer> {
        match self.answers.get(id)? {
            Answer::YesNo(answer) => Some(answer),
            _ => None,
        }
    }
}

/// Answers typed questions about a state (the content to judge, as text).
/// Calls the network and blocks, so it runs inside a job. An adapter
/// returns an answer of the asked kind for every question, or fails.
pub trait DecisionEngine: Send + Sync {
    fn decide(
        &self,
        key: &ApiKey,
        state: &str,
        questions: &Questions,
    ) -> Result<Decisions, ProviderFailure>;
}

impl<T: DecisionEngine + ?Sized> DecisionEngine for Arc<T> {
    fn decide(
        &self,
        key: &ApiKey,
        state: &str,
        questions: &Questions,
    ) -> Result<Decisions, ProviderFailure> {
        (**self).decide(key, state, questions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Score;

    fn score(levels: usize) -> Question {
        Question::Score {
            instructions: "How good?".into(),
            levels: (0..levels).map(|i| format!("level {i}")).collect(),
        }
    }

    fn choice(options: &[&str]) -> Question {
        Question::Choice {
            instructions: "Which one?".into(),
            options: options.iter().map(|o| ((*o).to_owned(), None)).collect(),
        }
    }

    #[test]
    fn questions_keep_their_order_and_ids() {
        let questions = Questions::new()
            .ask("b", score(3))
            .unwrap()
            .ask("a", choice(&["x", "y"]))
            .unwrap();
        let ids: Vec<_> = questions.iter().map(|(id, _)| id).collect();
        assert_eq!(ids, ["b", "a"]);
        assert_eq!(questions.len(), 2);
        assert!(questions.get("a").is_some());
    }

    #[test]
    fn ids_are_unique() {
        let error = Questions::new()
            .ask("q", score(3))
            .unwrap()
            .ask("q", score(3))
            .unwrap_err();
        assert_eq!(error, InvalidQuestion::DuplicateId("q".into()));
    }

    #[test]
    fn a_score_has_two_to_ten_levels() {
        assert!(Questions::new().ask("q", score(2)).is_ok());
        assert!(Questions::new().ask("q", score(10)).is_ok());
        for levels in [0, 1, 11] {
            assert_eq!(
                Questions::new().ask("q", score(levels)),
                Err(InvalidQuestion::Levels("q".into())),
                "{levels}"
            );
        }
    }

    #[test]
    fn a_choice_has_distinct_options() {
        assert!(Questions::new().ask("q", choice(&["a", "b"])).is_ok());
        for options in [&["a"][..], &["a", "a"][..]] {
            assert_eq!(
                Questions::new().ask("q", choice(options)),
                Err(InvalidQuestion::Options("q".into()))
            );
        }
    }

    #[test]
    fn a_question_needs_an_id_and_instructions() {
        let blank = Question::YesNo {
            instructions: "  ".into(),
            yes: None,
            no: None,
        };
        assert_eq!(
            Questions::new().ask("q", blank),
            Err(InvalidQuestion::Empty("q".into()))
        );
        assert_eq!(
            Questions::new().ask(" ", score(3)),
            Err(InvalidQuestion::Empty(" ".into()))
        );
    }

    #[test]
    fn confidence_is_clamped() {
        assert_eq!(Confidence::new(1.4).value(), 1.0);
        assert_eq!(Confidence::new(-0.2).value(), 0.0);
        assert_eq!(Confidence::new(f64::NAN).value(), 0.0);
        assert_eq!(Confidence::new(0.816).percent(), 82);
    }

    #[test]
    fn yes_no_confidence_grows_away_from_an_even_split() {
        assert_eq!(YesNoAnswer::new(0.5).confidence.value(), 0.0);
        assert_eq!(YesNoAnswer::new(0.95).confidence.percent(), 90);
        assert_eq!(YesNoAnswer::new(0.05).confidence.percent(), 90);
        assert_eq!(YesNoAnswer::new(1.0).confidence.value(), 1.0);
    }

    #[test]
    fn a_score_normalizes_to_0_100_whatever_its_levels() {
        let answer = |level: f64, levels: usize| ScoreAnswer {
            level,
            probabilities: vec![0.0; levels],
            confidence: Confidence::default(),
        };
        assert_eq!(answer(0.0, 5).normalized(), Score::new(0));
        assert_eq!(answer(4.0, 5).normalized(), Score::MAX);
        assert_eq!(answer(2.0, 5).normalized(), Score::new(50));
        assert_eq!(answer(1.05, 3).normalized(), Score::new(53));
        assert_eq!(answer(9.0, 3).normalized(), Score::MAX, "clamped");
    }

    #[test]
    fn decisions_read_answers_by_kind() {
        let decisions = Decisions {
            answers: HashMap::from([("urgent".to_owned(), Answer::YesNo(YesNoAnswer::new(0.9)))]),
            model: "engine-1".into(),
        };
        assert!(decisions.yes_no("urgent").is_some());
        assert!(decisions.score("urgent").is_none(), "wrong kind");
        assert!(decisions.choice("missing").is_none());
    }
}
