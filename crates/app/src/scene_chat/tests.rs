use bardo_domain::{CostRepository, JobState};

use super::*;
use crate::scenes::tests::{Harness, done, plan_answer, wait_done};

const WHITE_CUP: &str = "Make the cup white";

/// Claude's answer proposing to edit the image: a white cup. A second
/// edit, a cream cup, is one way too many and is left out.
fn proposal_answer() -> String {
    serde_json::json!({
        "reply": "I'll make the cup on the console white and keep the rest.",
        "kind": "proposal",
        "options": [
            {
                "kind": "edit_image",
                "instruction": "Make the cup on the console white. Keep everything else the same.",
                "prompt": "A control room after the signal is lost, a white cup on the console",
            },
            {
                "kind": "edit_image",
                "instruction": "Make the cup on the console cream. Keep everything else the same.",
                "prompt": "A control room after the signal is lost, a cream cup on the console",
            },
        ],
        "recommended": 1,
    })
    .to_string()
}

fn question_answer() -> String {
    serde_json::json!({
        "reply": "Which cup: the one on the console or the one in his hand?",
        "kind": "question",
        "options": [],
        "recommended": 0,
    })
    .to_string()
}

/// Sends `text` about scene 2 (the lost signal), answered with `answer`,
/// and waits for the reply.
fn ask(h: &Harness, app: &Bardo, project: VideoProjectId, text: &str, answer: String) -> JobId {
    h.answer(answer);
    let job = app
        .send_scene_chat(project, 2, text, BudgetConsent::Ask)
        .unwrap();
    done(app, job);
    job
}

fn last_reply(app: &Bardo, project: VideoProjectId) -> SceneChatMessage {
    app.scene_chat(project, 2)
        .unwrap()
        .lines
        .into_iter()
        .rev()
        .find(|line| !line.message.is_request())
        .unwrap()
        .message
}

fn applied(app: &Bardo, project: VideoProjectId) -> Option<AppliedState> {
    app.scene_chat(project, 2)
        .unwrap()
        .lines
        .into_iter()
        .rev()
        .find(|line| !line.message.is_request())
        .unwrap()
        .applied
}

#[test]
fn a_request_pays_only_for_claudes_reply_which_sees_the_scene_and_its_image() {
    let h = Harness::new();
    let app = h.start();
    let (project, plan) = h.drawn_project(&app);
    let drawn = h.images.prompts().len();
    let asked = h.text.requests().len();

    let view = app.scene_chat(project.id, 2).unwrap();
    assert_eq!(view.availability, ChatAvailability::Open);
    assert!(view.lines.is_empty());

    let job = ask(&h, &app, project.id, WHITE_CUP, proposal_answer());

    // One Claude call, with the scene's image, and no picture drawn.
    let requests = h.text.requests();
    assert_eq!(requests.len(), asked + 1);
    let request = requests.last().unwrap();
    assert_eq!(request.images.len(), 1, "the scene's image");
    assert!(request.prompt.contains(WHITE_CUP));
    assert!(
        request
            .prompt
            .contains("A control room after the signal is lost")
    );
    assert!(request.prompt.contains("Scene 3 of 3"));
    assert!(matches!(request.format, TextFormat::Json { .. }));
    assert_eq!(h.images.prompts().len(), drawn, "nothing drawn yet");
    let paid =
        h.db.recent_costs(app.profile().id, CostPurpose::SceneChat, 10)
            .unwrap();
    assert_eq!(paid.len(), 1);
    assert_eq!(paid[0].job, Some(job));

    let view = app.scene_chat(project.id, 2).unwrap();
    assert_eq!(view.lines.len(), 2);
    assert!(view.lines[0].message.is_request());
    assert_eq!(view.lines[0].message.text, WHITE_CUP);
    let reply = &view.lines[1];
    let proposal = reply.message.proposal().unwrap();
    assert_eq!(proposal.options().len(), 1, "one option per way");
    assert!(proposal.options()[0].instruction.contains("white"));
    assert_eq!(proposal.recommended(), 0);
    assert_eq!(
        proposal.image(),
        plan.scenes()[2].image().unwrap().generation.id
    );
    assert!(reply.applies);
    assert_eq!(reply.applied, None);
    assert!(!view.is_answering());
    assert_eq!(view.failure(), None);
    // The other scenes keep their own chats.
    assert!(app.scene_chat(project.id, 1).unwrap().lines.is_empty());
}

#[test]
fn applying_an_option_edits_the_current_image_which_waits_for_review() {
    let h = Harness::new();
    let app = h.start();
    let (project, plan) = h.drawn_project(&app);
    let current = plan.scenes()[2].image().unwrap().clone();
    ask(&h, &app, project.id, WHITE_CUP, proposal_answer());
    let reply = last_reply(&app, project.id);

    let job = app
        .apply_scene_chat_option(project.id, reply.id, 0, BudgetConsent::Ask)
        .unwrap();
    done(&app, job);

    let request = h.images.requests.lock().unwrap().last().unwrap().clone();
    let bytes = h.files.read(project.id, &current.file).unwrap();
    assert_eq!(
        request.references.last().unwrap().bytes,
        bytes,
        "the current image goes last"
    );
    assert_eq!(request.references.len(), 1, "no other shots");
    assert!(
        request
            .prompt
            .contains("Make the cup on the console white.")
    );
    assert!(
        request
            .prompt
            .contains(&format!("Reference {}", request.references.len()))
    );
    let scene = app.scenes(project.id).unwrap().plan.unwrap().scenes()[2].clone();
    assert_eq!(scene.image(), Some(&current), "kept until reviewed");
    assert!(scene.pending().is_some());
    assert_eq!(applied(&app, project.id), Some(AppliedState::Review));
    assert!(
        !app.scene_chat(project.id, 2).unwrap().lines[1].applies,
        "an option applied waits for its review"
    );
}

#[test]
fn using_the_edited_image_makes_its_prompt_the_scenes() {
    let h = Harness::new();
    let app = h.start();
    let (project, _) = h.drawn_project(&app);
    ask(&h, &app, project.id, WHITE_CUP, proposal_answer());
    let reply = last_reply(&app, project.id);
    done(
        &app,
        app.apply_scene_chat_option(project.id, reply.id, 0, BudgetConsent::Ask)
            .unwrap(),
    );

    let plan = app.accept_scene_image(project.id, 2).unwrap();

    assert_eq!(
        plan.scenes()[2].prompt().as_str(),
        "A control room after the signal is lost, a white cup on the console"
    );
    assert_eq!(applied(&app, project.id), Some(AppliedState::Used));
    assert!(matches!(
        app.apply_scene_chat_option(project.id, reply.id, 0, BudgetConsent::Ask),
        Err(SceneChatError::Apply(ChatApplyError::AlreadyUsed))
    ));
}

#[test]
fn keeping_the_old_image_leaves_the_prompt_and_the_options_open() {
    let h = Harness::new();
    let app = h.start();
    let (project, plan) = h.drawn_project(&app);
    let prompt = plan.scenes()[2].prompt().clone();
    ask(&h, &app, project.id, WHITE_CUP, proposal_answer());
    let reply = last_reply(&app, project.id);
    done(
        &app,
        app.apply_scene_chat_option(project.id, reply.id, 0, BudgetConsent::Ask)
            .unwrap(),
    );

    let plan = app.reject_scene_image(project.id, 2).unwrap();

    assert_eq!(plan.scenes()[2].prompt(), &prompt);
    assert_eq!(applied(&app, project.id), Some(AppliedState::Kept));
    assert!(app.scene_chat(project.id, 2).unwrap().lines[1].applies);
    done(
        &app,
        app.apply_scene_chat_option(project.id, reply.id, 0, BudgetConsent::Ask)
            .unwrap(),
    );
    assert_eq!(applied(&app, project.id), Some(AppliedState::Review));
}

#[test]
fn an_option_proposed_for_an_older_image_no_longer_applies() {
    let h = Harness::new();
    let app = h.start();
    let (project, _) = h.drawn_project(&app);
    ask(&h, &app, project.id, WHITE_CUP, proposal_answer());
    let reply = last_reply(&app, project.id);
    done(
        &app,
        app.regenerate_scene_image(project.id, 2, BudgetConsent::Ask)
            .unwrap(),
    );
    app.accept_scene_image(project.id, 2).unwrap();
    let drawn = h.images.prompts().len();

    assert!(!app.scene_chat(project.id, 2).unwrap().lines[1].applies);
    assert!(matches!(
        app.apply_scene_chat_option(project.id, reply.id, 0, BudgetConsent::Ask),
        Err(SceneChatError::Apply(ChatApplyError::OutOfDate))
    ));
    assert_eq!(h.images.prompts().len(), drawn);
}

#[test]
fn a_question_proposes_nothing_to_apply() {
    let h = Harness::new();
    let app = h.start();
    let (project, _) = h.drawn_project(&app);
    ask(&h, &app, project.id, "Change the cup", question_answer());
    let reply = last_reply(&app, project.id);

    assert!(reply.proposal().is_none());
    assert!(!app.scene_chat(project.id, 2).unwrap().lines[1].applies);
    assert!(matches!(
        app.apply_scene_chat_option(project.id, reply.id, 0, BudgetConsent::Ask),
        Err(SceneChatError::Apply(ChatApplyError::NotAProposal))
    ));

    // The next request sends the conversation so far.
    ask(
        &h,
        &app,
        project.id,
        "The one on the console",
        proposal_answer(),
    );
    let prompt = h.text.requests().last().unwrap().prompt.clone();
    assert!(prompt.contains("User: Change the cup"));
    assert!(prompt.contains("You: Which cup"));
}

#[test]
fn a_scene_without_an_image_takes_no_request() {
    let h = Harness::new();
    let app = h.start();
    let (project, _) = h.planned_project(&app);
    let asked = h.text.requests().len();

    assert_eq!(
        app.scene_chat(project.id, 2).unwrap().availability,
        ChatAvailability::NoImage
    );
    assert!(matches!(
        app.send_scene_chat(project.id, 2, WHITE_CUP, BudgetConsent::Ask),
        Err(SceneChatError::NoImage)
    ));
    assert_eq!(h.text.requests().len(), asked);
}

#[test]
fn an_empty_or_long_request_is_refused_before_anything_is_paid() {
    let h = Harness::new();
    let app = h.start();
    let (project, _) = h.drawn_project(&app);
    let asked = h.text.requests().len();

    let empty = app
        .send_scene_chat(project.id, 2, "   ", BudgetConsent::Ask)
        .unwrap_err();
    assert_eq!(empty.message(), Text::SceneChatEmpty);
    let long = "a".repeat(bardo_domain::MAX_CHAT_REQUEST_CHARS + 1);
    let long = app
        .send_scene_chat(project.id, 2, &long, BudgetConsent::Ask)
        .unwrap_err();
    assert_eq!(long.message(), Text::SceneChatTooLong);
    assert_eq!(h.text.requests().len(), asked);
    assert!(app.scene_chat(project.id, 2).unwrap().lines.is_empty());
}

#[test]
fn a_reply_on_its_way_holds_the_next_request_and_one_that_arrives_as_it_is_cancelled_is_shown() {
    use crate::testing::arriving;

    let h = Harness::new();
    let app = h.start();
    let (project, _) = h.drawn_project(&app);
    let asked = h.text.requests().len();
    h.answer(proposal_answer());
    let id = arriving::cancelled(&app, &h.text, || {
        app.send_scene_chat(project.id, 2, WHITE_CUP, BudgetConsent::Ask)
            .unwrap()
    });

    assert!(matches!(
        app.send_scene_chat(project.id, 2, "And the walls", BudgetConsent::Ask),
        Err(SceneChatError::Busy)
    ));
    // Another scene's chat is free.
    assert!(app.scene_chat(project.id, 1).unwrap().job.is_none());
    arriving::retry_refused(&app, id);

    arriving::lands(&app, &h.text, id);
    let view = app.scene_chat(project.id, 2).unwrap();
    assert_eq!(view.lines.len(), 2, "the paid reply is shown");
    assert!(view.lines[1].message.proposal().is_some());

    // Run again, the job finds its reply: Claude is not asked twice.
    assert_eq!(arriving::retried(&app, id).state(), JobState::Done);
    assert_eq!(h.text.requests().len(), asked + 1);
    assert_eq!(app.scene_chat(project.id, 2).unwrap().lines.len(), 2);
    let paid =
        h.db.recent_costs(app.profile().id, CostPurpose::SceneChat, 10)
            .unwrap();
    assert_eq!(paid.len(), 1);
}

#[test]
fn an_unusable_answer_fails_the_reply_and_a_retry_asks_again() {
    let h = Harness::new();
    let app = h.start();
    let (project, _) = h.drawn_project(&app);
    h.answer("not json".to_owned());
    let job = app
        .send_scene_chat(project.id, 2, WHITE_CUP, BudgetConsent::Ask)
        .unwrap();
    let failed = wait_done(&app, job);
    assert_eq!(failed.state(), JobState::Failed);

    let view = app.scene_chat(project.id, 2).unwrap();
    assert_eq!(view.lines.len(), 1, "only the request");
    assert_eq!(
        view.failure().map(|failure| failure.kind),
        Some(JobFailureKind::UnexpectedAnswer)
    );

    let asked = h.text.requests().len();
    h.answer(proposal_answer());
    app.retry_job(job).unwrap();
    done(&app, job);
    assert_eq!(h.text.requests().len(), asked + 1);
    let view = app.scene_chat(project.id, 2).unwrap();
    assert_eq!(view.lines.len(), 2);
    assert_eq!(view.failure(), None);
}

#[test]
fn planning_the_scenes_again_takes_their_chats_away() {
    let h = Harness::new();
    let app = h.start();
    let (project, _) = h.drawn_project(&app);
    ask(&h, &app, project.id, WHITE_CUP, proposal_answer());
    let reply = last_reply(&app, project.id);

    h.answer(plan_answer());
    done(
        &app,
        app.plan_scenes(project.id, true, BudgetConsent::Ask)
            .unwrap(),
    );

    assert!(app.scene_chat(project.id, 2).unwrap().lines.is_empty());
    assert!(matches!(
        app.apply_scene_chat_option(project.id, reply.id, 0, BudgetConsent::Ask),
        Err(SceneChatError::MessageNotFound)
    ));
}

#[test]
fn the_reply_is_written_in_the_apps_language() {
    let h = Harness::new();
    let mut app = h.start();
    let (project, _) = h.drawn_project(&app);
    app.set_ui_language(bardo_domain::UiLanguage::PtBr).unwrap();
    ask(&h, &app, project.id, WHITE_CUP, proposal_answer());
    let request = h.text.requests().last().unwrap().clone();
    let all = format!("{}\n{}", request.instructions, request.prompt);
    assert!(all.contains("Brazilian Portuguese"), "{all}");
}

#[test]
fn an_option_is_not_applied_over_a_new_image_waiting_for_review() {
    let h = Harness::new();
    let app = h.start();
    let (project, _) = h.drawn_project(&app);
    ask(&h, &app, project.id, WHITE_CUP, proposal_answer());
    let reply = last_reply(&app, project.id);
    done(
        &app,
        app.regenerate_scene_image(project.id, 2, BudgetConsent::Ask)
            .unwrap(),
    );
    let drawn = h.images.prompts().len();

    assert!(!app.scene_chat(project.id, 2).unwrap().lines[1].applies);
    let refused = app
        .apply_scene_chat_option(project.id, reply.id, 0, BudgetConsent::Ask)
        .unwrap_err();
    assert!(matches!(refused, SceneChatError::Pending), "{refused:?}");
    assert_eq!(refused.message(), Text::SceneChatPending);
    assert_eq!(h.images.prompts().len(), drawn, "the waiting image stays");

    // Keeping the current image lets the option apply again.
    app.reject_scene_image(project.id, 2).unwrap();
    assert!(app.scene_chat(project.id, 2).unwrap().lines[1].applies);
}

#[test]
fn a_prompt_the_user_edited_after_the_proposal_is_kept_when_the_edit_is_used() {
    let h = Harness::new();
    let app = h.start();
    let (project, _) = h.drawn_project(&app);
    ask(&h, &app, project.id, WHITE_CUP, proposal_answer());
    let reply = last_reply(&app, project.id);
    let mine = "A control room at night, my own words";
    app.edit_scene_prompt(project.id, 2, mine).unwrap();
    done(
        &app,
        app.apply_scene_chat_option(project.id, reply.id, 0, BudgetConsent::Ask)
            .unwrap(),
    );

    let plan = app.accept_scene_image(project.id, 2).unwrap();

    assert_eq!(plan.scenes()[2].prompt().as_str(), mine);
    assert_eq!(applied(&app, project.id), Some(AppliedState::Used));
}
