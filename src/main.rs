use chrono::prelude::*;
use ghost_actor::dependencies::futures::FutureExt;
use rand::Rng;
use rusqlite::*;
use std::fmt::Write;

/// Simple error type.
#[derive(Debug)]
pub struct Error(Box<dyn std::error::Error + Send + Sync>);

macro_rules! qf {
    ($($t:ty)*) => {$(
        impl From<$t> for Error {
            fn from(e: $t) -> Self {
                Self(Box::new(e))
            }
        }
    )*};
}
qf!( rusqlite::Error ghost_actor::GhostError );

/// Result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Encryption key for sqlcipher.
struct Key(pub Vec<u8>);

impl ToSql for Key {
    fn to_sql(&self) -> rusqlite::Result<types::ToSqlOutput<'_>> {
        let mut s = String::new();
        for b in &self.0 {
            write!(&mut s, "{:02X}", b)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        }
        Ok(types::ToSqlOutput::Owned(types::Value::Text(s)))
    }
}

/// Simulate getting an encryption key from Lair.
fn get_encryption_key_shim() -> Key {
    Key(vec![0; 32])
}

/// Demo entry type for database.
#[derive(Debug)]
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

ghost_actor::ghost_chan! {
    /// Our database api.
    pub chan DbApi<Error> {
        /// Insert an entry into the database.
        fn insert(entry: Entry) -> ();

        /// Query a range of entries from the database.
        fn query(
            dht_loc_start: u32,
            dht_loc_end: u32,
            created_at_start: DateTime<Utc>,
            created_at_end: DateTime<Utc>,
        ) -> Vec<Entry>;
    }
}

/// Spawn a database actor.
pub async fn spawn_database<P: AsRef<std::path::Path>>(
    path: P,
) -> Result<ghost_actor::GhostSender<DbApi>> {
    let builder = ghost_actor::actor_builder::GhostActorBuilder::new();

    let sender = builder.channel_factory().create_channel::<DbApi>().await?;

    tokio::task::spawn(builder.spawn(Db::new(path).await?));

    Ok(sender)
}

/// Internal DbApi ghost_actor implementation.
struct Db {
    con: Connection,
}

impl Db {
    pub async fn new<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let con = Connection::open(path)?;

        // set up encryption
        con.pragma_update(None, "key", &get_encryption_key_shim())?;

        // set to faster write-ahead-log mode
        con.pragma_update(None, "journal_mode", &"WAL".to_string())?;

        // create entries table
        con.execute(
            "
            CREATE TABLE IF NOT EXISTS entries (
                hash            BLOB PRIMARY KEY,
                dht_loc         INT NOT NULL,
                created_at      TEXT NOT NULL
            );",
            NO_PARAMS,
        )?;

        // create dht_loc + created_at index
        // we can have as many indexes as we want
        // i.e. we could have separate dht_loc only index
        // if we want queries that don't care about created_at, etc.
        con.execute(
            "
            CREATE INDEX IF NOT EXISTS entries_query_idx ON entries (
                dht_loc, created_at
            );",
            NO_PARAMS,
        )?;

        Ok(Self { con })
    }
}

impl ghost_actor::GhostControlHandler for Db {}

impl ghost_actor::GhostHandler<DbApi> for Db {}

impl DbApiHandler for Db {
    fn handle_insert(&mut self, entry: Entry) -> DbApiHandlerResult<()> {
        // best practices to use transactions
        // (although we're only executing a single statement here
        //  so strictly not needed)
        let tx = self.con.transaction()?;

        {
            // rusqlite keeps a cache of prepared statements
            // speeds things up a bit.
            let mut ins = tx.prepare_cached(
                "INSERT INTO entries (hash, dht_loc, created_at) VALUES (?1, ?2, ?3)",
            )?;

            // execute the insert
            ins.execute(params![entry.hash, entry.dht_loc, entry.created_at])?;
        }

        // commit the transaction
        tx.commit()?;

        Ok(async move { Ok(()) }.boxed().into())
    }

    fn handle_query(
        &mut self,
        dht_loc_start: u32,
        dht_loc_end: u32,
        created_at_start: DateTime<Utc>,
        created_at_end: DateTime<Utc>,
    ) -> DbApiHandlerResult<Vec<Entry>> {
        // this transaction is needed even less since we're not writing,
        // but if we were reading from multiple tables would keep them
        // consistent.
        let tx = self.con.transaction()?;

        let mut out = Vec::new();

        {
            // Really we'd want to use dht_arc with the start / half-length,
            // then branch on potential for a wrapping space that inverts
            // the `> <` signs below - but this is just a PoC.
            let mut query = tx.prepare_cached(
                "SELECT hash, dht_loc, created_at FROM entries
                    WHERE dht_loc >= ?1
                    AND dht_loc <= ?2
                    AND created_at >= ?3
                    AND created_at <= ?4;",
            )?;

            // map our results to the rust struct
            for r in query.query_map(
                params![dht_loc_start, dht_loc_end, created_at_start, created_at_end,],
                |row| {
                    Ok(Entry {
                        hash: row.get(0)?,
                        dht_loc: row.get(1)?,
                        created_at: row.get(2)?,
                    })
                },
            )? {
                out.push(r?);
            }
        }

        // we're not writing, we can rollback this transaction
        // essentially a no-op
        tx.rollback()?;

        Ok(async move { Ok(out) }.boxed().into())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // spawn the database actor
    let con = spawn_database("./DATABASE.SQLITE").await?;

    // insert a random entry
    con.insert(Entry::rand()).await?;

    // build up and execute a query
    let start = Utc.ymd(1970, 1, 1).and_hms(0, 1, 1);
    let end = Utc::now();
    let res = con.query(0, u32::MAX, start, end).await?;
    println!("{:#?}", res);

    Ok(())
}
