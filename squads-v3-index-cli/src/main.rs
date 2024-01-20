use anchor_lang::Discriminator;
use anyhow::anyhow;
use borsh::BorshDeserialize;
use clap::Parser;
use solana_cli_config::{Config, CONFIG_FILE};
use solana_client::client_error::ClientErrorKind;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_request::{RpcError, RpcResponseErrorData};
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::keypair::{read_keypair_file, Keypair};
use solana_sdk::signer::Signer;
use solana_sdk::system_program;
use solana_sdk::transaction::Transaction;
use squads_mpl::state::Ms;
use std::io::Write;
use std::str::FromStr;

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
        /// Multisig address
        multisig: Pubkey,
    },
    /// Check if an index exists for a given authority public key
    Check {
        /// Authority address
        authority: Pubkey,
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
        Subcommand::Index { multisig } => {
            index(&client, payer, cli.yes, multisig).await?;
        }
        Subcommand::Check { authority } => {
            check(&client, authority, true).await;
        }
    }

    Ok(())
}

async fn check(client: &RpcClient, authority: Pubkey, verbose: bool) -> bool {
    if authority.is_on_curve() {
        if verbose {
            println!(
                "Authority {} is not a Program Derived Address ❌",
                authority
            );
        }
        return false;
    }

    let program_id = Pubkey::from_str("idxqM2xnXsym7KL9YQmC8GG6TvdV9XxvHeMWdiswpwr").unwrap();
    let index_key = Pubkey::find_program_address(&[authority.as_ref()], &program_id).0;

    let Ok(index) = client.get_account(&index_key).await else {
        if verbose {
            println!("Index account does not exist for {} ❌", authority);
        }
        return false;
    };

    if index.owner != program_id {
        if verbose {
            println!("Index account does not exist for {} ❌", authority);
        }
        return false;
    }
    if verbose {
        println!("Index account exists for {} ✅", authority);
    }
    true
}

async fn index(
    client: &RpcClient,
    payer: Keypair,
    skip_confirmation: bool,
    multisig: Pubkey,
) -> anyhow::Result<()> {
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
        if check(client, authority_key, false).await {
            println!("{} already indexed!", authority_key);
            return Ok(());
        }
        if ms_account.data.len() < 8 {
            println!("Invalid multisig account {}", multisig);
            return Ok(());
        }
        let ms = Ms::try_from_slice(&ms_account.data[8..])?;
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
    Ok(())
}

async fn execute(ix: Instruction, client: &RpcClient, payer: &Keypair) -> anyhow::Result<()> {
    let authority_key = ix.accounts[1].pubkey.clone();
    let multisig_key = ix.accounts[2].pubkey.clone();
    let blockhash = client.get_latest_blockhash().await?;
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[payer], blockhash);
    let mut retries = 1;
    loop {
        match client.send_and_confirm_transaction(&tx).await {
            Ok(_) => {
                println!("Index account created");
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
