use alloy::{
    hex::FromHex,
    primitives::{keccak256, Address, U256},
};
use alloy_rlp::Encodable;
use clap::Parser;
use hex::decode as hex_decode;
use reqwest::Client;
use secp256k1::{Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};
use std::error::Error;
use tokio::main;

#[derive(Serialize)]
struct JsonRpcRequest<'a, T> {
    jsonrpc: &'static str,
    method: &'a str,
    params: T,
    id: u64,
}

#[derive(Deserialize)]
struct JsonRpcResponse<T> {
    #[serde(default)]
    result: Option<T>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

/// A simple “legacy” transaction container
#[derive(Debug)]
struct LegacyTransaction {
    nonce: U256,
    gas_price: U256,
    gas_limit: U256,
    to: Option<Address>,
    value: U256,
    // data must be string (if vec<u8> then it gets encoded as list..)
    data: Vec<u8>,
    v: u64,
    r: U256,
    s: U256,
}

/// RLP for the “unsigned” portion and for the “signed” portion
impl LegacyTransaction {
    fn rlp_encode_unsigned(&self, chain_id: u64) -> Vec<u8> {
        let mut buffer = Vec::<u8>::new();
        self.nonce.encode(&mut buffer);
        self.gas_price.encode(&mut buffer);
        self.gas_limit.encode(&mut buffer);
        self.to.unwrap_or_default().encode(&mut buffer);
        self.value.encode(&mut buffer);
        self.data.as_slice().encode(&mut buffer);

        // EIP-155 includes chain_id, 0, 0 at the end

        chain_id.encode(&mut buffer);
        0u8.encode(&mut buffer);
        0u8.encode(&mut buffer);

        let aa = alloy_rlp::Header {
            list: true,
            payload_length: buffer.len(),
        };

        let mut new_buffer = Vec::<u8>::new();
        aa.encode(&mut new_buffer);
        new_buffer.append(&mut buffer);
        new_buffer
    }

    fn rlp_encode_signed(&self) -> Vec<u8> {
        let mut buffer = Vec::<u8>::new();
        self.nonce.encode(&mut buffer);
        self.gas_price.encode(&mut buffer);
        self.gas_limit.encode(&mut buffer);
        self.to.unwrap_or_default().encode(&mut buffer);
        self.value.encode(&mut buffer);
        self.data.as_slice().encode(&mut buffer);
        self.v.encode(&mut buffer);
        self.r.encode(&mut buffer);
        self.s.encode(&mut buffer);

        let aa = alloy_rlp::Header {
            list: true,
            payload_length: buffer.len(),
        };

        let mut new_buffer = Vec::<u8>::new();
        aa.encode(&mut new_buffer);
        new_buffer.append(&mut buffer);
        new_buffer
    }
}

/// Minimal EIP-1559 transaction (type=2), ignoring error checks/optional fields
#[derive(Debug)]
struct Eip1559Transaction {
    chain_id: u64,
    nonce: U256,
    max_priority_fee_per_gas: U256,
    max_fee_per_gas: U256,
    gas_limit: U256,
    to: Option<Address>,
    value: U256,
    data: Vec<u8>,
    #[allow(dead_code)]
    access_list: Vec<(Address, Vec<U256>)>, // or a custom struct

    // Signature
    y_parity: u8, // 0 or 1
    r: U256,
    s: U256,
}

impl Eip1559Transaction {
    fn rlp_internal(&self) -> Vec<u8> {
        let mut buffer = Vec::<u8>::new();

        self.chain_id.encode(&mut buffer);
        self.nonce.encode(&mut buffer);
        self.max_priority_fee_per_gas.encode(&mut buffer);
        self.max_fee_per_gas.encode(&mut buffer);
        self.gas_limit.encode(&mut buffer);

        // If `to` is `None`, encode as empty bytes
        match self.to {
            Some(to_addr) => to_addr.encode(&mut buffer),
            None => (&[] as &[u8]).encode(&mut buffer),
        };
        self.value.encode(&mut buffer);
        self.data.as_slice().encode(&mut buffer);

        // access list - TODO.

        let aa1 = alloy_rlp::Header {
            list: true,
            payload_length: 0,
        };
        aa1.encode(&mut buffer);

        buffer
    }

    /// RLP for the *unsigned* portion, which you then keccak256 and sign
    fn rlp_encode_unsigned(&self) -> Vec<u8> {
        let mut buffer = self.rlp_internal();
        let aa = alloy_rlp::Header {
            list: true,
            payload_length: buffer.len(),
        };
        let mut new_buffer = Vec::<u8>::new();
        // this is crucial here.
        new_buffer.push(0x02);

        aa.encode(&mut new_buffer);
        new_buffer.append(&mut buffer);
        new_buffer
    }

    fn rlp_encode_signed(&self) -> Vec<u8> {
        let mut buffer = self.rlp_internal();
        self.y_parity.encode(&mut buffer);
        self.r.encode(&mut buffer);
        self.s.encode(&mut buffer);

        let aa = alloy_rlp::Header {
            list: true,
            payload_length: buffer.len(),
        };
        let mut new_buffer = Vec::<u8>::new();
        new_buffer.push(0x02);
        aa.encode(&mut new_buffer);
        new_buffer.append(&mut buffer);
        new_buffer
    }
}

#[derive(Debug)]
struct Eip7702Transaction {
    chain_id: u64,
    nonce: U256,
    max_priority_fee_per_gas: U256,
    max_fee_per_gas: U256,
    gas_limit: U256,
    to: Option<Address>,
    value: U256,
    data: Vec<u8>,
    #[allow(dead_code)]
    access_list: Vec<(Address, Vec<U256>)>, // or a custom struct
    authorization_list: Vec<Authorization7702>,

    // Signature
    y_parity: u8, // 0 or 1
    r: U256,
    s: U256,
}

impl Eip7702Transaction {
    fn rlp_internal(&self) -> Vec<u8> {
        let mut buffer = Vec::<u8>::new();

        self.chain_id.encode(&mut buffer);
        self.nonce.encode(&mut buffer);
        self.max_priority_fee_per_gas.encode(&mut buffer);
        self.max_fee_per_gas.encode(&mut buffer);
        self.gas_limit.encode(&mut buffer);

        // If `to` is `None`, encode as empty bytes
        match self.to {
            Some(to_addr) => to_addr.encode(&mut buffer),
            None => (&[] as &[u8]).encode(&mut buffer),
        };
        self.value.encode(&mut buffer);
        self.data.as_slice().encode(&mut buffer);

        // access list - TODO.
        let aa1 = alloy_rlp::Header {
            list: true,
            payload_length: 0,
        };
        aa1.encode(&mut buffer);

        // authorization list - TODO.
        //let aa1 = alloy_rlp::Header {
        //    list: true,
        //    payload_length: 0,
        //};
        //aa1.encode(&mut buffer);

        self.authorization_list.encode(&mut buffer);

        buffer
    }

    /// RLP for the *unsigned* portion, which you then keccak256 and sign
    fn rlp_encode_unsigned(&self) -> Vec<u8> {
        let mut buffer = self.rlp_internal();
        let aa = alloy_rlp::Header {
            list: true,
            payload_length: buffer.len(),
        };
        let mut new_buffer = Vec::<u8>::new();
        // this is crucial here.
        new_buffer.push(0x04);

        aa.encode(&mut new_buffer);
        new_buffer.append(&mut buffer);
        new_buffer
    }

    fn rlp_encode_signed(&self) -> Vec<u8> {
        let mut buffer = self.rlp_internal();
        self.y_parity.encode(&mut buffer);
        self.r.encode(&mut buffer);
        self.s.encode(&mut buffer);

        let aa = alloy_rlp::Header {
            list: true,
            payload_length: buffer.len(),
        };
        let mut new_buffer = Vec::<u8>::new();
        new_buffer.push(0x04);
        aa.encode(&mut new_buffer);
        new_buffer.append(&mut buffer);
        new_buffer
    }
}

#[derive(Debug)]
struct Authorization7702 {
    chain_id: u64,
    address: Address,
    nonce: U256,
    y_parity: u8, // 0 or 1
    r: U256,
    s: U256,
}

impl Authorization7702 {
    pub fn new(chain_id: u64, address: Address, nonce: U256, private_key_hex: String) -> Self {
        let mut buffer = Vec::<u8>::new();
        chain_id.encode(&mut buffer);
        address.encode(&mut buffer);
        nonce.encode(&mut buffer);
        let aa = alloy_rlp::Header {
            list: true,
            payload_length: buffer.len(),
        };
        let mut new_buffer = Vec::<u8>::new();
        // this is crucial here - this is 'MAGIC' part.
        new_buffer.push(0x05);

        aa.encode(&mut new_buffer);
        new_buffer.append(&mut buffer);

        let message_hash = keccak256(&new_buffer);
        let msg = secp256k1::Message::from_digest_slice(&message_hash.as_slice()).unwrap();
        let secret_key =
            SecretKey::from_slice(&hex_decode(private_key_hex.trim_start_matches("0x")).unwrap())
                .expect("invalid private key bytes");
        let signature = Secp256k1::new().sign_ecdsa_recoverable(&msg, &secret_key);

        let (recovery_id, rsig) = signature.serialize_compact();

        let r = bytes32_to_u256(&rsig[..32]);
        let s = bytes32_to_u256(&rsig[32..64]);
        let y_parity = recovery_id.to_i32() as u8; // 0 or 1
        Self {
            chain_id,
            address,
            nonce,
            y_parity,
            r,
            s,
        }
    }
}

impl Encodable for Authorization7702 {
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        let mut buffer = Vec::<u8>::new();

        self.chain_id.encode(&mut buffer);
        self.address.encode(&mut buffer);
        self.nonce.encode(&mut buffer);
        self.y_parity.encode(&mut buffer);
        self.r.encode(&mut buffer);
        self.s.encode(&mut buffer);
        let aa = alloy_rlp::Header {
            list: true,
            payload_length: buffer.len(),
        };
        aa.encode(out);
        out.put_slice(&buffer);
    }
}

fn bytes32_to_u256(bytes: &[u8]) -> U256 {
    U256::from_be_bytes::<32>(bytes.try_into().expect("slice must be 32 bytes"))
}

pub async fn get_nonce(url: &str, addr: Address) -> U256 {
    let client = Client::new();

    let params = [format!("0x{:x}", addr), "latest".to_string()];
    let req = JsonRpcRequest {
        jsonrpc: "2.0",
        method: "eth_getTransactionCount",
        params: &params,
        id: 1,
    };
    let resp: JsonRpcResponse<String> = client
        .post(url)
        .json(&req)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let nonce_hex = resp
        .result
        .ok_or("No result from getTransactionCount")
        .unwrap();
    let nonce_value =
        U256::from_str_radix(nonce_hex.trim_start_matches("0x"), 16).unwrap_or_default();

    println!("Nonce: {}", nonce_value);
    nonce_value
}

pub fn address_from_pkey(private_key_hex: &str) -> Address {
    let pk_nostrip = private_key_hex.trim_start_matches("0x");
    let pk_bytes = hex_decode(pk_nostrip).unwrap();
    let secret_key = SecretKey::from_slice(&pk_bytes).expect("invalid private key bytes");

    let pubkey = secret_key.public_key(&Secp256k1::new());

    let pubkey_uncompressed = pubkey.serialize_uncompressed(); // 65 bytes, [0x04, x, y]

    let hash = keccak256(&pubkey_uncompressed[1..]); // skip the 0x04
    let from_addr = Address::from_slice(&hash[12..]); // last 20 bytes
    from_addr
}

#[derive(clap::Parser)]
pub struct Args {
    #[arg(short, long)]
    tx_type: String,

    #[arg(short, long)]
    delegate_to: Option<String>,
}

#[main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    // ------------------------------------------------
    // 1. Parse the private key from hex
    // ------------------------------------------------
    let private_key_hex = "0x0fad2ca996a24d116097c481c27a59652a3d3611dfed64d8f9bf86568b1f431d";
    let pk_nostrip = private_key_hex.trim_start_matches("0x");
    let pk_bytes = hex_decode(pk_nostrip).unwrap();
    let secret_key = SecretKey::from_slice(&pk_bytes).expect("invalid private key bytes");

    let from_addr = address_from_pkey(private_key_hex);
    println!("From Address: 0x{:x}", from_addr);

    // 3. Get nonce from local Geth (http://localhost:8545)
    let client = Client::new();
    let url = "http://127.0.0.1:8848";

    let nonce_value = get_nonce(url, from_addr).await;

    let tx_type = args.tx_type;
    let chain_id = 1337;

    let raw_tx_hex = if tx_type == "legacy" {
        let mut tx = LegacyTransaction {
            nonce: nonce_value,
            gas_price: U256::from(1_000_000_000u64), // 1 gwei
            gas_limit: U256::from(21000u64),
            to: Some(Address::from_slice(
                &hex_decode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap(),
            )),
            value: U256::from(1_000_000_000_000_000_000u64), // 1 ETH in wei
            data: vec![],
            v: 0,
            r: U256::ZERO,
            s: U256::ZERO,
        };

        // 5. Sign (EIP-155 Legacy)
        let unsigned_rlp = tx.rlp_encode_unsigned(chain_id);
        let message_hash = keccak256(&unsigned_rlp);

        let msg = secp256k1::Message::from_digest_slice(&message_hash.as_slice())?;
        let signature = Secp256k1::new().sign_ecdsa_recoverable(&msg, &secret_key);

        let (recovery_id, rsig) = signature.serialize_compact();
        let rid = recovery_id.to_i32() as u64; // 0 or 1
        tx.r = bytes32_to_u256(&rsig[0..32]);
        tx.s = bytes32_to_u256(&rsig[32..64]);
        // EIP-155 => v = rid + 2 * chain_id + 35
        tx.v = rid + (2 * chain_id) + 35;

        // 6. RLP-encode and send
        let signed_tx_rlp = tx.rlp_encode_signed();
        let raw_tx_hex = format!("0x{}", hex::encode(signed_tx_rlp));
        println!("Raw signed TX: {}", raw_tx_hex);
        raw_tx_hex
    } else if tx_type == "1559" {
        let mut tx = Eip1559Transaction {
            chain_id: chain_id,
            nonce: nonce_value,
            max_priority_fee_per_gas: U256::from(1_000_000_000u64),
            max_fee_per_gas: U256::from(1_000_000_000u64),
            gas_limit: U256::from(21000u64),
            to: Some(Address::from_slice(
                &hex_decode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaabb").unwrap(),
            )),
            value: U256::from(1_000_000_000_000_000_000u64), // 1 ETH in wei,
            data: vec![],
            access_list: vec![],
            y_parity: 0,
            r: U256::ZERO,
            s: U256::ZERO,
        };
        let unsigned_rlp = tx.rlp_encode_unsigned();
        let message_hash = keccak256(&unsigned_rlp);
        let msg = secp256k1::Message::from_digest_slice(&message_hash.as_slice())?;
        let signature = Secp256k1::new().sign_ecdsa_recoverable(&msg, &secret_key);

        let (recovery_id, rsig) = signature.serialize_compact();
        tx.r = bytes32_to_u256(&rsig[..32]);
        tx.s = bytes32_to_u256(&rsig[32..64]);
        tx.y_parity = recovery_id.to_i32() as u8; // 0 or 1
        let signed_bytes = tx.rlp_encode_signed();
        let raw_tx_hex = format!("0x{}", hex::encode(signed_bytes));
        println!("Raw signed TX: {}", raw_tx_hex);
        raw_tx_hex
    } else if tx_type == "7702" {
        let second_pkey = "0x411bdd63dc116ba53e0e3fbe752ba21f869e272d4f544c8d545c617ce43f654e";
        let second_address = address_from_pkey(&second_pkey);
        let second_nonce = get_nonce(url, second_address).await;

        let delegate_to = Address::from_hex(
            args.delegate_to
                .unwrap_or("0x4F747741EF10551969F9688a8264FC6bb337fA5f".to_string()),
        )
        .unwrap();

        // Authorization is 'type 5' object.
        let authorization = Authorization7702::new(
            // for all chains.
            0,
            delegate_to, // deployed counter contract.
            second_nonce,
            second_pkey.to_string(),
        );

        // That is put into type '4' transaction.
        let mut tx = Eip7702Transaction {
            chain_id,
            nonce: nonce_value,
            max_priority_fee_per_gas: U256::from(1_000_000_000u64),
            max_fee_per_gas: U256::from(1_000_000_000u64),
            // more gas.
            gas_limit: U256::from(46000u64),
            // It doesn't matter who is the target of this transaction.
            to: Some(Address::from_slice(
                &hex_decode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaabb").unwrap(),
            )),
            value: U256::from(1_000_000_000_000_000_000u64), // 1 ETH in wei,
            data: vec![],
            access_list: vec![],
            authorization_list: vec![authorization],
            y_parity: 0,
            r: U256::ZERO,
            s: U256::ZERO,
        };
        let unsigned_rlp = tx.rlp_encode_unsigned();
        let message_hash = keccak256(&unsigned_rlp);
        let msg = secp256k1::Message::from_digest_slice(&message_hash.as_slice())?;
        let signature = Secp256k1::new().sign_ecdsa_recoverable(&msg, &secret_key);

        let (recovery_id, rsig) = signature.serialize_compact();
        tx.r = bytes32_to_u256(&rsig[..32]);
        tx.s = bytes32_to_u256(&rsig[32..64]);
        tx.y_parity = recovery_id.to_i32() as u8; // 0 or 1
        let signed_bytes = tx.rlp_encode_signed();
        let raw_tx_hex = format!("0x{}", hex::encode(signed_bytes));
        println!("Raw signed TX: {}", raw_tx_hex);
        raw_tx_hex
    } else {
        panic!("bad");
    };

    let send_params = [raw_tx_hex];
    let send_req = JsonRpcRequest {
        jsonrpc: "2.0",
        method: "eth_sendRawTransaction",
        params: &send_params,
        id: 2,
    };
    let send_resp: JsonRpcResponse<String> = client
        .post(url)
        .json(&send_req)
        .send()
        .await?
        .json()
        .await?;

    match send_resp.result {
        Some(tx_hash) => println!("TX submitted! Hash: {tx_hash}"),
        None => println!("Error: {:?}", send_resp.error),
    }

    Ok(())
}
