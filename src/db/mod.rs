use std::{collections::HashMap, time::Duration};

use bitcoin::hashes::hex::ToHex;
use nostr_sdk::Event;
use sqlx::{Executor, SqliteConnection};

use crate::{config::Config, name::Namespace, subcommands, util::Nsid};

static MIGRATIONS: [&'static str; 11] = [
    "CREATE TABLE blockchain (nsid PRIMARY KEY, blockhash, txid, vout, height);",
    "CREATE INDEX blockchain_height_dx ON blockchain(height);",
    "CREATE TABLE name_nsid (name PRIMARY KEY, nsid, root, parent, pubkey);",
    "CREATE INDEX name_nsid_nsid_idx ON name_nsid(nsid);",
    "CREATE INDEX name_nsid_parent_idx ON name_nsid(parent);",
    "CREATE TABLE create_events (nsid PRIMARY KEY, pubkey, created_at, event_id, name, children);",
    "CREATE TABLE records_events (nsid, pubkey, created_at, event_id, name, records);",
    "CREATE UNIQUE INDEX records_events_unique_idx ON records_events(nsid, pubkey)",
    "CREATE INDEX records_events_created_at_idx ON records_events(created_at);",
    "CREATE VIEW name_records_vw AS
        SELECT re.name, re.records, re.nsid FROM blockchain b
        JOIN name_nsid nn ON b.nsid = nn.root
        JOIN create_events ce ON b.nsid = ce.nsid
        JOIN records_events re on nn.nsid = re.nsid AND nn.pubkey = re.pubkey;",
    "CREATE VIEW top_level_names_vw AS
        SELECT ce.name, ce.nsid FROM blockchain b
        JOIN name_nsid nn ON b.nsid = nn.nsid
        JOIN create_events ce ON b.nsid = ce.nsid",
];

pub async fn initialize(config: &Config) -> anyhow::Result<()> {
    #[derive(sqlx::FromRow)]
    struct Usize(usize);

    let mut conn = config.sqlite().await?;

    sqlx::query("CREATE TABLE IF NOT EXISTS schema (version);")
        .execute(&mut conn)
        .await?;

    let (version,) =
        sqlx::query_as::<_, (i64,)>("SELECT COALESCE(MAX(version) + 1, 0) FROM schema")
            .fetch_one(&mut conn)
            .await?;

    for (idx, migration) in MIGRATIONS[version as usize..].into_iter().enumerate() {
        log::debug!("Migrations schema version {idx}");
        sqlx::query(migration).execute(&mut conn).await?;
        sqlx::query("INSERT INTO schema (version) VALUES (?);")
            .bind(idx as i64)
            .execute(&mut conn)
            .await?;
    }

    Ok(())
}

pub async fn insert_namespace(
    conn: &mut SqliteConnection,
    nsid: String,
    blockhash: String,
    txid: String,
    vout: usize,
    height: usize,
) -> anyhow::Result<()> {
    sqlx::query(include_str!("./queries/insert_namespace.sql"))
        .bind(nsid)
        .bind(blockhash)
        .bind(txid)
        .bind(height as i64)
        .execute(conn)
        .await?;

    Ok(())
}

pub async fn next_index_height(conn: &mut SqliteConnection) -> anyhow::Result<usize> {
    let (h,) = sqlx::query_as::<_, (i64,)>("SELECT COALESCE(MAX(height), 0) + 1 FROM blockchain;")
        .fetch_one(conn)
        .await?;
    Ok(h as usize)
}

pub async fn last_create_event_time(conn: &mut SqliteConnection) -> anyhow::Result<u64> {
    let (t,) =
        sqlx::query_as::<_, (i64,)>("SELECT COALESCE(MAX(created_at), 0) from create_events;")
            .fetch_one(conn)
            .await?;
    Ok(t as u64)
}

pub async fn insert_create_event(
    conn: &mut SqliteConnection,
    event: Event,
    ns: Namespace,
) -> anyhow::Result<()> {
    sqlx::query(include_str!("./queries/insert_create_event.sql"))
        .bind(ns.namespace_id().to_hex())
        .bind(event.pubkey.to_hex())
        .bind(event.created_at.as_i64())
        .bind(event.id.to_hex())
        .bind(&ns.0)
        .bind(serde_json::to_string(&ns.2)?)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn index_name_nsid(
    conn: &mut SqliteConnection,
    nsid: String,
    name: String,
    root: String,
    parent: Option<String>,
    pubkey: String,
) -> anyhow::Result<()> {
    sqlx::query(include_str!("./queries/index_name_nsid.sql"))
        .bind(name)
        .bind(nsid)
        .bind(root)
        .bind(parent)
        .bind(pubkey)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn last_records_time(conn: &mut SqliteConnection) -> anyhow::Result<u64> {
    let (t,) =
        sqlx::query_as::<_, (i64,)>("SELECT COALESCE(MAX(created_at), 0) FROM records_events;")
            .fetch_one(conn)
            .await?;
    Ok(t as u64)
}

pub async fn insert_records_event(
    conn: &mut SqliteConnection,
    nsid: String,
    pubkey: String,
    created_at: u64,
    event_id: String,
    name: String,
    records: String,
) -> anyhow::Result<()> {
    sqlx::query(include_str!("./queries/insert_records_event.sql"))
        .bind(nsid)
        .bind(pubkey)
        .bind(created_at as i64)
        .bind(event_id)
        .bind(name)
        .bind(records)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn nsid_for_name(
    conn: &mut SqliteConnection,
    name: String,
) -> anyhow::Result<Option<String>> {
    let s = sqlx::query_as::<_, (String,)>("SELECT nsid FROM name_nsid WHERE name = ?;")
        .bind(name)
        .fetch_optional(conn)
        .await?;
    Ok(s.map(|s| s.0))
}

pub async fn namespace_exists(conn: &mut SqliteConnection, nsid: String) -> anyhow::Result<bool> {
    let (b,) = sqlx::query_as::<_, (bool,)>("SELECT COUNT(*) FROM blockchain WHERE nsid = ?;")
        .bind(nsid)
        .fetch_one(conn)
        .await?;
    Ok(b)
}

pub async fn name_records(
    conn: &mut SqliteConnection,
    name: String,
) -> anyhow::Result<Option<HashMap<String, String>>> {
    let content =
        sqlx::query_as::<_, (String,)>("SELECT records from name_records_vw where name = ?;")
            .bind(name)
            .fetch_optional(conn)
            .await?;

    let records = content
        .map(|s| s.0)
        .map(|records| serde_json::from_str(&records))
        .transpose()?;
    Ok(records)
}

pub async fn top_level_names(conn: &mut SqliteConnection) -> anyhow::Result<Vec<(String, String)>> {
    Ok(
        sqlx::query_as::<_, (String, String)>("SELECT * FROM top_level_names_vw;")
            .fetch_all(conn)
            .await?,
    )
}

pub mod namespace {
    use std::collections::HashMap;

    use sqlx::SqliteConnection;

    pub struct NamespaceDetails {
        pub name: Option<String>,
        pub records: HashMap<String, String>,
        pub children: Vec<(String, String)>,
        pub blockdata: Option<(String, String, usize, usize)>,
    }

    pub async fn details(
        conn: &mut SqliteConnection,
        nsid: String,
    ) -> anyhow::Result<NamespaceDetails> {
        let name = name_for_nsid(conn, nsid.clone()).await?;

        let records = records(conn, nsid.clone()).await?;

        let blockdata = blockchain_data(conn, nsid.clone()).await?;

        let children = children(conn, nsid).await?;

        let d = NamespaceDetails {
            name,
            records: serde_json::from_str(&records.unwrap_or(String::from("{}")))?,
            children,
            blockdata,
        };
        Ok(d)
    }

    async fn children(
        conn: &mut SqliteConnection,
        nsid: String,
    ) -> Result<Vec<(String, String)>, anyhow::Error> {
        Ok(
            sqlx::query_as::<_, (String, String)>(include_str!("./queries/children.sql"))
                .bind(nsid)
                .fetch_all(conn)
                .await?,
        )
    }

    async fn records(
        conn: &mut SqliteConnection,
        nsid: String,
    ) -> Result<Option<String>, anyhow::Error> {
        let records = sqlx::query_as::<_, (String,)>(include_str!("./queries/records.sql"))
            .bind(nsid)
            .fetch_optional(conn)
            .await?;
        Ok(records.map(|s| s.0))
    }

    async fn name_for_nsid(
        conn: &mut SqliteConnection,
        nsid: String,
    ) -> anyhow::Result<Option<String>> {
        let name =
            sqlx::query_as::<_, (String,)>("SELECT name FROM name_nsid WHERE nsid = ? LIMIT 1;")
                .bind(nsid)
                .fetch_optional(conn)
                .await?;
        Ok(name.map(|n| n.0))
    }

    async fn blockchain_data(
        conn: &mut SqliteConnection,
        nsid: String,
    ) -> anyhow::Result<Option<(String, String, usize, usize)>> {
        let bd = sqlx::query_as::<_, (String, String, i64, i64)>(include_str!(
            "./queries/blockchain_data.sql"
        ))
        .fetch_optional(conn)
        .await?
        .map(|s| (s.0, s.1, s.2 as usize, s.3 as usize));
        Ok(bd)
    }
}
