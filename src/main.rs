mod fp;
mod hash;
mod keys;
mod proof;
mod tree;

use axum::{
    response::{Html, Json},
    routing::get,
    Router,
};

use ethers::prelude::*;

use eyre::Result;
use keys::{PrivateKey, PublicKey};
use proof::prove;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

use proof::Proof;
use structopt::StructOpt;
use tree::SparseMerkleTree;

// Initialize wallet, TODO: let secret be derived from a BIP-39 mnemonic code
#[derive(StructOpt, Debug)]
pub struct InitOpt {
    endpoint: String,
}

// Open web wallet interface
#[derive(StructOpt, Debug)]
pub struct WalletOpt {}

// Show wallet info
#[derive(StructOpt, Debug)]
pub struct InfoOpt {}

// Deposit to Owshen address
#[derive(StructOpt, Debug)]
pub struct DepositOpt {
    #[structopt(long)]
    to: PublicKey,
}

// Withdraw to Ethereum address
#[derive(StructOpt, Debug)]
pub struct WithdrawOpt {
    #[structopt(long)]
    to: Address,
}

#[derive(StructOpt, Debug)]
enum OwshenCliOpt {
    Init(InitOpt),
    Info(InfoOpt),
    Deposit(DepositOpt),
    Withdraw(WithdrawOpt),
    Wallet(WalletOpt),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GetWithdrawResponse {
    proof: Proof,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Wallet {
    priv_key: PrivateKey,
    endpoint: String,
}

const PARAMS_FILE: &str = "contracts/circuits/coin_withdraw_0001.zkey";

async fn root(pub_key: PublicKey) -> Html<String> {
    Html(include_str!("html/wallet.html").replace("{OWSHEN_ADDRESS}", &pub_key.to_string()))
}

async fn serve_wallet(pub_key: PublicKey) -> Result<()> {
    let app = Router::new()
        .route("/", get(move || async { root(pub_key).await }))
        .route("/withdraw", get(|| async { Json(Proof::default()) }));

    const PORT: u16 = 8000;
    let url = format!("http://127.0.0.1:{}", PORT);
    let addr = SocketAddr::from(([127, 0, 0, 1], 8000));
    println!("Running wallet on: {}", url);
    open::that(url).unwrap();
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let wallet_path = home::home_dir().unwrap().join(".owshen-wallet.json");

    let wallet = std::fs::read_to_string(&wallet_path)
        .map(|s| {
            let w: Wallet = serde_json::from_str(&s).expect("Invalid wallet file!");
            w
        })
        .ok();

    let opt = OwshenCliOpt::from_args();

    match opt {
        OwshenCliOpt::Init(InitOpt { endpoint }) => {
            if wallet.is_none() {
                let wallet = Wallet {
                    priv_key: PrivateKey::generate(&mut rand::thread_rng()),
                    endpoint,
                };
                std::fs::write(wallet_path, serde_json::to_string(&wallet).unwrap()).unwrap();
            } else {
                println!("Wallet is already initialized!");
            }
        }
        OwshenCliOpt::Wallet(WalletOpt {}) => {
            if let Some(wallet) = &wallet {
                serve_wallet(wallet.priv_key.clone().into()).await?;
            } else {
                println!("Wallet is not initialized!");
            }
        }
        OwshenCliOpt::Info(InfoOpt {}) => {
            if let Some(wallet) = &wallet {
                println!(
                    "Owshen Address: {}",
                    PublicKey::from(wallet.priv_key.clone())
                );
            } else {
                println!("Wallet is not initialized!");
            }
        }
        OwshenCliOpt::Deposit(DepositOpt { to }) => {
            // Transfer ETH to the Owshen contract and create a new commitment
            println!("Depositing a coin to Owshen address: {}", to);
        }
        OwshenCliOpt::Withdraw(WithdrawOpt { to }) => {
            // Prove you own a certain coin in the Owshen contract and retrieve rewards in the given ETH address
            let mut smt = SparseMerkleTree::new(32);
            smt.set(123, 4567.into());
            smt.set(2345, 4567.into());
            smt.set(2346, 1234.into());
            smt.set(0, 11234.into());
            smt.set(12345678, 11234.into());
            let val = smt.get(2345);
            println!(
                "{:?}: {}",
                smt.root(),
                SparseMerkleTree::verify(smt.root(), 2345, &val)
            );
            println!(
                "Proof: {:?}",
                prove(
                    PARAMS_FILE,
                    2345,
                    val.value,
                    123,
                    val.proof.try_into().unwrap()
                )?
            );
            println!("Withdraw a coin to Ethereum address: {}", to);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bindings::coin_withdraw_verifier::CoinWithdrawVerifier;
    use ethers::abi::Abi;
    use ethers::utils::Ganache;
    use std::sync::Arc;

    use ethers::core::types::Bytes;
    use ethers::middleware::contract::ContractFactory;
    use std::str::FromStr;

    #[tokio::test]
    async fn test_poseidon() {
        let port = 8545u16;
        let url = format!("http://localhost:{}", port).to_string();

        let _ganache = Ganache::new().port(port).spawn();
        let provider = Provider::<Http>::try_from(url).unwrap();
        let provider = Arc::new(provider);
        let accounts = provider.get_accounts().await.unwrap();
        let from = accounts[0];

        let abi = serde_json::from_str::<Abi>(include_str!("html/poseidon2.abi")).unwrap();
        let bytecode = Bytes::from_str(include_str!("html/poseidon2.evm")).unwrap();

        let client = Provider::<Http>::try_from("http://localhost:8545").unwrap();
        let client = std::sync::Arc::new(client);

        let factory = ContractFactory::new(abi, bytecode, client);

        let mut deployer = factory.deploy(()).unwrap().legacy();
        deployer.tx.set_from(from);

        let contract = deployer.send().await.unwrap();

        let func = contract
            .method_hash::<_, U256>([41, 165, 242, 246], ([U256::from(123), U256::from(234)],))
            .unwrap();

        let gas = func.clone().estimate_gas().await.unwrap();
        assert_eq!(gas, 50349.into());

        let hash = func.clone().call().await.unwrap();

        assert_eq!(
            hash,
            U256::from_str_radix(
                "0x0e331f99e024251a3a17152d7562d6257edc99595f9169b4e3b122d58a0e9d62",
                16
            )
            .unwrap()
        );
    }

    #[tokio::test]
    async fn test_deposit() {
        let priv_key = PrivateKey::from_secret(1234.into());
        let pub_key: PublicKey = priv_key.clone().into();
        let timestamp = 123;

        let mut smt = SparseMerkleTree::new(32);
        smt.set(123, 4567.into());
        smt.set(2345, pub_key.commitment);
        smt.set(2346, 1234.into());
        smt.set(0, 11234.into());
        smt.set(12345678, 11234.into());
        let val = smt.get(2345);

        let port = 8545u16;
        let url = format!("http://localhost:{}", port).to_string();

        let ganache = Ganache::new().port(port).spawn();

        let provider = Provider::<Http>::try_from(url).unwrap();
        let provider = Arc::new(provider);

        let accounts = provider.get_accounts().await.unwrap();
        let from = accounts[0];

        let proof = prove(
            PARAMS_FILE,
            2345,
            1234.into(),
            timestamp,
            val.proof.try_into().unwrap(),
        )
        .unwrap();

        let verifier = CoinWithdrawVerifier::deploy(provider.clone(), ())
            .unwrap()
            .legacy()
            .from(from)
            .send()
            .await
            .unwrap();

        let verified = verifier
            .verify_proof(
                proof.a,
                proof.b,
                proof.c,
                [smt.root().into(), priv_key.nullifier(2345).into()],
            )
            .legacy()
            .from(from)
            .call()
            .await
            .unwrap();

        assert!(verified);

        drop(ganache);
    }
}
