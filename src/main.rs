use chrono::prelude::*;
use futures::StreamExt;
use rand::Rng;
use sqlx::*;

/// Simulate getting an encryption key from Lair.
fn get_encryption_key_shim() -> [u8; 32] {
    [
        26, 111, 7, 31, 52, 204, 156, 103, 203, 171, 156, 89, 98, 51, 158, 143, 57, 134, 93, 56,
        199, 225, 53, 141, 39, 77, 145, 130, 136, 108, 96, 201,
    ]
}

/// Demo entry type for database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Entry {
    hash: Vec<u8>,
    dht_loc: u32,
    created_at: DateTime<Utc>,
}

impl Entry {
    /// Generate a random entry
    pub fn rand() -> Self {
        let mut hash = vec![0; 4];
        rand::thread_rng().fill(&mut hash[..]);

        Self {
            hash,
            dht_loc: rand::thread_rng().gen(),
            created_at: Utc::now(),
        }
    }
}

async fn make_connection<P: AsRef<std::path::Path>>(path: P) -> anyhow::Result<SqliteConnection> {
    let mut con = SqliteConnection::connect(&path.as_ref().to_string_lossy()).await?;

    let key = get_encryption_key_shim();
    let mut cmd =
        *br#"PRAGMA key = "x'0000000000000000000000000000000000000000000000000000000000000000'";"#;
    {
        use std::io::Write;
        let mut c = std::io::Cursor::new(&mut cmd[16..80]);
        for b in &key {
            write!(c, "{:02X}", b)?;
        }
    }
    con.execute(std::str::from_utf8(&cmd).unwrap()).await?;

    // set to faster write-ahead-log mode
    // con.pragma_update(None, "journal_mode", &"WAL".to_string())?;

    // create entries table
    con.execute(
        "CREATE TABLE IF NOT EXISTS entries (
                hash            BLOB PRIMARY KEY,
                dht_loc         INT NOT NULL,
                created_at      TEXT NOT NULL
            );",
        // NO_PARAMS,
    )
    .await?;

    // create dht_loc + created_at index
    // we can have as many indexes as we want
    // i.e. we could have separate dht_loc only index
    // if we want queries that don't care about created_at, etc.
    con.execute(
        "CREATE INDEX IF NOT EXISTS entries_query_idx ON entries (
                dht_loc, created_at
            );",
        // NO_PARAMS,
    )
    .await?;
    Ok(con)
}

// fn handle_query(
//     &mut self,
//     dht_loc_start: u32,
//     dht_loc_end: u32,
//     created_at_start: DateTime<Utc>,
//     created_at_end: DateTime<Utc>,
// ) -> DbApiHandlerResult<Vec<Entry>> {
//     // this transaction is needed even less since we're not writing,
//     // but if we were reading from multiple tables would keep them
//     // consistent.
//     let tx = self.con.transaction()?;

//     let mut out = Vec::new();

//     {
//         // Really we'd want to use dht_arc with the start / half-length,
//         // then branch on potential for a wrapping space that inverts
//         // the `> <` signs below - but this is just a PoC.
//         let mut query = tx.prepare_cached(
//             "SELECT hash, dht_loc, created_at FROM entries
// WHERE dht_loc >= ?1
// AND dht_loc <= ?2
// AND created_at >= ?3
// AND created_at <= ?4;",
//         )?;

//         // map our results to the rust struct
//         for r in query.query_map(
//             params![dht_loc_start, dht_loc_end, created_at_start, created_at_end,],
//             |row| {
//                 Ok(Entry {
//                     hash: row.get(0)?,
//                     dht_loc: row.get(1)?,
//                     created_at: row.get(2)?,
//                 })
//             },
//         )? {
//             out.push(r?);
//         }
//     }

//     // we're not writing, we can rollback this transaction
//     // essentially a no-op
//     tx.rollback()?;

//     Ok(async move { Ok(out) }.boxed().into())
// }

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // spawn the database actor
    let mut con = make_connection("sqlite::memory:").await?;

    let entry = Entry::rand();
    let entry_hash = entry.hash.clone();

    con.transaction(|tx| {
        Box::pin(async {
            sqlx::query("INSERT INTO entries (hash, dht_loc, created_at) VALUES (?1, ?2, ?3)")
                .bind(entry.hash)
                .bind(entry.dht_loc)
                .bind(entry.created_at)
                .execute(tx)
                .await
        })
    })
    .await?;

    let fetched: Vec<Entry> = con
        .transaction(|tx| {
            Box::pin(async {
                let start = Utc.ymd(1970, 1, 1).and_hms(0, 1, 1);
                let end = Utc::now();
                sqlx::query_as::<_, Entry>(
                    "SELECT hash, dht_loc, created_at FROM entries
            WHERE dht_loc >= ?1
            AND dht_loc <= ?2
            AND created_at >= ?3
            AND created_at <= ?4
            ;",
                )
                .bind(0)
                .bind(u32::MAX)
                .bind(start)
                .bind(end)
                .fetch(tx)
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .collect::<Result<Vec<_>>>()
            })
        })
        .await?;

    // let res = con.query(0, u32::MAX, start, end).await?;
    println!("{:#?}", fetched);

    Ok(())
}
