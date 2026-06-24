#![allow(unused)]
use bitcoincore_rpc::bitcoin::hex::DisplayHex;
use bitcoincore_rpc::bitcoin::Amount;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use serde::Deserialize;
use serde_json::json;
use std::fs::File;
use std::io::Write;

// Node access params
const RPC_URL: &str = "http://127.0.0.1:18443"; // Default regtest RPC port
const RPC_USER: &str = "alice";
const RPC_PASS: &str = "password";

// You can use calls not provided in RPC lib API using the generic `call` function.
// An example of using the `send` RPC call, which doesn't have exposed API.
// You can also use serde_json `Deserialize` derivation to capture the returned json result.
fn send(rpc: &Client, addr: &str) -> bitcoincore_rpc::Result<String> {
    let args = [
        json!([{addr : 100 }]), // recipient address
        json!(null),            // conf target
        json!(null),            // estimate mode
        json!(null),            // fee rate in sats/vb
        json!(null),            // Empty option object
    ];

    #[derive(Deserialize)]
    struct SendResult {
        complete: bool,
        txid: String,
    }
    let send_result = rpc.call::<SendResult>("send", &args)?;
    assert!(send_result.complete);
    Ok(send_result.txid)
}

fn main() -> bitcoincore_rpc::Result<()> {
    // Connect to Bitcoin Core RPC
    let rpc = Client::new(
        RPC_URL,
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    // Get blockchain info
    let blockchain_info = rpc.get_blockchain_info()?;
    println!("Blockchain Info: {:?}", blockchain_info);

    // Create/Load the wallets, named 'Miner' and 'Trader'. Have logic to optionally create/load them if they do not exist or not loaded already.
    let miner_wallet = create_or_load_wallet(&rpc, "Miner")?;
    let trader_wallet = create_or_load_wallet(&rpc, "Trader")?;

    // Generate spendable balances in the Miner wallet. How many blocks needs to be mined?
    let miner_addr = miner_wallet
        .get_new_address(Some("Mining Reward"), None)?
        .assume_checked();
    println!("Miner address: {}", miner_addr);

    // Mine new blocks to this address until you get positive wallet balance.
    // Coinbase transactions require 100 confirmations before they can be spent.
    // So we need to mine at least 101 blocks (100 to mature, 1 for the balance to show up as spendable).
    println!("Mining blocks to Miner wallet...");
    miner_wallet.generate_to_address(101, &miner_addr)?;

    // Explain why wallet balance for block rewards behaves that way.
    // Comment: Coinbase transactions (block rewards) have a maturity requirement of 100 blocks in Bitcoin.
    // This prevents the rewards from being spent immediately, reducing the risk of funds disappearing
    // if a reorganization occurs.

    let balance = miner_wallet.get_balance(None, None)?;
    println!("Miner balance: {}", balance);

    // Create a receiving addressed labeled "Received" from Trader wallet.
    let trader_addr = trader_wallet
        .get_new_address(Some("Received"), None)?
        .assume_checked();
    println!("Trader address: {}", trader_addr);

    // Send 20 BTC from Miner to Trader
    let amount = Amount::from_btc(20.0).map_err(|e| {
        bitcoincore_rpc::Error::JsonRpc(bitcoincore_rpc::jsonrpc::Error::Rpc(
            bitcoincore_rpc::jsonrpc::error::RpcError {
                code: -1,
                message: e.to_string(),
                data: None,
            },
        ))
    })?;
    let txid =
        miner_wallet.send_to_address(&trader_addr, amount, None, None, None, None, None, None)?;
    println!("Transaction ID: {}", txid);

    // Check transaction in mempool
    let mempool_entry = rpc.get_mempool_entry(&txid)?;
    println!("Mempool entry: {:?}", mempool_entry);

    // Mine 1 block to confirm the transaction
    miner_wallet.generate_to_address(1, &miner_addr)?;

    // Extract all required transaction details
    let tx = miner_wallet.get_transaction(&txid, Some(true))?;
    let block_height = miner_wallet.get_block_count()?;
    let block_hash = miner_wallet.get_block_hash(block_height)?;

    // Get more details from getrawtransaction
    let raw_tx = miner_wallet.get_raw_transaction_info(&txid, None)?;

    // Identify Miner's Input Address and Amount
    // This is a bit tricky as there might be multiple inputs.
    // But for a simple transaction from Miner, we can check the inputs.
    // Actually, we can get this from the transaction info.

    let mut input_addr = String::new();
    let mut input_amount = 0.0;

    // For the sake of this exercise, we can look at the inputs of the transaction
    for input in &raw_tx.vin {
        if let Some(prev_txid) = input.txid {
            let prev_tx = rpc.get_raw_transaction_info(&prev_txid, None)?;
            let vout = &prev_tx.vout[input.vout.unwrap() as usize];
            if let Some(ref addr) = vout.script_pub_key.address {
                input_addr = addr.clone().assume_checked().to_string();
                input_amount = vout.value.to_btc();
                break; // Just take the first one for simplicity if multiple
            }
        }
    }

    let trader_output_addr = trader_addr.to_string();
    let mut trader_output_amount = 0.0;
    let mut change_addr = String::new();
    let mut change_amount = 0.0;

    for output in &raw_tx.vout {
        if let Some(ref addr) = output.script_pub_key.address {
            let addr_str = addr.clone().assume_checked().to_string();
            if addr_str == trader_output_addr {
                trader_output_amount = output.value.to_btc();
            } else {
                change_addr = addr_str;
                change_amount = output.value.to_btc();
            }
        }
    }

    let fees = tx
        .details
        .iter()
        .fold(0.0, |acc, d| acc + d.fee.map(|f| f.to_btc()).unwrap_or(0.0));

    // Write the data to ../out.txt in the specified format given in readme.md
    let mut file = File::create("../out.txt").expect("Unable to create file");
    writeln!(file, "{}", txid)?;
    writeln!(file, "{}", input_addr)?;
    writeln!(file, "{}", input_amount)?;
    writeln!(file, "{}", trader_output_addr)?;
    writeln!(file, "{}", trader_output_amount)?;
    writeln!(file, "{}", change_addr)?;
    writeln!(file, "{}", change_amount)?;
    writeln!(file, "{}", fees)?;
    writeln!(file, "{}", block_height)?;
    writeln!(file, "{}", block_hash)?;

    Ok(())
}

fn create_or_load_wallet(rpc: &Client, name: &str) -> bitcoincore_rpc::Result<Client> {
    let wallets = rpc.list_wallets()?;
    if !wallets.contains(&name.to_string()) {
        match rpc.create_wallet(name, None, None, None, None) {
            Ok(_) => println!("Wallet '{}' created.", name),
            Err(e) => {
                // If creation fails, it might be because it already exists but isn't loaded
                match rpc.load_wallet(name) {
                    Ok(_) => println!("Wallet '{}' loaded.", name),
                    Err(_) => return Err(e),
                }
            }
        }
    }
    let wallet_url = format!("{}/wallet/{}", RPC_URL, name);
    Client::new(
        &wallet_url,
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )
}
