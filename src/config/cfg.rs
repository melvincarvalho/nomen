use std::path::PathBuf;

use anyhow::anyhow;
use bitcoin::{FeeRate, Network};
use nostr_sdk::{
    prelude::{FromSkStr, ToBech32},
    Options,
};
use sqlx::{sqlite, SqlitePool};

use super::{
    Cli, ConfigFile, NameNewSubcommand, NameTransferSubcommand, ServerSubcommand, Subcommand,
};

#[derive(Clone, Debug)]
pub struct Config {
    pub cli: Cli,
    pub file: ConfigFile,
}

impl Config {
    pub fn new(cli: Cli, file: ConfigFile) -> Self {
        Self { cli, file }
    }

    pub fn rpc_auth(&self) -> bitcoincore_rpc::Auth {
        if let Some(cookie) = &self.rpc_cookie() {
            bitcoincore_rpc::Auth::CookieFile(cookie.clone())
        } else if self.rpc_user().is_some() || self.rpc_password().is_some() {
            bitcoincore_rpc::Auth::UserPass(
                self.rpc_user().expect("RPC user not configured"),
                self.rpc_password().expect("RPC password not configured"),
            )
        } else {
            bitcoincore_rpc::Auth::None
        }
    }

    pub fn rpc_client(&self) -> anyhow::Result<bitcoincore_rpc::Client> {
        let host = self.rpc_host();
        let port = self.rpc_port();
        let url = format!("{host}:{port}");
        let auth = self.rpc_auth();
        Ok(bitcoincore_rpc::Client::new(&url, auth)?)
    }

    pub async fn sqlite(&self) -> anyhow::Result<sqlite::SqlitePool> {
        let db = self.data();

        // SQLx doesn't seem to like it if a db file does not already exist, so let's create an empty one
        if !tokio::fs::try_exists(&db).await? {
            tokio::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .open(&db)
                .await?;
        }

        Ok(SqlitePool::connect(&format!("sqlite:{}", db.to_string_lossy())).await?)
    }

    pub async fn nostr_client(
        &self,
        sk: &str,
    ) -> anyhow::Result<(nostr_sdk::Keys, nostr_sdk::Client)> {
        let keys = nostr_sdk::Keys::from_sk_str(sk)?;
        let client = nostr_sdk::Client::with_opts(&keys, Options::new().wait_for_send(true));
        let relays = self.relays();
        for relay in relays {
            client.add_relay(relay, None).await?;
        }
        client.connect().await;
        Ok((keys, client))
    }

    pub async fn nostr_random_client(
        &self,
    ) -> anyhow::Result<(nostr_sdk::Keys, nostr_sdk::Client)> {
        let keys = nostr_sdk::Keys::generate();
        let sk = keys.secret_key()?.to_bech32()?;
        self.nostr_client(&sk).await
    }

    pub fn starting_block_height(&self) -> usize {
        match self.network() {
            Network::Bitcoin => 790500,
            Network::Signet => 143500,
            _ => 0,
        }
    }

    fn rpc_cookie(&self) -> Option<PathBuf> {
        self.cli
            .cookie
            .as_ref()
            .or(self.file.rpc.cookie.as_ref())
            .cloned()
    }

    fn rpc_user(&self) -> Option<String> {
        self.cli
            .rpcuser
            .as_ref()
            .or(self.file.rpc.user.as_ref())
            .cloned()
    }

    fn rpc_password(&self) -> Option<String> {
        self.cli
            .rpcpass
            .as_ref()
            .or(self.file.rpc.password.as_ref())
            .cloned()
    }

    fn rpc_port(&self) -> u16 {
        self.cli
            .rpcport
            .or(self.file.rpc.port)
            .expect("RPC port required")
    }

    fn rpc_host(&self) -> String {
        self.cli
            .rpchost
            .as_ref()
            .or(self.file.rpc.host.as_ref())
            .cloned()
            .unwrap_or_else(|| "127.0.0.1".to_string())
    }

    fn data(&self) -> PathBuf {
        self.cli
            .data
            .as_ref()
            .or(self.file.data.as_ref())
            .cloned()
            .unwrap_or_else(|| "nomen.db".into())
    }

    pub fn relays(&self) -> Vec<String> {
        self.cli
            .relays
            .as_ref()
            .or(self.file.nostr.relays.as_ref())
            .cloned()
            .unwrap_or_else(|| {
                vec![
                    "wss://relay.damus.io".into(),
                    "wss://relay.snort.social".into(),
                    "wss://nos.lol".into(),
                    "wss://nostr.orangepill.dev".into(),
                ]
            })
    }

    pub fn network(&self) -> Network {
        self.cli
            .network
            .or(self.file.rpc.network)
            .unwrap_or(Network::Bitcoin)
    }

    pub fn server_bind(&self) -> Option<String> {
        match &self.cli.subcommand {
            Subcommand::Server(ServerSubcommand { bind, .. }) => bind.clone(),
            _ => None,
        }
        .or_else(|| self.file.server.bind.clone())
    }

    pub fn server_indexer_delay(&self) -> u64 {
        match &self.cli.subcommand {
            Subcommand::Server(ServerSubcommand { indexer_delay, .. }) => *indexer_delay,
            _ => None,
        }
        .or(self.file.server.indexer_delay)
        .unwrap_or(30)
    }

    pub fn confirmations(&self) -> anyhow::Result<usize> {
        Ok(self.file.server.confirmations.unwrap_or(3))
    }
}
