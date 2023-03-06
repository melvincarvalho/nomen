use anyhow::anyhow;
use bitcoin::{hashes::hex::ToHex, Network};
use bitcoincore_rpc::RpcApi;

use crate::config::Config;

pub fn index_blockchain(config: &Config) -> anyhow::Result<()> {
    let mut height = starting_blockheight(config.network.unwrap())?;
    log::info!("Starting index from block height: {height}");

    let client = config.rpc_client()?;

    let mut blockhash = client.get_block_hash(height)?;
    let mut blockinfo = client.get_block_info(&blockhash)?;
    while let Some(next_hash) = blockinfo.nextblockhash {
        log_height(blockinfo.height as u64);

        for txid in blockinfo.tx {
            let tx = client.get_raw_transaction(&txid, None)?;

            for output in tx.output {
                if output.script_pubkey.is_op_return() {
                    let b = &output.script_pubkey.as_bytes()[2..];
                    if b.starts_with(b"gun") {
                        let b = &b[3..];
                        match parse_gun_output(&b) {
                            Ok(b) => log::info!("GUN output found: {}", b.to_hex()),
                            Err(e) => log::error!("Index error: {e}"),
                        }
                    }
                }
            }
        }

        blockhash = next_hash;
        blockinfo = client.get_block_info(&blockhash)?;
    }
    Ok(())
}

fn log_height(height: u64) {
    if height % 10 == 0 {
        log::info!("Indexing block height {height}");
    } else {
        log::debug!("Indexing block height {height}");
    }
}

fn starting_blockheight(network: Network) -> anyhow::Result<u64> {
    match network {
        Network::Bitcoin => Err(anyhow!("Unsupported network {}", network)),
        Network::Testnet => Err(anyhow!("Unsupported network {}", network)),
        Network::Signet => Err(anyhow!("Unsupported network {}", network)),
        Network::Regtest => Ok(1),
    }
}

fn parse_gun_output(byte: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut b = byte.into_iter();
    let (gun_ver, gun_type) = (b.next(), b.next());
    match (gun_ver, gun_type) {
        (Some(&0), Some(&0)) => Ok(b.copied().collect()),
        _ => Err(anyhow!("Invalid GUN code")),
    }
}