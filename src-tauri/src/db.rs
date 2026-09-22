use rusqlite::Connection;
use std::path::Path;

pub fn open(p: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(p)?;
    c.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         CREATE TABLE IF NOT EXISTS accounts(
           email TEXT PRIMARY KEY,
           history_id TEXT,
           last_sync INTEGER,
           needs_auth INTEGER DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS messages(
           id TEXT PRIMARY KEY,
           account TEXT NOT NULL,
           thread_id TEXT,
           subject TEXT,
           sender TEXT,
           snippet TEXT,
           date INTEGER,
           labels TEXT,
           list_unsub TEXT,
           list_unsub_post TEXT,
           category TEXT,
           confidence REAL,
           reason TEXT,
           summary TEXT,
           unread INTEGER DEFAULT 1,
           state TEXT DEFAULT 'inbox'
         );
         CREATE INDEX IF NOT EXISTS msg_acct_date ON messages(account, date DESC);
         CREATE TABLE IF NOT EXISTS proposals(
           id INTEGER PRIMARY KEY,
           msg_id TEXT NOT NULL,
           account TEXT NOT NULL,
           action TEXT NOT NULL,
           category TEXT,
           status TEXT DEFAULT 'pending',
           created INTEGER
         );
         CREATE TABLE IF NOT EXISTS actions(
           id INTEGER PRIMARY KEY,
           msg_id TEXT,
           account TEXT,
           action TEXT NOT NULL,
           detail TEXT,
           result TEXT,
           undone INTEGER DEFAULT 0,
           created INTEGER
         );
         CREATE TABLE IF NOT EXISTS labels(
           account TEXT,
           name TEXT,
           id TEXT,
           PRIMARY KEY(account, name)
         );
         CREATE TABLE IF NOT EXISTS digests(
           id INTEGER PRIMARY KEY,
           created INTEGER,
           hours INTEGER,
           json TEXT
         );
         CREATE TABLE IF NOT EXISTS approvals(
           account TEXT,
           category TEXT,
           n INTEGER DEFAULT 0,
           PRIMARY KEY(account, category)
         );",
    )?;
    let _ = c.execute_batch("ALTER TABLE messages ADD COLUMN body TEXT");
    let _ = c.execute_batch("ALTER TABLE accounts ADD COLUMN inbox_total INTEGER");
    Ok(c)
}
