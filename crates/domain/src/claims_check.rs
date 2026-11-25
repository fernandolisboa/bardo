//! The claims check of an ad script (docs/spec/marketing-videos.md,
//! CONTEXT.md): what the script states about its product, each statement
//! tied to a claim of the Brief stage or left unbacked.
//!
//! A check belongs to the text and the brief it read. Once either changes
//! it is out of date, and checking again replaces it. An unbacked
//! statement locks nothing: it is shown where the user decides.

use std::time::SystemTime;

use crate::{AdBrief, ClaimId, JobId, ScriptText, VideoProjectId};

/// Which of a project's scripts a check read: the current one, or a
/// regenerated one waiting for review beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckedScript {
    Current,
    Pending,
}

impl CheckedScript {
    pub const ALL: [CheckedScript; 2] = [CheckedScript::Current, CheckedScript::Pending];

    /// The stable code it is stored under.
    pub fn code(self) -> &'static str {
        match self {
            CheckedScript::Current => "current",
            CheckedScript::Pending => "pending",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|which| which.code() == code)
    }
}

/// One thing a script states about its product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedStatement {
    /// The sentence of the script that states it, counted from 0 by
    /// [`ScriptText::sentences`]; `None` when the checker named a sentence
    /// the script does not have.
    pub sentence: Option<usize>,
    /// What it states, in the checker's words.
    pub says: String,
    /// The brief's claim that backs it; `None` when no claim does.
    pub claim: Option<ClaimId>,
}

impl CheckedStatement {
    /// In characters. The checker is asked for a few words; a longer
    /// answer is cut.
    pub const MAX_SAYS_CHARS: usize = 300;

    /// Whether no claim of the brief backs it.
    pub fn is_unbacked(&self) -> bool {
        self.claim.is_none()
    }
}

/// A script checked against the claims of its project's Brief stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimsCheck {
    pub project: VideoProjectId,
    pub of: CheckedScript,
    /// The script as the check read it.
    pub text: String,
    /// When the brief the check read had last changed.
    pub brief_at: SystemTime,
    /// The advertiser's own words as the check read them (the offer, the
    /// call to action and what must be said). The product's part of them
    /// changes without the brief changing.
    pub notes: String,
    pub statements: Vec<CheckedStatement>,
    pub checked_at: SystemTime,
    /// The model that checked it.
    pub model: String,
    /// The job that checked it, so a resumed job does not check twice.
    pub job: Option<JobId>,
}

impl ClaimsCheck {
    /// The statements no claim backs.
    pub fn unbacked(&self) -> impl Iterator<Item = &CheckedStatement> {
        self.statements
            .iter()
            .filter(|statement| statement.is_unbacked())
    }

    /// The claims the script uses, each once, in the order it first uses
    /// them.
    pub fn claims_used(&self) -> Vec<ClaimId> {
        let mut used = Vec::new();
        for claim in self
            .statements
            .iter()
            .filter_map(|statement| statement.claim)
        {
            if !used.contains(&claim) {
                used.push(claim);
            }
        }
        used
    }

    /// Whether `text` is the script the check read.
    pub fn reads(&self, text: &ScriptText) -> bool {
        self.text == text.as_str()
    }

    /// Whether the script, the brief or the advertiser's words (`notes`,
    /// as a check would read them now) changed since the check read them.
    pub fn is_outdated(&self, text: &ScriptText, brief: &AdBrief, notes: &str) -> bool {
        !self.reads(text) || brief.changed_at() > self.brief_at || self.notes != notes
    }

    /// The sentence of the checked script a statement names.
    pub fn sentence_of(&self, statement: &CheckedStatement) -> Option<&str> {
        crate::script::script_sentences(&self.text)
            .get(statement.sentence?)
            .copied()
    }

    /// The check of a regenerated script, once that script is accepted.
    pub fn accepted(mut self) -> Self {
        self.of = CheckedScript::Current;
        self
    }
}

/// The shape the checker answers in: every statement with the number of
/// its sentence, what it states and the number of the claim that backs it
/// (0 for none).
pub fn claims_check_schema() -> String {
    serde_json::json!({
        "type": "object",
        "properties": {
            "statements": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "sentence": {"type": "integer"},
                        "says": {"type": "string"},
                        "claim": {"type": "integer"},
                    },
                    "required": ["sentence", "says", "claim"],
                    "additionalProperties": false,
                },
            },
        },
        "required": ["statements"],
        "additionalProperties": false,
    })
    .to_string()
}

/// The checker's answer is not in the shape it was asked for.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unreadable claims check: {0}")]
pub struct ClaimsAnswerError(pub String);

/// The statements of the checker's `answer` about a script of `sentences`
/// sentences, with the brief's `claims` in the order the prompt numbered
/// them, from 1.
///
/// A statement tied to no claim, or to a number the prompt did not give,
/// is unbacked: the checker cannot vouch for a claim the brief does not
/// have. A statement that names a sentence the script does not have is
/// kept without one, so a flag is never lost to a miscount; so is one
/// whose words are missing, as long as it names a sentence. One that
/// names no sentence and says nothing is left out.
pub fn checked_statements(
    answer: &str,
    sentences: usize,
    claims: &[ClaimId],
) -> Result<Vec<CheckedStatement>, ClaimsAnswerError> {
    let root: serde_json::Value =
        serde_json::from_str(answer).map_err(|error| ClaimsAnswerError(error.to_string()))?;
    let listed = root
        .get("statements")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| ClaimsAnswerError("no list of statements".to_owned()))?;
    let number = |entry: &serde_json::Value, field: &str| {
        entry
            .get(field)
            .and_then(serde_json::Value::as_i64)
            .and_then(|number| usize::try_from(number).ok())
            .and_then(|number| number.checked_sub(1))
    };
    let mut statements = Vec::new();
    for entry in listed {
        if !entry.is_object() {
            return Err(ClaimsAnswerError("a statement is not an object".to_owned()));
        }
        let says: String = entry
            .get("says")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(CheckedStatement::MAX_SAYS_CHARS)
            .collect();
        let sentence = number(entry, "sentence").filter(|index| *index < sentences);
        if says.is_empty() && sentence.is_none() {
            continue;
        }
        statements.push(CheckedStatement {
            sentence,
            says,
            claim: number(entry, "claim").and_then(|index| claims.get(index).copied()),
        });
    }
    Ok(statements)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{
        AdAngleId, Product, ProductBrief, ProductDraft, ProductId, ProductSheet, ProfileId,
    };

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs)
    }

    const SCRIPT: &str = "Still squinting at your desk? The Lumen lamp folds flat. \
        It is the brightest lamp ever made. Order yours today.";

    fn check(statements: Vec<CheckedStatement>) -> ClaimsCheck {
        ClaimsCheck {
            project: VideoProjectId::new(),
            of: CheckedScript::Current,
            text: SCRIPT.to_owned(),
            brief_at: at(10),
            notes: "Call to action: Order yours today".to_owned(),
            statements,
            checked_at: at(20),
            model: "claude-test".to_owned(),
            job: None,
        }
    }

    fn brief(changed_at: SystemTime) -> AdBrief {
        let details = ProductBrief::validate(ProductDraft {
            name: "Lumen lamp".into(),
            description: "A desk lamp that folds flat.".into(),
            call_to_action: "Order yours today".into(),
            ..ProductDraft::default()
        })
        .unwrap();
        let product = Product::new(ProfileId::new(), details);
        let sheet = ProductSheet::empty(ProductId::new(), at(0));
        AdBrief::start(
            VideoProjectId::new(),
            &product,
            AdAngleId::new(),
            &sheet,
            changed_at,
        )
    }

    #[test]
    fn codes_round_trip() {
        for which in CheckedScript::ALL {
            assert_eq!(CheckedScript::from_code(which.code()), Some(which));
        }
        assert_eq!(CheckedScript::from_code("draft"), None);
    }

    #[test]
    fn an_answer_ties_each_statement_to_its_sentence_and_its_claim() {
        let claims = [ClaimId::new(), ClaimId::new()];
        let answer = r#"{"statements":[
            {"sentence":2,"says":"The lamp folds flat","claim":2},
            {"sentence":3,"says":"It is the brightest lamp ever made","claim":0}
        ]}"#;
        let statements = checked_statements(answer, 4, &claims).unwrap();
        assert_eq!(
            statements,
            [
                CheckedStatement {
                    sentence: Some(1),
                    says: "The lamp folds flat".into(),
                    claim: Some(claims[1]),
                },
                CheckedStatement {
                    sentence: Some(2),
                    says: "It is the brightest lamp ever made".into(),
                    claim: None,
                },
            ]
        );
        assert!(!statements[0].is_unbacked());
        assert!(statements[1].is_unbacked());
    }

    #[test]
    fn a_claim_the_brief_does_not_have_backs_nothing() {
        let claims = [ClaimId::new()];
        for named in ["2", "7", "-1", "null", "\"1\"", "1.5"] {
            let answer = format!(
                r#"{{"statements":[{{"sentence":1,"says":"Rated 4.9","claim":{named}}}]}}"#
            );
            let statements = checked_statements(&answer, 4, &claims).unwrap();
            assert!(statements[0].is_unbacked(), "{named}");
        }
        // No claim at all: every statement is unbacked.
        let answer = r#"{"statements":[{"sentence":1,"says":"It folds flat","claim":1}]}"#;
        assert!(checked_statements(answer, 4, &[]).unwrap()[0].is_unbacked());
    }

    #[test]
    fn a_statement_with_a_sentence_the_script_does_not_have_keeps_its_flag() {
        let claims = [ClaimId::new()];
        for named in ["0", "5", "-3", "null"] {
            let answer = format!(
                r#"{{"statements":[{{"sentence":{named},"says":"Rated 4.9","claim":0}}]}}"#
            );
            let statements = checked_statements(&answer, 4, &claims).unwrap();
            assert_eq!(statements.len(), 1, "{named}");
            assert_eq!(statements[0].sentence, None, "{named}");
            assert!(statements[0].is_unbacked(), "{named}");
        }
    }

    #[test]
    fn what_a_statement_says_is_one_line_within_its_limit() {
        let long = "word ".repeat(200);
        let answer = serde_json::json!({"statements": [
            {"sentence": 1, "says": "  It folds\n flat  ", "claim": 1},
            {"sentence": 1, "says": long, "claim": 0},
        ]})
        .to_string();
        let statements = checked_statements(&answer, 4, &[ClaimId::new()]).unwrap();
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[0].says, "It folds flat");
        assert_eq!(
            statements[1].says.chars().count(),
            CheckedStatement::MAX_SAYS_CHARS
        );
    }

    #[test]
    fn a_flag_without_words_is_kept_with_its_sentence() {
        let answer = serde_json::json!({"statements": [
            {"sentence": 3, "says": "   ", "claim": 0},
            {"sentence": 2, "claim": 0},
            // Neither a sentence nor words: nothing to show.
            {"sentence": 9, "says": "", "claim": 0},
        ]})
        .to_string();
        let statements = checked_statements(&answer, 4, &[ClaimId::new()]).unwrap();
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[0].sentence, Some(2));
        assert_eq!(statements[0].says, "");
        assert!(statements[0].is_unbacked());
        assert_eq!(statements[1].sentence, Some(1));
    }

    #[test]
    fn an_answer_in_another_shape_is_refused() {
        for answer in [
            "The script is fine.",
            r#"{"flags":[]}"#,
            r#"{"statements":"none"}"#,
            r#"{"statements":["It folds flat"]}"#,
        ] {
            assert!(checked_statements(answer, 4, &[]).is_err(), "{answer}");
        }
        assert_eq!(
            checked_statements(r#"{"statements":[]}"#, 4, &[]),
            Ok(vec![])
        );
    }

    #[test]
    fn the_schema_asks_for_a_sentence_what_it_says_and_a_claim() {
        let schema: serde_json::Value = serde_json::from_str(&claims_check_schema()).unwrap();
        let statement = &schema["properties"]["statements"]["items"];
        assert_eq!(
            statement["required"],
            serde_json::json!(["sentence", "says", "claim"])
        );
        assert_eq!(statement["additionalProperties"], false);
    }

    #[test]
    fn a_check_lists_what_is_unbacked_and_each_claim_used_once() {
        let (first, second) = (ClaimId::new(), ClaimId::new());
        let statement = |sentence: usize, claim: Option<ClaimId>| CheckedStatement {
            sentence: Some(sentence),
            says: "…".into(),
            claim,
        };
        let check = check(vec![
            statement(1, Some(second)),
            statement(2, None),
            statement(2, Some(first)),
            statement(3, Some(second)),
        ]);
        assert_eq!(check.unbacked().count(), 1);
        assert_eq!(check.claims_used(), [second, first]);
        assert_eq!(
            check.sentence_of(&check.statements[1]),
            Some("It is the brightest lamp ever made.")
        );
        let nowhere = CheckedStatement {
            sentence: None,
            says: "…".into(),
            claim: None,
        };
        assert_eq!(check.sentence_of(&nowhere), None);
        assert_eq!(check.sentence_of(&statement(9, None)), None);
    }

    #[test]
    fn a_check_is_out_of_date_once_what_it_read_changes() {
        let check = check(Vec::new());
        let same = ScriptText::new(SCRIPT).unwrap();
        let notes = check.notes.clone();
        assert!(check.reads(&same));
        assert!(!check.is_outdated(&same, &brief(at(10)), &notes));
        assert!(!check.is_outdated(&same, &brief(at(5)), &notes));
        // The brief changed after the check read it.
        assert!(check.is_outdated(&same, &brief(at(11)), &notes));
        // The script was edited.
        let edited = ScriptText::new(&SCRIPT.replace("brightest", "kindest")).unwrap();
        assert!(!check.reads(&edited));
        assert!(check.is_outdated(&edited, &brief(at(10)), &notes));
        // The product's own words changed, which the brief does not record.
        assert!(check.is_outdated(
            &same,
            &brief(at(10)),
            "Call to action: Order yours today\nMust be said: Sold by Lumen."
        ));
    }

    #[test]
    fn an_accepted_scripts_check_becomes_the_current_scripts() {
        let mut pending = check(Vec::new());
        pending.of = CheckedScript::Pending;
        assert_eq!(pending.accepted().of, CheckedScript::Current);
    }
}
