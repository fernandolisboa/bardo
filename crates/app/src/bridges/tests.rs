use std::sync::Mutex;
use std::time::Duration;

use bardo_domain::{
    AspectRatio, CostRepository, JobState, MediaAssetRepository, Meter, VideoProject, bridge_prompt,
};

use super::*;
use crate::editor::{EditAction, Editor};
use crate::scenes::tests::{Harness, done, wait_done};

const PATIENCE: Duration = Duration::from_secs(10);

/// A drawn project open in the editor, its proxies built.
fn opened(h: &Harness, app: &Bardo) -> (VideoProject, Editor) {
    // Long enough a narration for a bridge to fit around its cuts.
    *h.speech.length.lock().unwrap() = Duration::from_secs(30);
    let (project, _) = h.drawn_project(app);
    let mut editor = app.open_editor(project.id).unwrap();
    settle(app, &mut editor);
    (project, editor)
}

fn settle(app: &Bardo, editor: &mut Editor) {
    let deadline = std::time::Instant::now() + PATIENCE;
    loop {
        app.refresh_editor(editor).unwrap();
        if !editor.view().is_building() {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "never settled");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The ask for a bridge over cut `cut` with the fake range model.
fn ask(cut: usize, seconds: u32, note: &str) -> BridgeAsk {
    BridgeAsk {
        cut,
        provider: Provider::Higgsfield,
        model: "fake/range".into(),
        seconds,
        note: note.into(),
    }
}

fn bridge_costs(h: &Harness, app: &Bardo) -> Vec<bardo_domain::CostRecord> {
    h.db.costs_between(
        app.profile().id,
        std::time::SystemTime::UNIX_EPOCH,
        std::time::SystemTime::now() + Duration::from_secs(60),
    )
    .unwrap()
    .into_iter()
    .filter(|record| record.purpose == CostPurpose::BridgeClip)
    .collect()
}

/// Makes a bridge over cut 1 and waits for its job.
fn made(app: &Bardo, editor: &mut Editor, note: &str) -> (BridgeId, Job) {
    let id = app
        .make_bridge(editor, ask(1, 3, note), BudgetConsent::Ask)
        .unwrap();
    let job = editor.view().bridges[0].job;
    let job = done(app, job);
    app.refresh_editor(editor).unwrap();
    (id, job)
}

#[test]
fn the_panel_offers_the_models_that_end_on_a_frame_with_the_lengths_that_fit() {
    let h = Harness::new();
    let app = h.start();
    let (_, editor) = opened(&h, &app);
    app.save_rate(Provider::Higgsfield, "fake/", Meter::VideoSeconds, "0.10")
        .unwrap();
    let timeline = editor.view().timeline.clone().unwrap();
    let options = app.bridge_options(&editor, 1).unwrap();
    assert_eq!(options.refusal, None);
    assert!(options.any_model);
    // The fake choices model cannot end on a frame.
    let names: Vec<&str> = options
        .models
        .iter()
        .map(|model| model.model.id.model())
        .collect();
    assert_eq!(names, ["fake/range"]);
    let lengths = &options.models[0].lengths;
    assert_eq!(lengths[0].seconds, 3, "shortest first");
    assert_eq!(lengths[0].price, Some(Money::from_cents(30)));
    // Only what the two pictures around the cut can give.
    for length in lengths {
        assert!(timeline.bridge_span(1, length.seconds).is_ok());
    }
    assert!(
        timeline
            .bridge_span(1, lengths.last().unwrap().seconds + 1)
            .is_err()
            || lengths.last().unwrap().seconds == 15
    );
    // No cut before the first clip.
    let first = app.bridge_options(&editor, 0).unwrap();
    assert_eq!(first.refusal, Some(BridgeRefusal::NoCut));
    assert!(first.models.is_empty());
}

#[test]
fn without_a_key_no_model_is_offered_and_nothing_is_asked() {
    let h = Harness::new();
    let mut app = h.start();
    let (_, mut editor) = opened(&h, &app);
    app.remove_provider_key(Provider::Higgsfield).unwrap();
    let options = app.bridge_options(&editor, 1).unwrap();
    assert!(options.models.is_empty());
    assert!(options.any_model, "the panel says to set up a key");
    let error = app
        .make_bridge(&mut editor, ask(1, 3, ""), BudgetConsent::Ask)
        .unwrap_err();
    assert!(matches!(error, BridgeError::NoModel));
    assert_eq!(error.message(), Text::EditorBridgeNoModel);
    assert!(h.clips.submissions().is_empty());
}

#[test]
fn a_bridge_sends_the_frames_around_the_cut_once_and_waits_on_it_for_review() {
    let h = Harness::new();
    let app = h.start();
    let (_, mut editor) = opened(&h, &app);
    *h.clips.quote.lock().unwrap() = Some(Money::from_cents(42));
    let before = editor.view().timeline.clone().unwrap();
    let span = before.bridge_span(1, 3).unwrap();
    let (first_source, last_source) = before.bridge_ends(&span).unwrap();

    let (id, job) = made(&app, &mut editor, " a slow pan ");

    // Both ends read from their pictures, the last at the first's size.
    let frames = h.media.bridge_frames.lock().unwrap().clone();
    assert_eq!(frames.len(), 2);
    assert!(frames[0].0.ends_with(first_source.file()));
    assert_eq!((frames[0].1, frames[0].2), (None, None), "a still, whole");
    assert!(frames[1].0.ends_with(last_source.file()));
    assert_eq!(frames[1].2, Some((1280, 720)));
    // Both handed over, then one submission that ends on the last.
    assert_eq!(h.clips.staged.lock().unwrap().len(), 2);
    let sent = h.clips.requests.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    let request = &sent[0].request;
    assert_eq!(request.model, "fake/range");
    assert_eq!(request.prompt, bridge_prompt("a slow pan"));
    assert_eq!(request.seconds, 3);
    assert_eq!(request.aspect, AspectRatio::Landscape);
    assert_eq!(request.first_frame.format, ImageFormat::Jpeg);
    let last = request.last.as_ref().unwrap();
    assert_eq!(last.image, StagedImage("https://fake/image-2".into()));
    assert_eq!(
        bardo_domain::image_dimensions(&last.frame.bytes),
        bardo_domain::image_dimensions(&request.first_frame.bytes)
    );
    // Its own idempotency keys, never a scene clip's.
    assert_eq!(sent[0].submission, format!("{}-bridge-1", job.id()));

    // Paid once, at the provider's quote.
    let costs = bridge_costs(&h, &app);
    assert_eq!(costs.len(), 1);
    assert_eq!(
        costs[0].cost,
        bardo_domain::Cost::Reported(Money::from_cents(42))
    );
    assert_eq!(costs[0].job, Some(job.id()));

    // It waits on its cut; the cut is as it was.
    let view = editor.view();
    assert_eq!(view.timeline.as_ref(), Some(&before));
    assert_eq!(view.bridges.len(), 1);
    let bridge = &view.bridges[0];
    assert_eq!(bridge.id, id);
    assert_eq!(bridge.cut, Some(1));
    assert_eq!((bridge.from, bridge.to), (span.start(), span.end()));
    assert!(bridge.is_ready());
    assert_eq!(bridge.model, "Fake Range");
    let file = bridge.file.clone().unwrap();
    assert_eq!(
        h.files.read(view.project.id, &file).unwrap(),
        crate::testing::CLIP
    );
    // Its frames are gone with the clip made.
    let (first, last) = Bridge::frame_names(id);
    assert!(!h.files.exists(view.project.id, &first));
    assert!(!h.files.exists(view.project.id, &last));
    // A second bridge on the same cut waits for this one.
    let error = app
        .make_bridge(&mut editor, ask(1, 3, ""), BudgetConsent::Ask)
        .unwrap_err();
    assert!(matches!(error, BridgeError::Busy));
}

#[test]
fn using_a_bridge_places_it_over_its_stretch_in_one_undoable_edit() {
    let h = Harness::new();
    let app = h.start();
    let (project, mut editor) = opened(&h, &app);
    let before = editor.view().timeline.clone().unwrap();
    let (id, _) = made(&app, &mut editor, "");
    let file = editor.view().bridges[0].file.clone().unwrap();

    app.use_bridge(&mut editor, id).unwrap();

    let after = editor.view().timeline.clone().unwrap();
    assert_eq!(after.duration(), before.duration(), "no time added");
    assert_eq!(after.video().len(), before.video().len() + 1);
    let placed = &after.video()[1];
    assert_eq!(placed.source.file(), Some(file.as_str()));
    let span = before.bridge_span(1, 3).unwrap();
    assert_eq!((placed.at, placed.end()), (span.start(), span.end()));
    assert_eq!(editor.selection(), Some(ItemRef::video(1)));
    // A project asset now, in the bin; the bridge is no longer waiting.
    let assets = app.media_assets.media_assets(project.id).unwrap();
    let asset = assets.iter().find(|asset| asset.file == file).unwrap();
    assert_eq!(asset.source, AssetSource::Generated);
    assert_eq!(asset.kind, MediaKind::Video);
    assert!(editor.view().bridges.is_empty());
    assert!(app.bridges.bridges(project.id).unwrap().is_empty());
    // Undone like any edit; the asset stays in the bin.
    app.edit(&mut editor, EditAction::Undo).unwrap();
    assert_eq!(editor.view().timeline.as_ref(), Some(&before));
    app.edit(&mut editor, EditAction::Redo).unwrap();
    assert_eq!(editor.view().timeline.as_ref(), Some(&after));
}

#[test]
fn a_bridge_plays_on_its_cut_in_the_preview_before_it_is_used() {
    let h = Harness::new();
    let app = h.start();
    let (_, mut editor) = opened(&h, &app);
    let (id, _) = made(&app, &mut editor, "");
    settle(&app, &mut editor);
    let file = editor.view().bridges[0].file.clone().unwrap();
    let plays = |editor: &Editor| {
        editor.preview_plan().unwrap().video.iter().any(|clip| {
            matches!(&clip.source, bardo_media::ffmpeg::ClipSource::Video(path)
                if path.to_string_lossy().contains(file.trim_end_matches(".mp4")))
        })
    };
    assert!(!plays(&editor));
    editor.preview_bridge(Some(id));
    assert_eq!(editor.previewed_bridge(), Some(id));
    assert!(plays(&editor), "its proxy plays over the stretch");
    assert_eq!(
        editor.playhead(),
        editor.view().bridges[0].from - Duration::from_secs(1)
    );
    // Nothing was placed.
    assert_eq!(
        editor.view().timeline.as_ref().unwrap().video().len(),
        editor.view().clips.len()
    );
    editor.preview_bridge(None);
    assert!(!plays(&editor));
}

#[test]
fn a_bridge_whose_cut_changed_can_only_be_discarded() {
    let h = Harness::new();
    let app = h.start();
    let (project, mut editor) = opened(&h, &app);
    let (id, _) = made(&app, &mut editor, "");
    let file = editor.view().bridges[0].file.clone().unwrap();
    // The clip before the cut gets shorter: both pictures move.
    let end = editor.view().clips[0].at + editor.view().clips[0].duration;
    app.edit(
        &mut editor,
        EditAction::Trim {
            item: ItemRef::video(0),
            edge: bardo_domain::Edge::End,
            to: end - Duration::from_millis(500),
            reach: Duration::ZERO,
        },
    )
    .unwrap();
    assert_eq!(editor.view().bridges[0].cut, None);
    let error = app.use_bridge(&mut editor, id).unwrap_err();
    assert!(matches!(error, BridgeError::CutChanged));
    assert_eq!(error.message(), Text::EditorBridgeStale);
    app.discard_bridge(&mut editor, id).unwrap();
    assert!(editor.view().bridges.is_empty());
    assert!(!h.files.exists(project.id, &file));
}

#[test]
fn a_bridge_being_made_is_not_discarded_and_holds_nothing_on_the_cut() {
    let h = Harness::new();
    let app = h.start();
    let (_, mut editor) = opened(&h, &app);
    h.clips.hold(true);
    let id = app
        .make_bridge(&mut editor, ask(1, 3, ""), BudgetConsent::Ask)
        .unwrap();
    let deadline = std::time::Instant::now() + PATIENCE;
    while h.clips.polls.lock().unwrap().is_empty() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    app.refresh_editor(&mut editor).unwrap();
    assert_eq!(editor.view().bridges[0].state, BridgeState::Making);
    assert!(matches!(
        app.use_bridge(&mut editor, id),
        Err(BridgeError::NotReady)
    ));
    assert!(matches!(
        app.discard_bridge(&mut editor, id),
        Err(BridgeError::StillMaking)
    ));
    // The clip is being made and billed: it counts already.
    assert_eq!(bridge_costs(&h, &app).len(), 1);
    h.clips.hold(false);
}

#[test]
fn a_bridge_the_provider_fails_is_not_billed_and_a_retry_sends_it_anew() {
    let h = Harness::new();
    let app = h.start();
    let (_, mut editor) = opened(&h, &app);
    h.clips.fail("first frame to the last");
    app.make_bridge(&mut editor, ask(1, 3, ""), BudgetConsent::Ask)
        .unwrap();
    let job = editor.view().bridges[0].job;
    let failed = wait_done(&app, job);
    assert_eq!(failed.state(), JobState::Failed);
    assert!(
        failed.failure().unwrap().detail.starts_with("The bridge:"),
        "{:?}",
        failed.failure()
    );
    assert!(bridge_costs(&h, &app).is_empty(), "taken back");
    app.refresh_editor(&mut editor).unwrap();
    assert!(matches!(
        editor.view().bridges[0].state,
        BridgeState::Failed(Some(_))
    ));

    h.clips.succeed_all();
    app.retry_job(job).unwrap();
    done(&app, job);
    let sent = h.clips.requests.lock().unwrap().clone();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].submission, format!("{job}-bridge-2"));
    // Both frames handed over again for the new submission.
    assert_eq!(h.clips.staged.lock().unwrap().len(), 4);
    assert_eq!(bridge_costs(&h, &app).len(), 1);
    app.refresh_editor(&mut editor).unwrap();
    assert!(editor.view().bridges[0].is_ready());
}

#[test]
fn a_restart_while_a_bridge_is_made_polls_the_same_request() {
    let h = Harness::new();
    let app = h.start();
    let (project, mut editor) = opened(&h, &app);
    h.clips.hold(true);
    app.make_bridge(&mut editor, ask(1, 3, ""), BudgetConsent::Ask)
        .unwrap();
    let job = editor.view().bridges[0].job;
    let deadline = std::time::Instant::now() + PATIENCE;
    while h.clips.polls.lock().unwrap().is_empty() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    drop(editor);
    drop(app);

    h.clips.hold(false);
    let app = h.start();
    done(&app, job);
    assert_eq!(h.clips.submissions().len(), 1, "submitted once");
    assert_eq!(h.clips.staged.lock().unwrap().len(), 2, "staged once");
    assert_eq!(bridge_costs(&h, &app).len(), 1, "paid once");
    let editor = app.open_editor(project.id).unwrap();
    assert!(editor.view().bridges[0].is_ready());
}

#[test]
fn a_bridge_whose_answer_was_lost_is_found_instead_of_paid_for_twice() {
    let mut h = Harness::new();
    let clips = Arc::new(crate::testing::FakeClips {
        provider: Some(Provider::Kling),
        no_idempotency: true,
        lost: Mutex::new(1),
        ..crate::testing::FakeClips::default()
    });
    h.more_clips.push(Arc::clone(&clips) as _);
    let mut app = h.start();
    app.save_provider_key(Provider::Kling, "kling-key").unwrap();
    let (_, mut editor) = opened(&h, &app);
    app.make_bridge(
        &mut editor,
        BridgeAsk {
            provider: Provider::Kling,
            ..ask(1, 3, "")
        },
        BudgetConsent::Ask,
    )
    .unwrap();
    let job = editor.view().bridges[0].job;
    done(&app, job);
    assert_eq!(*clips.sends.lock().unwrap(), 1, "sent once");
    assert_eq!(*clips.lookups.lock().unwrap(), 1);
    assert_eq!(bridge_costs(&h, &app).len(), 1);
}

#[test]
fn a_model_that_cannot_end_on_a_frame_gets_nothing_handed_over() {
    let h = Harness::new();
    let app = h.start();
    let (project, mut editor) = opened(&h, &app);
    let error = app
        .make_bridge(
            &mut editor,
            BridgeAsk {
                model: "fake/choices".into(),
                seconds: 5,
                ..ask(1, 3, "")
            },
            BudgetConsent::Ask,
        )
        .unwrap_err();
    assert!(matches!(error, BridgeError::NotOffered));
    // A job for such a model (one retired since it was queued, say)
    // fails before staging anything.
    let timeline = editor.view().timeline.clone().unwrap();
    let span = timeline.bridge_span(1, 5).unwrap();
    let (first, last) = timeline.bridge_ends(&span).unwrap();
    let id = BridgeId::new();
    let payload = BridgePayload {
        project: project.id.to_string(),
        bridge: id.to_string(),
        provider: Provider::Higgsfield.code().into(),
        model: "fake/choices".into(),
    };
    let job = Job::new(app.profile.id, JobKind::BridgeClip, to_json(&payload));
    app.bridges
        .save_bridge(&Bridge {
            id,
            project: project.id,
            owner: app.profile.id,
            cut: BridgeCut::of(&timeline, 1).unwrap(),
            from: span.from,
            to: span.to,
            first,
            last,
            provider: Provider::Higgsfield,
            model: "fake/choices".into(),
            seconds: 5,
            note: String::new(),
            job: job.id(),
            file: None,
            asked_at: SystemTime::now(),
        })
        .unwrap();
    let job = app.jobs.enqueue(job).unwrap();
    let failed = wait_done(&app, job);
    assert_eq!(failed.state(), JobState::Failed);
    assert_eq!(failed.failure().unwrap().kind, JobFailureKind::NotAllowed);
    assert!(h.clips.staged.lock().unwrap().is_empty());
    assert!(h.media.bridge_frames.lock().unwrap().is_empty());
}

#[test]
fn a_bridge_past_the_budget_asks_first_and_is_refused_where_it_cannot_fit() {
    let h = Harness::new();
    let app = h.start();
    let (_, mut editor) = opened(&h, &app);
    app.save_rate(Provider::Higgsfield, "fake/", Meter::VideoSeconds, "0.10")
        .unwrap();
    app.set_budget(Provider::Higgsfield, "0.20").unwrap();
    let error = app
        .make_bridge(&mut editor, ask(1, 3, ""), BudgetConsent::Ask)
        .unwrap_err();
    let BridgeError::OverBudget(estimate) = error else {
        panic!("expected the budget question, got {error:?}");
    };
    assert_eq!(estimate.total(), Money::from_cents(30));
    assert!(editor.view().bridges.is_empty(), "nothing asked");
    assert!(h.clips.submissions().is_empty());
    // Too long for the pictures around the cut.
    let error = app
        .make_bridge(&mut editor, ask(1, 15, ""), BudgetConsent::Confirmed)
        .unwrap_err();
    assert!(matches!(
        error,
        BridgeError::Refused(BridgeRefusal::TooLong)
    ));
    let error = app
        .make_bridge(
            &mut editor,
            ask(1, 3, &"x".repeat(BRIDGE_NOTE_MAX_CHARS + 1)),
            BudgetConsent::Confirmed,
        )
        .unwrap_err();
    assert!(matches!(error, BridgeError::NoteTooLong));
    // Confirmed, it goes.
    app.make_bridge(&mut editor, ask(1, 3, ""), BudgetConsent::Confirmed)
        .unwrap();
    assert_eq!(editor.view().bridges.len(), 1);
}

#[test]
fn a_retried_bridge_job_asks_about_what_it_has_left_to_pay() {
    let h = Harness::new();
    let app = h.start();
    let (_, mut editor) = opened(&h, &app);
    h.clips.fail("first frame to the last");
    app.make_bridge(&mut editor, ask(1, 3, ""), BudgetConsent::Ask)
        .unwrap();
    let job = editor.view().bridges[0].job;
    wait_done(&app, job);
    let failed = app.jobs().into_iter().find(|j| j.id() == job).unwrap();
    let (_, calls) = app.bridge_left(&failed).unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].purpose, CostPurpose::BridgeClip);
    assert_eq!(calls[0].video_seconds, Some(3));
}
