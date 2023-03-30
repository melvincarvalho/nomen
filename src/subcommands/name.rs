use std::io::Write;

use anyhow::anyhow;
use bitcoin::secp256k1::SecretKey;
use bitcoincore_rpc::RpcApi;
use nostr_sdk::Keys;

use crate::{
    config::{Config, NameSubcommand, TxInfo},
    util::{Hash160, IndigoKind, Nsid},
};

pub async fn name(config: &Config, cmd: &NameSubcommand) -> anyhow::Result<()> {
    match cmd {
        NameSubcommand::New(new_data) => new::new(config, new_data).await?,
        NameSubcommand::Update(update_data) => update::update(config, update_data).await?,
        NameSubcommand::Record(record_data) => record::record(config, record_data).await?,
    }

    Ok(())
}

// TODO: refactor this module into separate files

mod new {
    use std::io::Write;

    use anyhow::anyhow;
    use bitcoin::{hashes::hex::ToHex, secp256k1::SecretKey};
    use bitcoincore_rpc::{RawTx, RpcApi};
    use itertools::Itertools;
    use nostr_sdk::{prelude::TagKind, EventBuilder, Keys, Tag};

    use crate::{
        config::{Config, NameNewSubcommand, TxInfo},
        subcommands::name::{create_unsigned_tx, get_keys},
        util::{ChildPair, NameKind, Nsid, NsidBuilder},
        util::{Hash160, IndigoKind},
    };

    use super::{get_transaction, op_return};

    pub async fn new(config: &Config, args: &NameNewSubcommand) -> anyhow::Result<()> {
        let keys = get_keys(&args.privkey)?;
        let nsid = args
            .children
            .iter()
            .cloned()
            .map(ChildPair::pair)
            .fold(
                NsidBuilder::new(&args.name, &keys.public_key()),
                |acc, (n, pk)| acc.update_child(&n, pk),
            )
            .finalize();
        let tx = create_unsigned_tx(config, &args.txinfo, nsid, IndigoKind::Create).await?;

        println!("Nsid: {}", nsid.to_hex());
        println!("Unsigned Tx: {}", tx.raw_hex());

        let event = create_event(&args.children, nsid, args, keys)?;
        let (_k, nostr) = config.nostr_random_client().await?;
        let event_id = nostr.send_event(event).await?;

        println!("Sent event {event_id}");

        Ok(())
    }

    fn create_event(
        children: &[ChildPair],
        nsid: Nsid,
        args: &NameNewSubcommand,
        keys: Keys,
    ) -> Result<nostr_sdk::Event, anyhow::Error> {
        let children_json = {
            let s = children
                .iter()
                .cloned()
                .map(ChildPair::pair)
                .map(|(name, pubkey)| (name, pubkey.to_hex()))
                .collect_vec();
            serde_json::to_string(&s)
        }?;
        let event = EventBuilder::new(
            NameKind::Name.into(),
            children_json,
            &[
                Tag::Identifier(nsid.to_hex()),
                Tag::Generic(TagKind::Custom("ind".to_owned()), vec![args.name.clone()]),
            ],
        )
        .to_event(&keys)?;
        Ok(event)
    }
}

mod update {
    use anyhow::anyhow;
    use bitcoin::{hashes::hex::ToHex, Transaction};
    use bitcoincore_rpc::RawTx;
    use itertools::Itertools;
    use nostr_sdk::{prelude::TagKind, EventBuilder, Keys, Tag};

    use crate::{
        config::{Config, NameUpdateSubcommand, TxInfo},
        util::{ChildPair, IndigoKind, NameKind, Nsid, NsidBuilder},
    };

    use super::{create_unsigned_tx, get_keys, get_transaction, op_return};

    pub async fn update(config: &Config, args: &NameUpdateSubcommand) -> anyhow::Result<()> {
        let keys = get_keys(&args.privkey)?;
        let nsid = args
            .children
            .iter()
            .fold(
                NsidBuilder::new(&args.name, &keys.public_key()),
                |acc, child| {
                    let p = child.clone().pair();
                    acc.update_child(&p.0, p.1)
                },
            )
            .prev(args.previous)
            .finalize();
        let tx = create_unsigned_tx(config, &args.txinfo, nsid, IndigoKind::Update).await?;

        println!("Nsid: {}", nsid.to_hex());
        println!("Unsigned Tx: {}", tx.raw_hex());

        let event = update_event(&args.children, args.previous, nsid, args, keys)?;
        let (_k, nostr) = config.nostr_random_client().await?;
        let event_id = nostr.send_event(event).await?;

        println!("Sent event {event_id}");

        Ok(())
    }

    fn update_event(
        children: &[ChildPair],
        prev: Nsid,
        nsid: Nsid,
        args: &NameUpdateSubcommand,
        keys: Keys,
    ) -> Result<nostr_sdk::Event, anyhow::Error> {
        let children_json = {
            let s = children
                .iter()
                .cloned()
                .map(ChildPair::pair)
                .map(|(name, pubkey)| (name, pubkey.to_hex()))
                .collect_vec();
            serde_json::to_string(&s)
        }?;
        let event = EventBuilder::new(
            NameKind::Update.into(),
            children_json,
            &[
                Tag::Identifier(nsid.to_hex()),
                Tag::Generic(
                    TagKind::Custom("ind".to_owned()),
                    vec![args.name.clone(), prev.to_hex()],
                ),
            ],
        )
        .to_event(&keys)?;
        Ok(event)
    }
}

mod record {
    use std::collections::HashMap;

    use nostr_sdk::{prelude::TagKind, EventBuilder, Tag};

    use crate::{
        config::{Config, NameRecordSubcomand},
        util::NameKind,
    };

    use super::get_keys;

    pub async fn record(config: &Config, record_data: &NameRecordSubcomand) -> anyhow::Result<()> {
        let keys = get_keys(&record_data.privkey)?;
        let map: HashMap<String, String> = record_data
            .records
            .iter()
            .map(|p| p.clone().pair())
            .collect();
        let records = serde_json::to_string(&map)?;

        let event = EventBuilder::new(
            NameKind::Record.into(),
            records,
            &[
                Tag::Identifier(record_data.nsid.to_string()),
                Tag::Generic(
                    TagKind::Custom("ind".to_owned()),
                    vec![record_data.name.clone()],
                ),
            ],
        )
        .to_event(&keys)?;

        let (_keys, client) = config.nostr_random_client().await?;
        let event_id = client.send_event(event).await?;
        println!("Sent event {event_id}");

        Ok(())
    }

    fn parse_records(records: &[String]) -> HashMap<String, String> {
        records
            .iter()
            .filter_map(|rec| rec.split_once('='))
            .map(|(k, v)| (k.to_uppercase(), v.to_owned()))
            .collect()
    }
}

fn get_keys(privkey: &Option<SecretKey>) -> Result<Keys, anyhow::Error> {
    let privkey = if let Some(s) = privkey {
        *s
    } else {
        // TODO: use a better system for getting secure info than this, like a secure prompt
        print!("Private key: ");
        std::io::stdout().flush()?;
        let mut s = String::new();
        std::io::stdin().read_line(&mut s)?;
        s.trim().to_string().parse()?
    };
    let keys = Keys::new(privkey);
    Ok(keys)
}

async fn get_transaction(
    config: &Config,
    txid: &bitcoin::Txid,
) -> Result<bitcoin::Transaction, anyhow::Error> {
    let client = config.rpc_client()?;
    let txid = *txid;
    Ok(tokio::task::spawn_blocking(move || client.get_raw_transaction(&txid, None)).await??)
}

fn op_return(nsid: Nsid, kind: IndigoKind) -> Vec<u8> {
    let mut v = Vec::with_capacity(25);
    v.extend(b"IND\x00");
    v.push(kind.into());
    v.extend(nsid.as_ref());
    v
}

async fn create_unsigned_tx(
    config: &Config,
    args: &TxInfo,
    nsid: Nsid,
    kind: IndigoKind,
) -> Result<bitcoin::Transaction, anyhow::Error> {
    let tx = get_transaction(config, &args.txid).await?;
    let txout = &tx.output[args.vout as usize];
    let new_amount = txout
        .value
        .checked_sub(args.fee as u64)
        .ok_or_else(|| anyhow!("Fee is over available amount in tx"))?;
    let txin = bitcoin::TxIn {
        previous_output: bitcoin::OutPoint {
            txid: args.txid,
            vout: args.vout,
        },
        script_sig: bitcoin::Script::new(), // Unsigned tx with empty script
        sequence: bitcoin::Sequence::ZERO,
        witness: bitcoin::Witness::new(),
    };
    let txout = bitcoin::TxOut {
        value: new_amount,
        script_pubkey: args.address.script_pubkey(),
    };
    let op_return = bitcoin::TxOut {
        value: 0,
        script_pubkey: bitcoin::Script::new_op_return(&op_return(nsid, kind)),
    };
    let tx = bitcoin::Transaction {
        version: 1,
        lock_time: bitcoin::PackedLockTime::ZERO,
        input: vec![txin],
        output: vec![txout, op_return],
    };
    Ok(tx)
}
