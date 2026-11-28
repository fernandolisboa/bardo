//! Scene chats (#368, ADR-0049): stored per scene plan and position, and
//! removed with the plan.

use bardo_domain::{
    ChatEntry, ChatTarget, JobId, RepositoryError, SceneChatMessage, SceneChatMessageId,
    SceneChatRepository, ScenePlanId,
};
use rusqlite::{Row, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

#[derive(Debug, thiserror::Error)]
#[error("stored scene chat message is invalid: {0}")]
struct InvalidRow(String);

fn invalid(detail: impl Into<String>) -> RepositoryError {
    boxed(InvalidRow(detail.into()))
}

fn uuid(text: &str) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(text).map_err(boxed)
}

const COLUMNS: &str = "id, plan_id, position, author, target, text, reply, job_id, at";

type RawRow = (
    String,
    String,
    i64,
    String,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    i64,
);

fn raw(row: &Row<'_>) -> rusqlite::Result<RawRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    ))
}

fn message(row: RawRow) -> Result<SceneChatMessage, RepositoryError> {
    let (id, plan, position, author, target, text, reply, job, at) = row;
    let entry = match (author.as_str(), target, reply) {
        ("request", Some(target), None) => ChatEntry::Request {
            target: ChatTarget::from_code(&target)
                .ok_or_else(|| invalid(format!("unknown target {target}")))?,
        },
        ("reply", None, Some(reply)) => SceneChatMessage::entry_from_json(&reply)
            .ok_or_else(|| invalid("a reply that does not read"))?,
        _ => return Err(invalid(format!("unknown author {author}"))),
    };
    Ok(SceneChatMessage {
        id: SceneChatMessageId::from(uuid(&id)?),
        plan: ScenePlanId::from(uuid(&plan)?),
        scene: usize::try_from(position).map_err(boxed)?,
        text,
        entry,
        job: job.as_deref().map(uuid).transpose()?.map(JobId::from),
        at: from_unix_millis(at),
    })
}

impl SceneChatRepository for Database {
    fn scene_chat(
        &self,
        plan: ScenePlanId,
        scene: usize,
    ) -> Result<Vec<SceneChatMessage>, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM scene_chat_message
                 WHERE plan_id = ?1 AND position = ?2
                 ORDER BY at, rowid"
            ))
            .map_err(boxed)?;
        let rows = statement
            .query_map(
                params![plan.to_string(), i64::try_from(scene).map_err(boxed)?],
                raw,
            )
            .map_err(boxed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(boxed)?;
        rows.into_iter().map(message).collect()
    }

    fn chat_message(
        &self,
        id: SceneChatMessageId,
    ) -> Result<Option<SceneChatMessage>, RepositoryError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM scene_chat_message WHERE id = ?1"),
                [id.to_string()],
                raw,
            )
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                error => Err(error),
            })
            .map_err(boxed)?;
        row.map(message).transpose()
    }

    fn save_chat_message(&self, message: &SceneChatMessage) -> Result<(), RepositoryError> {
        let (author, target, reply) = match &message.entry {
            ChatEntry::Request { target } => ("request", Some(target.code()), None),
            ChatEntry::Reply { .. } => ("reply", None, message.reply_json()),
        };
        let conn = self.conn();
        // A plan that is gone keeps no chat: the insert below would fail
        // on its foreign key, so it is left out instead.
        conn.execute(
            &format!(
                "INSERT INTO scene_chat_message ({COLUMNS})
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9
                 WHERE EXISTS (SELECT 1 FROM scene_plan WHERE id = ?2)
                 ON CONFLICT (id) DO UPDATE SET
                     text = excluded.text,
                     reply = excluded.reply,
                     job_id = excluded.job_id"
            ),
            params![
                message.id.to_string(),
                message.plan.to_string(),
                i64::try_from(message.scene).map_err(boxed)?,
                author,
                target,
                message.text,
                reply,
                message.job.map(|job| job.to_string()),
                to_unix_millis(message.at),
            ],
        )
        .map_err(boxed)?;
        Ok(())
    }
}
