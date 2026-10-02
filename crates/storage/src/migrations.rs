use rusqlite::Connection;

/// Ordered schema migrations. Append only: never edit a released migration.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_user_profile.sql"),
    include_str!("../migrations/0002_channel.sql"),
    include_str!("../migrations/0003_job.sql"),
    include_str!("../migrations/0004_niche_research.sql"),
    include_str!("../migrations/0005_theme.sql"),
    include_str!("../migrations/0006_script.sql"),
];

/// Applies every migration newer than the database's `user_version`, each in
/// its own transaction.
pub(crate) fn run(conn: &mut Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let applied = usize::try_from(current).unwrap_or(0);
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(applied) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", index as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_version(conn: &Connection) -> usize {
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        usize::try_from(version).unwrap()
    }

    #[test]
    fn migrates_a_new_database_to_the_latest_version() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        assert_eq!(user_version(&conn), MIGRATIONS.len());
    }

    #[test]
    fn upgrading_keeps_existing_rows() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute(
            "INSERT INTO user_profile (id, ui_language) VALUES ('p', 'pt-BR')",
            [],
        )
        .unwrap();

        run(&mut conn).unwrap();

        assert_eq!(user_version(&conn), MIGRATIONS.len());
        let language: String = conn
            .query_row("SELECT ui_language FROM user_profile", [], |row| row.get(0))
            .unwrap();
        assert_eq!(language, "pt-BR");
    }

    #[test]
    fn running_twice_is_a_no_op() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        run(&mut conn).unwrap();
        assert_eq!(user_version(&conn), MIGRATIONS.len());
    }
}
