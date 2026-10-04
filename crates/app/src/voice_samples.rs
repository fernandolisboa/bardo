//! Voice samples on the Personas screen: hear a voice while choosing it
//! and its presets, before any narration is made with it.
//!
//! Two kinds: the provider's stock preview of a voice (free, but deaf to
//! the presets) and a short sentence read with the form's voice and presets
//! as they are, saved or not, through the narration adapter and model. A
//! reading is a paid call, recorded in costs like narration. Both are kept
//! on this machine by a key of everything that changes the sound, so
//! hearing the same voice, presets and sentence again costs nothing.
//!
//! Like the voice listing, a sample is prepared here (`voice_sample`,
//! `voice_preview`), made on a background thread (`VoiceSampling::run`)
//! and played on the screen's thread (`play_voice_sample`). A reading that
//! would reach the provider's budget asks first, like any generation.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

use bardo_domain::{
    ApiKey, CostPurpose, GenerationPresets, InvalidSampleText, Metered, Provider, ProviderFailure,
    Redactor, RepositoryError, SampleSource, SampleText, SpeechRequest, SpeechSynthesizer, Voice,
    VoicePreviews, VoiceRef, VoiceSampleStore,
};
use bardo_media::{Playback, PlaybackError};

use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::{Bardo, ProviderKeyError, Text};

/// Why a sample cannot be made or played.
#[derive(Debug, thiserror::Error)]
pub enum VoiceSampleError {
    #[error("no voice chosen")]
    NoVoice,
    /// The provider lists no stock preview for this voice.
    #[error("the voice has no stock preview")]
    NoPreview,
    #[error(transparent)]
    Text(#[from] InvalidSampleText),
    /// Reading a sample needs the voice provider's key.
    #[error("no {0} key saved")]
    MissingKey(Provider),
    #[error("could not read the {0} key")]
    KeyUnreadable(Provider),
    /// The reading would reach the provider's budget; the screen asks
    /// before reading it with `BudgetConsent::Confirmed`.
    #[error("over budget")]
    OverBudget(SpendEstimate),
    /// The provider refused or failed; the detail is already redacted.
    #[error("{}", .0.detail)]
    Provider(ProviderFailure),
    /// The sample was made but could not be kept on disk.
    #[error("could not keep the sample: {0}")]
    NotKept(String),
    #[error(transparent)]
    Playback(#[from] PlaybackError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl VoiceSampleError {
    /// What the Personas screen says.
    pub fn message(&self) -> Text {
        match self {
            VoiceSampleError::NoVoice => Text::VoiceSampleNoVoice,
            VoiceSampleError::NoPreview => Text::VoicePreviewNone,
            VoiceSampleError::Text(InvalidSampleText::TooLong) => Text::VoiceSampleTooLong,
            // Only a blank default sentence in a locale gets here.
            VoiceSampleError::Text(InvalidSampleText::Empty) => Text::VoiceSampleFailed,
            VoiceSampleError::MissingKey(_) => Text::VoiceSampleMissingKey,
            VoiceSampleError::KeyUnreadable(_) => Text::ProviderKeyStoreFailed,
            VoiceSampleError::OverBudget(_) => Text::BudgetReachedTitle,
            VoiceSampleError::Repository(_) => Text::VoiceSampleFailed,
            VoiceSampleError::Provider(_) => Text::VoiceSampleFailed,
            VoiceSampleError::NotKept(_) => Text::VoiceSampleNotKept,
            VoiceSampleError::Playback(_) => Text::VoiceSampleCannotPlay,
        }
    }

    /// The provider's own words, for a line under the message.
    pub fn detail(&self) -> Option<&str> {
        match self {
            VoiceSampleError::Provider(failure) => Some(&failure.detail),
            _ => None,
        }
    }
}

/// How the sample will be had.
enum Work {
    /// Already on this machine.
    Kept(PathBuf),
    Read {
        key: ApiKey,
        speech: Arc<dyn SpeechSynthesizer>,
    },
    Download {
        previews: Arc<dyn VoicePreviews>,
    },
}

/// Keys of the samples being made. A second request for one waits for the
/// first instead of paying for it again (the screen stops waiting for a
/// sample it no longer wants, but its call goes on).
#[derive(Debug, Default)]
pub(crate) struct SamplesInFlight {
    keys: Mutex<HashSet<String>>,
    done: Condvar,
}

impl SamplesInFlight {
    /// Waits while `key` is being made, then claims it until the claim is
    /// dropped.
    fn claim(self: &Arc<Self>, key: &str) -> Claim {
        let mut keys = self.keys.lock().expect("samples in flight lock");
        while keys.contains(key) {
            keys = self.done.wait(keys).expect("samples in flight lock");
        }
        keys.insert(key.to_owned());
        Claim {
            in_flight: Arc::clone(self),
            key: key.to_owned(),
        }
    }
}

struct Claim {
    in_flight: Arc<SamplesInFlight>,
    key: String,
}

impl Drop for Claim {
    fn drop(&mut self) {
        if let Ok(mut keys) = self.in_flight.keys.lock() {
            keys.remove(&self.key);
        }
        self.in_flight.done.notify_all();
    }
}

/// A sample ready to make. `run` may call the provider and blocks, so the
/// screen runs it on a background thread unless `is_kept`.
pub struct VoiceSampling {
    source: SampleSource,
    work: Work,
    store: Arc<dyn VoiceSampleStore>,
    in_flight: Arc<SamplesInFlight>,
    costs: CostBook,
    redactor: Redactor,
}

impl std::fmt::Debug for VoiceSampling {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VoiceSampling")
            .field("source", &self.source)
            .field("kept", &self.is_kept())
            .finish_non_exhaustive()
    }
}

impl VoiceSampling {
    pub fn source(&self) -> &SampleSource {
        &self.source
    }

    /// Whether the sample is already on this machine: playing it is free
    /// and instant.
    pub fn is_kept(&self) -> bool {
        matches!(self.work, Work::Kept(_))
    }

    /// Makes the sample (reads or downloads it unless kept), keeps it and
    /// records what a reading cost.
    pub fn run(self) -> VoiceSample {
        let result = self.make();
        if let Err(error) = &result {
            tracing::warn!(%error, "could not make a voice sample");
        }
        VoiceSample {
            source: self.source,
            result,
        }
    }

    fn make(&self) -> Result<SampleAudio, VoiceSampleError> {
        let key = self.source.key();
        let redacted = |mut failure: ProviderFailure| {
            failure.detail = self.redactor.redact(&failure.detail);
            VoiceSampleError::Provider(failure)
        };
        let kept = |path: PathBuf| SampleAudio {
            path,
            billed_characters: None,
        };
        if let Work::Kept(path) = &self.work {
            return Ok(kept(path.clone()));
        }
        // The same sample asked for twice: the second waits, then finds it.
        let _claim = self.in_flight.claim(&key);
        if let Some(path) = self.store.find(&key) {
            return Ok(kept(path));
        }
        let (audio, billed_characters) = match &self.work {
            Work::Kept(_) => unreachable!("returned above"),
            Work::Download { previews } => {
                let SampleSource::Preview { url, .. } = &self.source else {
                    unreachable!("downloads are prepared for previews only");
                };
                (previews.download(url).map_err(redacted)?, None)
            }
            Work::Read {
                key: api_key,
                speech,
            } => {
                let SampleSource::Reading {
                    voice,
                    presets,
                    text,
                    ..
                } = &self.source
                else {
                    unreachable!("readings are prepared for readings only");
                };
                let request = SpeechRequest {
                    voice: voice.clone(),
                    presets: *presets,
                    text: text.as_str().to_owned(),
                    previous_text: None,
                    next_text: None,
                };
                let speech = speech.synthesize(api_key, &request).map_err(redacted)?;
                // The money is spent whether or not the audio can be kept.
                self.costs.record(
                    PaidCall {
                        provider: voice.provider(),
                        model: &speech.model,
                        purpose: CostPurpose::VoiceSample,
                        usage: Metered::characters(speech.billed_characters),
                        job: None,
                        reported: None,
                    },
                    None,
                    None,
                );
                tracing::info!(characters = speech.billed_characters, "read a voice sample");
                (speech.audio, Some(speech.billed_characters))
            }
        };
        let path = self
            .store
            .keep(&key, &audio)
            .map_err(|error| VoiceSampleError::NotKept(error.to_string()))?;
        Ok(SampleAudio {
            path,
            billed_characters,
        })
    }
}

/// How making a sample ended.
#[derive(Debug)]
pub struct VoiceSample {
    pub source: SampleSource,
    pub result: Result<SampleAudio, VoiceSampleError>,
}

/// A sample on this machine, ready to play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleAudio {
    pub path: PathBuf,
    /// What the provider billed to read it just now; `None` when it cost
    /// nothing (kept from before, or a stock preview).
    pub billed_characters: Option<u64>,
}

/// A sample playing. Lives with the screen; dropping it stops the sound.
pub struct SamplePlayer {
    playback: Box<dyn Playback>,
}

impl SamplePlayer {
    /// Playing right now: not stopped and not at the end.
    pub fn is_playing(&self) -> bool {
        self.playback.is_playing()
    }

    pub fn stop(&mut self) {
        self.playback.pause();
    }
}

impl std::fmt::Debug for SamplePlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SamplePlayer")
            .field("playing", &self.is_playing())
            .finish()
    }
}

impl Bardo {
    /// The sentence a sample reads when the user types none, in the
    /// interface language.
    pub fn sample_sentence(&self) -> String {
        self.text(Text::VoiceSampleSentence).into_owned()
    }

    /// Prepares `text` (the default sentence when blank) read with `voice`
    /// and `presets`, as the narration model would read it. A sample kept
    /// from before needs no key and costs nothing; a new one that would
    /// reach the provider's budget needs `BudgetConsent::Confirmed`.
    pub fn voice_sample(
        &self,
        voice: Option<&VoiceRef>,
        presets: GenerationPresets,
        text: &str,
        consent: BudgetConsent,
    ) -> Result<VoiceSampling, VoiceSampleError> {
        let voice = voice.ok_or(VoiceSampleError::NoVoice)?;
        let text = if text.trim().is_empty() {
            SampleText::new(&self.sample_sentence())?
        } else {
            SampleText::new(text)?
        };
        let source = SampleSource::Reading {
            voice: voice.clone(),
            presets,
            text,
            model: bardo_ai::elevenlabs::SPEECH_MODEL.to_owned(),
        };
        let work = match self.voice_samples.find(&source.key()) {
            Some(path) => Work::Kept(path),
            None => {
                let provider = voice.provider();
                let call = PlannedCall::new(provider, CostPurpose::VoiceSample, 1)
                    .with_characters(text_chars(&source));
                if let Err(estimate) = self.check_budget(&[call], consent)? {
                    return Err(VoiceSampleError::OverBudget(estimate));
                }
                let key = self
                    .provider_keys
                    .read(self.profile.id, provider)
                    .map_err(|error| match error {
                        ProviderKeyError::NotSet => VoiceSampleError::MissingKey(provider),
                        _ => VoiceSampleError::KeyUnreadable(provider),
                    })?;
                Work::Read {
                    key,
                    speech: Arc::clone(&self.speech),
                }
            }
        };
        Ok(self.sampling(source, work))
    }

    /// Prepares the provider's stock preview of `voice`, from the voice
    /// listing. Free; no key goes with it.
    pub fn voice_preview(&self, voice: &Voice) -> Result<VoiceSampling, VoiceSampleError> {
        let url = voice
            .preview_url
            .clone()
            .ok_or(VoiceSampleError::NoPreview)?;
        let source = SampleSource::Preview {
            voice: voice.reference.clone(),
            url,
        };
        let work = match self.voice_samples.find(&source.key()) {
            Some(path) => Work::Kept(path),
            None => Work::Download {
                previews: Arc::clone(&self.previews),
            },
        };
        Ok(self.sampling(source, work))
    }

    fn sampling(&self, source: SampleSource, work: Work) -> VoiceSampling {
        VoiceSampling {
            source,
            work,
            store: Arc::clone(&self.voice_samples),
            in_flight: Arc::clone(&self.samples_in_flight),
            costs: self.cost_book.clone(),
            redactor: self.provider_keys.redactor().clone(),
        }
    }

    /// Starts playing a sample from the beginning.
    pub fn play_voice_sample(&self, audio: &SampleAudio) -> Result<SamplePlayer, VoiceSampleError> {
        let mut playback = self.audio.open(&audio.path)?;
        playback.play()?;
        Ok(SamplePlayer { playback })
    }
}

/// Characters a reading sends to the provider, which is what it bills.
fn text_chars(source: &SampleSource) -> usize {
    match source {
        SampleSource::Reading { text, .. } => text.as_str().chars().count(),
        SampleSource::Preview { .. } => 0,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::SystemTime;

    use bardo_domain::{CostRepository, Month, ProviderFailureKind, UiLanguage, VoiceCategory};
    use bardo_storage::{Database, MemorySecretStore, MemoryVoiceSamples};

    use super::*;
    use crate::narrations::testing::FakeAudioOutput;
    use crate::testing::{self, FakeSpeech};
    use crate::{Providers, Repositories};

    const ELEVENLABS_KEY: &str = "sk_test_elevenlabs_key_0001";
    const PREVIEW: &str = "https://storage.googleapis.com/eleven-public-prod/preview.mp3";

    /// Answers every download with `audio`, or `failure` when set; keeps
    /// the links it was asked for.
    #[derive(Default)]
    struct FakePreviews {
        failure: Mutex<Option<ProviderFailure>>,
        asked: Mutex<Vec<String>>,
    }

    impl VoicePreviews for FakePreviews {
        fn download(&self, url: &str) -> Result<Vec<u8>, ProviderFailure> {
            self.asked.lock().unwrap().push(url.to_owned());
            match self.failure.lock().unwrap().clone() {
                Some(failure) => Err(failure),
                None => Ok(b"ID3 preview".to_vec()),
            }
        }
    }

    struct Harness {
        db: Arc<Database>,
        speech: Arc<FakeSpeech>,
        previews: Arc<FakePreviews>,
        samples: Arc<MemoryVoiceSamples>,
        audio: Arc<FakeAudioOutput>,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                db: Arc::new(Database::open_in_memory().unwrap()),
                speech: Arc::default(),
                previews: Arc::default(),
                samples: Arc::default(),
                audio: Arc::default(),
            }
        }

        fn start(&self) -> Bardo {
            let providers = Providers {
                speech: Arc::clone(&self.speech) as _,
                previews: Arc::clone(&self.previews) as _,
                audio: Arc::clone(&self.audio) as _,
                ..testing::providers()
            };
            let repositories = Repositories {
                voice_samples: Arc::clone(&self.samples) as _,
                ..Repositories::shared(Arc::clone(&self.db), Arc::new(MemorySecretStore::default()))
            };
            Bardo::start(repositories, providers, Some("en-US")).unwrap()
        }

        fn start_with_key(&self) -> Bardo {
            let mut app = self.start();
            app.save_provider_key(Provider::ElevenLabs, ELEVENLABS_KEY)
                .unwrap();
            app
        }

        fn sample_costs(&self, app: &Bardo) -> Vec<bardo_domain::CostRecord> {
            let month = Month::of(SystemTime::now());
            CostRepository::costs_between(&*self.db, app.profile().id, month.start(), month.end())
                .unwrap()
                .into_iter()
                .filter(|record| record.purpose == CostPurpose::VoiceSample)
                .collect()
        }
    }

    fn wyatt() -> VoiceRef {
        VoiceRef::elevenlabs("FrS6cKLB1wg4WYgPa9GW", "Wyatt").unwrap()
    }

    fn presets(stability: u8) -> GenerationPresets {
        GenerationPresets {
            stability,
            ..GenerationPresets::default()
        }
    }

    fn listed(preview_url: Option<&str>) -> Voice {
        Voice {
            reference: wyatt(),
            category: VoiceCategory::Default,
            description: String::new(),
            labels: vec![],
            preview_url: preview_url.map(str::to_owned),
        }
    }

    #[test]
    fn a_sample_reads_the_sentence_with_the_forms_voice_and_presets() {
        let h = Harness::new();
        let app = h.start_with_key();
        let sampling = app
            .voice_sample(
                Some(&wyatt()),
                presets(30),
                "  Testing, one two.  ",
                BudgetConsent::Ask,
            )
            .unwrap();
        assert!(!sampling.is_kept());
        let sample = sampling.run();
        let audio = sample.result.unwrap();
        assert_eq!(audio.billed_characters, Some(17));

        let requests = h.speech.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].voice, wyatt());
        assert_eq!(requests[0].presets, presets(30));
        assert_eq!(requests[0].text, "Testing, one two.");
        assert_eq!(requests[0].previous_text, None);

        let key = sample.source.key();
        assert_eq!(h.samples.audio(&key).unwrap(), testing::PART_AUDIO);
        assert_eq!(h.samples.find(&key), Some(audio.path));
    }

    #[test]
    fn a_blank_sentence_reads_the_default_one_in_the_interface_language() {
        let h = Harness::new();
        let mut app = h.start_with_key();
        app.voice_sample(Some(&wyatt()), presets(50), " ", BudgetConsent::Ask)
            .unwrap()
            .run()
            .result
            .unwrap();
        app.set_ui_language(UiLanguage::PtBr).unwrap();
        app.voice_sample(Some(&wyatt()), presets(50), "", BudgetConsent::Ask)
            .unwrap()
            .run()
            .result
            .unwrap();
        let texts: Vec<_> = h.speech.requests().into_iter().map(|r| r.text).collect();
        assert_eq!(texts.len(), 2);
        assert_ne!(texts[0], texts[1], "one sentence per language");
        assert!(texts.iter().all(|text| !text.is_empty()));
        assert!(texts[1].contains("Olá"), "{}", texts[1]);
    }

    #[test]
    fn a_sample_heard_before_plays_again_without_a_call_or_a_cost() {
        let h = Harness::new();
        let app = h.start_with_key();
        let first = app
            .voice_sample(Some(&wyatt()), presets(30), "Hello.", BudgetConsent::Ask)
            .unwrap()
            .run()
            .result
            .unwrap();

        let again = app
            .voice_sample(Some(&wyatt()), presets(30), " Hello. ", BudgetConsent::Ask)
            .unwrap();
        assert!(again.is_kept());
        let again = again.run().result.unwrap();
        assert_eq!(again.path, first.path);
        assert_eq!(again.billed_characters, None, "free");
        assert_eq!(h.speech.requests().len(), 1, "no second call");
        assert_eq!(h.sample_costs(&app).len(), 1, "no second cost");

        // Another preset is another sample; going back is free again.
        app.voice_sample(Some(&wyatt()), presets(31), "Hello.", BudgetConsent::Ask)
            .unwrap()
            .run()
            .result
            .unwrap();
        assert_eq!(h.speech.requests().len(), 2);
        assert!(
            app.voice_sample(Some(&wyatt()), presets(30), "Hello.", BudgetConsent::Ask)
                .unwrap()
                .is_kept()
        );
    }

    #[test]
    fn each_reading_is_a_cost_record_of_the_model_that_spoke() {
        let h = Harness::new();
        let app = h.start_with_key();
        app.voice_sample(Some(&wyatt()), presets(30), "Hello.", BudgetConsent::Ask)
            .unwrap()
            .run()
            .result
            .unwrap();
        let costs = h.sample_costs(&app);
        assert_eq!(costs.len(), 1);
        let record = &costs[0];
        assert_eq!(record.provider, Provider::ElevenLabs);
        assert_eq!(record.usage, Metered::characters(6));
        assert_eq!(record.channel, None);
        assert_eq!(record.project, None);
        assert_eq!(record.job, None);
        assert_eq!(record.model, "eleven-fake", "the model that spoke");
    }

    #[test]
    fn a_kept_sample_plays_even_without_the_key() {
        let h = Harness::new();
        let mut app = h.start_with_key();
        app.voice_sample(Some(&wyatt()), presets(30), "Hello.", BudgetConsent::Ask)
            .unwrap()
            .run()
            .result
            .unwrap();
        app.remove_provider_key(Provider::ElevenLabs).unwrap();
        assert!(
            app.voice_sample(Some(&wyatt()), presets(30), "Hello.", BudgetConsent::Ask)
                .unwrap()
                .is_kept()
        );
        let error = app
            .voice_sample(Some(&wyatt()), presets(40), "Hello.", BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(
            error,
            VoiceSampleError::MissingKey(Provider::ElevenLabs)
        ));
        assert_eq!(error.message(), Text::VoiceSampleMissingKey);
    }

    #[test]
    fn a_new_reading_past_the_budget_asks_first_and_a_kept_one_never_does() {
        let h = Harness::new();
        let app = h.start_with_key();
        app.voice_sample(Some(&wyatt()), presets(30), "Hello.", BudgetConsent::Ask)
            .unwrap()
            .run()
            .result
            .unwrap();
        app.set_budget(Provider::ElevenLabs, "0.000001").unwrap();

        let error = app
            .voice_sample(Some(&wyatt()), presets(40), "Hello.", BudgetConsent::Ask)
            .unwrap_err();
        let VoiceSampleError::OverBudget(estimate) = &error else {
            panic!("{error:?}");
        };
        assert_eq!(estimate.providers[0].provider, Provider::ElevenLabs);
        assert_eq!(error.message(), Text::BudgetReachedTitle);
        assert_eq!(h.speech.requests().len(), 1, "no call before the answer");

        let read = app
            .voice_sample(
                Some(&wyatt()),
                presets(40),
                "Hello.",
                BudgetConsent::Confirmed,
            )
            .unwrap()
            .run()
            .result
            .unwrap();
        assert_eq!(read.billed_characters, Some(6));

        // Heard before: free, so nothing to ask.
        assert!(
            app.voice_sample(Some(&wyatt()), presets(30), "Hello.", BudgetConsent::Ask)
                .unwrap()
                .is_kept()
        );
    }

    #[test]
    fn the_same_reading_asked_for_twice_at_once_is_paid_once() {
        let h = Harness::new();
        let app = h.start_with_key();
        let prepare = || {
            app.voice_sample(Some(&wyatt()), presets(30), "Hello.", BudgetConsent::Ask)
                .unwrap()
        };
        // The screen stopped waiting for the first and asked again.
        let (first, second) = (prepare(), prepare());
        assert!(!first.is_kept() && !second.is_kept());
        let results = std::thread::scope(|scope| {
            let a = scope.spawn(|| first.run());
            let b = scope.spawn(|| second.run());
            [a.join().unwrap(), b.join().unwrap()]
        });

        assert_eq!(h.speech.requests().len(), 1);
        assert_eq!(h.sample_costs(&app).len(), 1);
        let billed: Vec<_> = results
            .iter()
            .map(|sample| sample.result.as_ref().unwrap().billed_characters)
            .collect();
        assert!(
            billed.contains(&Some(6)) && billed.contains(&None),
            "{billed:?}"
        );
    }

    #[test]
    fn a_sample_needs_a_voice_and_a_short_sentence() {
        let h = Harness::new();
        let app = h.start_with_key();
        let error = app
            .voice_sample(None, presets(30), "Hello.", BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(error, VoiceSampleError::NoVoice));
        assert_eq!(error.message(), Text::VoiceSampleNoVoice);

        let long = "a".repeat(SampleText::MAX_CHARS + 1);
        let error = app
            .voice_sample(Some(&wyatt()), presets(30), &long, BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(
            error,
            VoiceSampleError::Text(InvalidSampleText::TooLong)
        ));
        assert_eq!(error.message(), Text::VoiceSampleTooLong);
        assert!(h.speech.requests().is_empty());
    }

    #[test]
    fn a_provider_failure_is_shown_redacted_and_costs_nothing() {
        let h = Harness::new();
        *h.speech.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::LimitReached,
            format!("quota exceeded for {ELEVENLABS_KEY}"),
        ));
        let app = h.start_with_key();
        let error = app
            .voice_sample(Some(&wyatt()), presets(30), "Hello.", BudgetConsent::Ask)
            .unwrap()
            .run()
            .result
            .unwrap_err();
        assert_eq!(error.message(), Text::VoiceSampleFailed);
        let detail = error.detail().unwrap();
        assert!(!detail.contains(ELEVENLABS_KEY), "{detail}");
        assert!(detail.contains("quota exceeded"), "{detail}");
        assert!(h.samples.is_empty(), "nothing kept");
        assert!(h.sample_costs(&app).is_empty());
    }

    #[test]
    fn a_stock_preview_downloads_once_and_costs_nothing() {
        let h = Harness::new();
        // No key at all: previews are public.
        let app = h.start();
        let voice = listed(Some(PREVIEW));
        let sampling = app.voice_preview(&voice).unwrap();
        assert!(!sampling.is_kept());
        let audio = sampling.run().result.unwrap();
        assert_eq!(audio.billed_characters, None);
        assert_eq!(*h.previews.asked.lock().unwrap(), [PREVIEW]);

        let again = app.voice_preview(&voice).unwrap();
        assert!(again.is_kept());
        assert_eq!(again.run().result.unwrap().path, audio.path);
        assert_eq!(h.previews.asked.lock().unwrap().len(), 1);
        assert!(h.speech.requests().is_empty());
        assert!(h.sample_costs(&app).is_empty());
    }

    #[test]
    fn a_voice_without_a_preview_or_a_failed_download_says_so() {
        let h = Harness::new();
        let app = h.start();
        let error = app.voice_preview(&listed(None)).unwrap_err();
        assert!(matches!(error, VoiceSampleError::NoPreview));
        assert_eq!(error.message(), Text::VoicePreviewNone);

        *h.previews.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Unexpected,
            "ElevenLabs no longer has this preview; list the voices again",
        ));
        let error = app
            .voice_preview(&listed(Some(PREVIEW)))
            .unwrap()
            .run()
            .result
            .unwrap_err();
        assert_eq!(error.message(), Text::VoiceSampleFailed);
        assert!(error.detail().unwrap().contains("list the voices again"));
    }

    #[test]
    fn a_made_sample_plays_from_the_start() {
        let h = Harness::new();
        let app = h.start_with_key();
        let audio = app
            .voice_sample(Some(&wyatt()), presets(30), "Hello.", BudgetConsent::Ask)
            .unwrap()
            .run()
            .result
            .unwrap();
        let mut player = app.play_voice_sample(&audio).unwrap();
        assert_eq!(
            h.audio.opened.lock().unwrap().as_slice(),
            std::slice::from_ref(&audio.path)
        );
        assert!(player.is_playing());
        player.stop();
        assert!(!player.is_playing());

        *h.audio.failure.lock().unwrap() = Some(PlaybackError::NoOutput("no device".into()));
        let error = app.play_voice_sample(&audio).unwrap_err();
        assert_eq!(error.message(), Text::VoiceSampleCannotPlay);
    }
}
