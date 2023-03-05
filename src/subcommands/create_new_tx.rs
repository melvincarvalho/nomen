use std::str::FromStr;

use anyhow::anyhow;
use bitcoin::{
    hashes::hex::ToHex, Address, OutPoint, PackedLockTime, Script, Sequence, Transaction, TxIn,
    TxOut, Txid, Witness,
};
use bitcoincore_rpc::{RawTx, RpcApi};
use yansi::Paint;

use crate::{
    config::Config,
    name::{self, Name},
};

pub fn create_new_tx(
    config: &Config,
    name: &String,
    input: &String,
    address: &String,
    pubkey: &String,
    fee_rate: &usize,
) -> anyhow::Result<()> {
    let mut pkbin = [0; 32];
    hex::decode_to_slice(&pubkey, &mut pkbin)?;
    let name = Name {
        parent_name: String::new(),
        name: name.clone(),
        pubkey: pkbin,
        names: vec![],
    };
    name::Validator::new(&name).validate()?;

    let mut input = input.split(':');
    let txid: Txid = input.next().ok_or(anyhow!("Invalid input"))?.parse()?;
    let vout: usize = input.next().ok_or(anyhow!("Invalid input"))?.parse()?;
    let address = Address::from_str(&address)?;

    let client = config.rpc_client()?;
    let tx = client.get_raw_transaction(&txid, None)?;
    let txout = tx.output.get(vout).ok_or(anyhow!("Invalid output"))?;
    let txin = TxIn {
        previous_output: OutPoint {
            txid,
            vout: vout as u32,
        },
        script_sig: Script::new(),
        sequence: Sequence::MAX,
        witness: Witness::new(),
    };
    let new_txout = TxOut {
        value: txout.value,
        script_pubkey: address.script_pubkey(),
    };

    let mut op_return = format!("gun\x00\x00").as_bytes().to_vec();
    let namespace_id = name.namespace_id();
    log::debug!("Namespace id for {}: {}", name.name, namespace_id.to_hex());
    op_return.extend(name.namespace_id());
    let op_return = Script::new_op_return(&op_return);
    let op_out = TxOut {
        value: 0,
        script_pubkey: op_return,
    };

    let mut new_tx = Transaction {
        version: 1,
        lock_time: PackedLockTime::ZERO,
        input: vec![txin],
        output: vec![new_txout, op_out],
    };

    let fee = (new_tx.vsize() * fee_rate) as u64;
    log::debug!("Estimated fee: {fee}");
    new_tx.output[0].value -= fee;

    println!(
        "{}",
        Paint::green(
            "Here is the unsigned tranasction. Sign it and broadcast it from your wallet:\n"
        )
    );
    println!("{}", new_tx.raw_hex());
    Ok(())
}
