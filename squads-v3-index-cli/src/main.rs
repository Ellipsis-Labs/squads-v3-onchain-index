use anchor_lang::Discriminator;
use anyhow::anyhow;
use borsh::BorshDeserialize;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use solana_cli_config::{Config, CONFIG_FILE};
use solana_client::client_error::ClientErrorKind;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_client::rpc_request::{RpcError, RpcResponseErrorData};
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::signer::keypair::{read_keypair_file, Keypair};
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use solana_sdk::{bpf_loader_upgradeable, system_program};
use solana_transaction_status::UiTransactionEncoding;
use squads_mpl::state::Ms;
use std::io::Write;
use std::str::FromStr;
use std::vec;

#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    #[clap(subcommand)]
    subcommand: Subcommand,
    /// Optionally include your RPC endpoint. Use "local", "dev", "main" for default endpoints. Defaults to your Solana CLI config file.
    #[clap(global = true, short, long)]
    url: Option<String>,
    /// Optionally include your keypair path. Defaults to your Solana CLI config file.
    #[clap(global = true, short, long)]
    keypair_path: Option<String>,
    /// Optionally include your RPC endpoint. Use "local", "dev", "main" for default endpoints. Defaults to your Solana CLI config file.
    #[clap(global = true, short, long, default_value = "false")]
    yes: bool,
}

#[derive(Parser, Debug)]
#[clap(author = "Ellipsis", version, about)]
enum Subcommand {
    /// Create an on-chain index that ties a multisig authority to the Squads V3 program
    Index {
        /// Address of the multisig authority or the program
        address: Pubkey,
    },
    /// Check if an index exists for a given authority public key
    Check {
        /// Address of the multisig authority or the program
        address: Pubkey,
    },
}

pub fn get_network(network_str: &str) -> &str {
    match network_str {
        "mainnet" | "main" | "m" | "mainnet-beta" => "https://api.mainnet-beta.solana.com",
        _ => network_str,
    }
}

pub fn prompt_for_confirmation(message: &str) -> anyhow::Result<bool> {
    loop {
        let input = get_response(message)?;
        let trimmed_input = input.trim();
        match trimmed_input {
            "Yes" | "yes" | "y" => return Ok(true),
            "No" | "no" | "n" => {
                return Ok(false);
            }
            _ => {
                writeln!(std::io::stdout(), "Please indicate yes or no.")?;
                continue;
            }
        }
    }
}

pub fn get_response(message: &str) -> anyhow::Result<String> {
    write!(std::io::stdout(), "{}\n(y/n) ", message)?;
    std::io::stdout().flush()?;
    let mut buffer = String::new();
    std::io::stdin().read_line(&mut buffer)?;
    Ok(buffer)
}

pub fn get_payer_keypair_from_path(path: &str) -> anyhow::Result<Keypair> {
    read_keypair_file(&*shellexpand::tilde(path)).map_err(|e| anyhow!(e.to_string()))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Args::parse();
    let config = match CONFIG_FILE.as_ref() {
        Some(config_file) => Config::load(config_file).unwrap_or_else(|_| {
            println!("Failed to load config file: {}", config_file);
            Config::default()
        }),
        None => Config::default(),
    };
    let commitment = CommitmentConfig::confirmed();
    let payer = get_payer_keypair_from_path(&cli.keypair_path.unwrap_or(config.keypair_path))
        .expect("Keypair file does not exist. Please run `solana-keygen new`");
    let network_url = &get_network(
        &cli.url
            .unwrap_or("https://api.mainnet-beta.solana.com".to_string()),
    )
    .to_string();
    let client = RpcClient::new_with_commitment(network_url.to_string(), commitment);
    match cli.subcommand {
        Subcommand::Index { address } => {
            index(&client, payer, cli.yes, address).await?;
        }
        Subcommand::Check { address } => {
            check(&client, address, true).await?;
        }
    }

    Ok(())
}

async fn index(
    client: &RpcClient,
    payer: Keypair,
    skip_confirmation: bool,
    address: Pubkey,
) -> anyhow::Result<()> {
    let mut is_program = false;
    let account_data = client.get_account(&address).await;
    let multisig = match account_data {
        Ok(account_data) => {
            if account_data.owner == squads_mpl::id() {
                if account_data.data.len() < 8 {
                    println!("Invalid multisig account {}", address);
                    return Ok(());
                }
                let _ = Ms::try_from_slice(&account_data.data[8..])?;
                let mut disc = [0_u8; 8];
                disc.copy_from_slice(&account_data.data[..8]);
                if Ms::DISCRIMINATOR != disc {
                    println!("Invalid multisig account {}", address);
                    return Ok(());
                }
                address
            } else if account_data.owner == bpf_loader_upgradeable::id()
                && account_data.data.len() == 36
            {
                let (program_data, _) = Pubkey::find_program_address(
                    &[address.as_ref()],
                    &bpf_loader_upgradeable::id(),
                );
                let program_data_account = client.get_account(&program_data).await?;
                if program_data_account.data[12] == 0 {
                    println!("Program is immutable");
                    return Ok(());
                }
                let authority = Pubkey::try_from_slice(program_data_account.data[13..45].as_ref())?;
                if authority.is_on_curve() {
                    println!(
                        "Ugrade Authority for {} is not a Program Derived Address ❌",
                        address
                    );
                    return Ok(());
                }
                println!("Searching for multisig for {}", address);
                let ms =
                    get_multisig_account_from_program_data(client, &program_data, &authority).await;
                if let Some(ms) = ms {
                    is_program = true;
                    println!("Found multisig for {}: {}", address, ms);
                    ms
                } else {
                    println!("Failed to find multisig for {}", address);
                    return Ok(());
                }
            } else {
                println!("Invalid Account {}", address);
                println!("{:#?}", account_data);
                return Ok(());
            }
        }
        Err(_) => {
            println!("Account {} does not exist", address);
            return Ok(());
        }
    };

    let (authority_key, _) = Pubkey::find_program_address(
        &[
            b"squad",
            multisig.as_ref(),
            &1_u32.to_le_bytes(), // Authority index should just be 1
            b"authority",
        ],
        &squads_mpl::id(),
    );

    let program_id = Pubkey::from_str("idxqM2xnXsym7KL9YQmC8GG6TvdV9XxvHeMWdiswpwr")?;

    let index_key = Pubkey::find_program_address(&[authority_key.as_ref()], &program_id).0;

    // Instruction to create the index account
    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(authority_key, false),
            AccountMeta::new_readonly(multisig, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(index_key, false),
        ],
        data: vec![],
    };

    if skip_confirmation {
        execute(ix, client, &payer).await?;
    } else {
        let Ok(ms_account) = client.get_account(&multisig).await else {
            println!("Multisig account does not exist");
            return Ok(());
        };
        if check(client, authority_key, false).await? {
            println!(
                "{} already indexed!",
                if is_program { address } else { authority_key }
            );
            return Ok(());
        }
        if ms_account.data.len() < 8 {
            println!("Invalid multisig account {}", multisig);
            return Ok(());
        }
        // We need to pass in the exact offset of the vector's end to satisfy Borsh deserialization
        let vec_offset = 58;
        let vec_len = u32::from_le_bytes(ms_account.data[54..58].try_into().unwrap());
        let vec_end = (vec_offset + vec_len * 32) as usize;
        let ms = Ms::try_from_slice(&ms_account.data[8..vec_end])?;
        let mut disc = [0_u8; 8];
        disc.copy_from_slice(&ms_account.data[..8]);
        if Ms::DISCRIMINATOR != disc {
            println!("Invalid multisig account {}", multisig);
            return Ok(());
        }
        println!("{}/{} Multisig account exists", ms.threshold, ms.keys.len());
        println!("Multisig key: {}", multisig);
        println!("Authority key: {}", authority_key);
        println!();
        let confirmation_str = format!(
            "Executing instruction: \n\n{:#?}\n\nCost: {} SOL\n",
            ix, "0.00089588"
        );
        if prompt_for_confirmation(&confirmation_str)? {
            execute(ix, client, &payer).await?;
        } else {
            println!("Exiting without executing instruction");
            return Ok(());
        }
    }
    if is_program {
        println!("Program {} is now linked to Squads V3!", address);
    } else {
        println!("Authority {} is now indexed!", authority_key);
    }
    Ok(())
}

async fn check(client: &RpcClient, address: Pubkey, verbose: bool) -> anyhow::Result<bool> {
    let mut is_program = false;
    let authority = {
        let account_res = client.get_account(&address).await;
        match account_res {
            Ok(a) => {
                // Allow user to pass in a program ID
                if a.owner == bpf_loader_upgradeable::id() && a.data.len() == 36 {
                    let (program_data, _) = Pubkey::find_program_address(
                        &[address.as_ref()],
                        &bpf_loader_upgradeable::id(),
                    );
                    let program_data_account = client.get_account(&program_data).await?;
                    is_program = true;
                    Pubkey::try_from_slice(program_data_account.data[13..45].as_ref())?
                } else {
                    address
                }
            }
            Err(_) => address,
        }
    };
    if authority.is_on_curve() {
        if verbose {
            println!(
                "Authority {} is not a Program Derived Address ❌",
                authority
            );
        }
        return Ok(false);
    }

    let program_id = Pubkey::from_str("idxqM2xnXsym7KL9YQmC8GG6TvdV9XxvHeMWdiswpwr").unwrap();
    let index_key = Pubkey::find_program_address(&[authority.as_ref()], &program_id).0;

    let Ok(index) = client.get_account(&index_key).await else {
        if verbose {
            println!("Index account does not exist for {} ❌", authority);
        }
        return Ok(false);
    };

    if index.owner != program_id {
        if verbose {
            println!("Index account does not exist for {} ❌", authority);
        }
        return Ok(false);
    }
    if verbose {
        println!("Index account exists for {} ✅", authority);
        if is_program {
            println!();
            println!("{} is controlled by a Squads multisig", address);
        }
        println!();
        if let Some(multisig_addr) =
            get_multisig_account_from_authority(client, &index_key, &authority).await
        {
            let account_data = client.get_account(&multisig_addr).await?;
            // We need to pass in the exact offset of the vector's end to satisfy Borsh deserialization
            let vec_offset = 58;
            let vec_len = u32::from_le_bytes(account_data.data[54..58].try_into().unwrap());
            let vec_end = (vec_offset + vec_len * 32) as usize;
            if let Ok(multisig) = Ms::try_from_slice(&account_data.data[8..vec_end]) {
                println!("Multisig details");
                println!("Address: {}", multisig_addr);
                println!("Threshold: {}/{}", multisig.threshold, multisig.keys.len());
                println!("Members: {:#?}", multisig.keys);
            }
        }
    }
    Ok(true)
}

async fn get_multisig_account_from_authority(
    client: &RpcClient,
    index_key: &Pubkey,
    authority: &Pubkey,
) -> Option<Pubkey> {
    let transaction_history = client
        .get_signatures_for_address(&index_key)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|tx| {
            if tx.err.is_none() {
                Some(tx.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if !transaction_history.is_empty() {
        let last_transaction = transaction_history.iter().rev().next()?;
        if let Some(key) = extract_multisig_key_from_transaction(
            client,
            &Signature::from_str(&last_transaction.signature).unwrap(),
            authority,
        )
        .await
        {
            return Some(key);
        }
    }
    None
}

async fn get_multisig_account_from_program_data(
    client: &RpcClient,
    program_data: &Pubkey,
    authority: &Pubkey,
) -> Option<Pubkey> {
    let transaction_history = client
        .get_signatures_for_address(&program_data)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|tx| {
            if tx.err.is_none() {
                Some(tx.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let progress_bar = ProgressBar::new(42);
    progress_bar
        .set_style(ProgressStyle::default_spinner().template("{spinner:.green} {wide_msg}"));
    progress_bar.enable_steady_tick(100);

    let total_transactions = transaction_history.len();
    progress_bar.set_message(format!(
        "[{}/{}] Searching transaction history",
        0, total_transactions,
    ));

    for (i, tx) in transaction_history.iter().enumerate() {
        let sig = &Signature::from_str(&tx.signature).unwrap();
        progress_bar.set_message(format!(
            "[{}/{}] Searching transaction history: {}",
            i + 1,
            total_transactions,
            sig,
        ));
        if let Some(key) = extract_multisig_key_from_transaction(client, &sig, authority).await {
            progress_bar.set_message(format!("Found multisig key after {} transactions", i + 1));
            return Some(key);
        }
    }
    None
}

async fn extract_multisig_key_from_transaction(
    client: &RpcClient,
    signature: &Signature,
    authority: &Pubkey,
) -> Option<Pubkey> {
    let transaction_details = client
        .get_transaction_with_config(
            &signature,
            RpcTransactionConfig {
                commitment: Some(CommitmentConfig::confirmed()),
                max_supported_transaction_version: Some(1),
                encoding: Some(UiTransactionEncoding::Binary),
            },
        )
        .await
        .ok()?;
    let tx = transaction_details
        .transaction
        .transaction
        .decode()?
        .into_legacy_transaction()?;
    for account in tx.message.account_keys.iter() {
        let (derived_authority_key, _) = Pubkey::find_program_address(
            &[
                b"squad",
                account.as_ref(),
                &1_u32.to_le_bytes(), // Authority index should just be 1
                b"authority",
            ],
            &squads_mpl::id(),
        );
        if &derived_authority_key != authority {
            continue;
        }
        return Some(account.clone());
    }
    None
}

async fn execute(ix: Instruction, client: &RpcClient, payer: &Keypair) -> anyhow::Result<()> {
    let authority_key = ix.accounts[1].pubkey.clone();
    let multisig_key = ix.accounts[2].pubkey.clone();
    let blockhash = client.get_latest_blockhash().await?;
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[payer], blockhash);
    let mut retries = 1;
    loop {
        match client
            .send_and_confirm_transaction_with_spinner_and_commitment(
                &tx,
                CommitmentConfig::confirmed(),
            )
            .await
        {
            Ok(_) => {
                break;
            }
            Err(e) => {
                match e.kind() {
                    ClientErrorKind::RpcError(RpcError::RpcResponseError {
                        code: _,
                        message: _,
                        data,
                    }) => {
                        if let RpcResponseErrorData::SendTransactionPreflightFailure(_) = data {
                            println!("Invalid multisig account {}", multisig_key);
                            return Ok(());
                        }
                    }
                    _ => {}
                }
                println!("Attempt {}. Error creating index account: {}", retries, e);
                retries += 1;
                if retries > 10 {
                    println!("Failed to create index account after 10 attempts");
                    return Ok(());
                }
                continue;
            }
        }
    }
    println!("Successfully created index for {}", authority_key);
    Ok(())
}
