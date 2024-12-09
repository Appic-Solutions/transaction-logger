use candid::{CandidType, Nat, Principal};
use ic_ethereum_types::Address;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::DefaultMemoryImpl;
use ic_stable_structures::{storable::Bound, BTreeMap, Storable};
use num_traits::ToPrimitive;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cell::RefCell;
use storage_config::{
    evm_to_icp_memory, icp_to_evm_memory, minter_memory, supported_appic_tokens_memory_id,
    supported_ckerc20_tokens_memory_id,
};

use std::str::FromStr;

use crate::endpoints::{
    AddEvmToIcpTx, AddIcpToEvmTx, CandidEvmToIcp, CandidIcpToEvm, MinterArgs, TokenPair,
    Transaction,
};
use crate::numeric::{BlockNumber, Erc20TokenAmount, LedgerBurnIndex};
use crate::scrape_events::NATIVE_ERC20_ADDRESS;

use std::fmt::Debug;

use crate::minter_clinet::appic_minter_types::events::{TransactionReceipt, TransactionStatus};

#[derive(
    Clone, Copy, CandidType, PartialEq, PartialOrd, Eq, Ord, Debug, Deserialize, Serialize,
)]
pub enum Oprator {
    DfinityCkEthMinter,
    AppicMinter,
}

#[derive(Clone, PartialEq, Ord, PartialOrd, Eq, Debug, Deserialize, Serialize)]
pub struct Minter {
    pub id: Principal,
    pub last_observed_event: u64,
    pub last_scraped_event: u64,
    pub oprator: Oprator,
    pub evm_to_icp_fee: Erc20TokenAmount,
    pub icp_to_evm_fee: Erc20TokenAmount,
    pub chain_id: ChainId,
}

impl Minter {
    pub fn update_last_observed_event(&mut self, event: u64) {
        self.last_observed_event = event
    }

    pub fn update_last_scraped_event(&mut self, event: u64) {
        self.last_scraped_event = event
    }

    pub fn from_minter_args(args: MinterArgs) -> Self {
        let MinterArgs {
            chain_id,
            minter_id,
            oprator,
            last_observed_event,
            last_scraped_event,
            evm_to_icp_fee,
            icp_to_evm_fee,
        } = args;
        Self {
            id: minter_id,
            last_observed_event: nat_to_u64(&last_observed_event),
            last_scraped_event: nat_to_u64(&last_scraped_event),
            oprator,
            evm_to_icp_fee: Erc20TokenAmount::try_from(evm_to_icp_fee)
                .expect("Should not fail converting fees"),
            icp_to_evm_fee: Erc20TokenAmount::try_from(icp_to_evm_fee)
                .expect("Should not fail converting fees"),
            chain_id: ChainId::from(&chain_id),
        }
    }
}

#[derive(Clone, PartialEq, Ord, Eq, PartialOrd, Debug, Deserialize, Serialize)]
pub struct MinterKey(pub ChainId, pub Oprator);

impl MinterKey {
    pub fn oprator(&self) -> Oprator {
        self.1
    }

    pub fn chain_id(&self) -> ChainId {
        self.0
    }
}

impl From<&Minter> for MinterKey {
    fn from(value: &Minter) -> Self {
        Self(value.chain_id, value.oprator)
    }
}

type TransactionHash = String;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Deserialize, Serialize)]
pub struct EvmToIcpTxIdentifier(TransactionHash, ChainId);

impl EvmToIcpTxIdentifier {
    /// Creates a new `EvmToIcpTxIdentifier` instance.
    pub fn new(transaction_hash: &TransactionHash, chain_id: ChainId) -> Self {
        Self(transaction_hash.clone(), chain_id)
    }
}

impl From<&AddEvmToIcpTx> for EvmToIcpTxIdentifier {
    fn from(value: &AddEvmToIcpTx) -> Self {
        Self::new(&value.transaction_hash, ChainId::from(&value.chain_id))
    }
}

#[derive(Clone, CandidType, PartialEq, Ord, Eq, PartialOrd, Debug, Deserialize, Serialize)]
pub enum EvmToIcpStatus {
    PendingVerification,
    Accepted,
    Minted,
    Invalid(String),
    Quarantined,
}

#[derive(Clone, PartialEq, Ord, Eq, PartialOrd, Debug, Deserialize, Serialize)]
pub struct EvmToIcpTx {
    pub from_address: Address,
    pub transaction_hash: TransactionHash,
    pub value: Erc20TokenAmount,
    pub block_number: Option<BlockNumber>,
    pub actual_received: Option<Erc20TokenAmount>,
    pub principal: Principal,
    pub subaccount: Option<[u8; 32]>,
    pub chain_id: ChainId,
    pub total_gas_spent: Option<Erc20TokenAmount>,
    pub erc20_contract_address: Address,
    pub icrc_ledger_id: Option<Principal>,
    pub status: EvmToIcpStatus,
    pub verified: bool,
    pub time: u64,
    pub oprator: Oprator,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Deserialize, Serialize)]
pub struct IcpToEvmIdentifier(LedgerBurnIndex, ChainId);

impl IcpToEvmIdentifier {
    /// Creates a new `IcpToEvmIdentifier` instance.
    pub fn new(ledger_burn_index: LedgerBurnIndex, chain_id: ChainId) -> Self {
        Self(ledger_burn_index, chain_id)
    }
}

impl From<&AddIcpToEvmTx> for IcpToEvmIdentifier {
    fn from(value: &AddIcpToEvmTx) -> Self {
        let ledger_burn_index = LedgerBurnIndex::new(nat_to_u64(&value.native_ledger_burn_index));
        let chain_id = ChainId::from(&value.chain_id);
        Self::new(ledger_burn_index, chain_id)
    }
}

#[derive(CandidType, Clone, PartialEq, Ord, Eq, PartialOrd, Debug, Deserialize, Serialize)]
pub enum IcpToEvmStatus {
    PendingVerification,
    Accepted,
    Created,
    SignedTransaction,
    FinalizedTransaction,
    ReplacedTransaction,
    Reimbursed,
    QuarantinedReimbursement,
    Successful,
    Failed,
}

#[derive(Clone, PartialEq, Ord, Eq, PartialOrd, Debug, Deserialize, Serialize)]
pub struct IcpToEvmTx {
    pub transaction_hash: Option<TransactionHash>,
    pub native_ledger_burn_index: LedgerBurnIndex,
    pub withdrawal_amount: Erc20TokenAmount,
    pub actual_received: Option<Erc20TokenAmount>,
    pub destination: Address,
    pub from: Principal,
    pub chain_id: ChainId,
    pub from_subaccount: Option<[u8; 32]>,
    pub time: u64,
    pub max_transaction_fee: Option<Erc20TokenAmount>,
    pub effective_gas_price: Option<Erc20TokenAmount>,
    pub gas_used: Option<Erc20TokenAmount>,
    pub total_gas_spent: Option<Erc20TokenAmount>,
    pub erc20_ledger_burn_index: Option<LedgerBurnIndex>,
    pub erc20_contract_address: Address,
    pub icrc_ledger_id: Option<Principal>,
    pub verified: bool,
    pub status: IcpToEvmStatus,
    pub oprator: Oprator,
}

#[derive(Clone, PartialEq, Ord, Eq, PartialOrd, Debug, Deserialize, Serialize)]
pub struct Erc20Identifier(pub Address, pub ChainId);

impl Erc20Identifier {
    pub fn new(contract: &Address, chain_id: ChainId) -> Self {
        Self(*contract, chain_id)
    }

    pub fn erc20_address(&self) -> Address {
        self.0
    }
    pub fn chain_id(&self) -> ChainId {
        self.1
    }
}
// State Definition,
// All types of transactions will be sotred in this stable state
pub struct State {
    // List of all minters including (cketh dfinity and appic minters)
    pub minters: BTreeMap<MinterKey, Minter, StableMemory>,

    // List of all evm_to_icp transactions
    pub evm_to_icp_txs: BTreeMap<EvmToIcpTxIdentifier, EvmToIcpTx, StableMemory>,

    // list of all icp_to_evm transactions
    pub icp_to_evm_txs: BTreeMap<IcpToEvmIdentifier, IcpToEvmTx, StableMemory>,

    pub supported_ckerc20_tokens: BTreeMap<Erc20Identifier, Principal, StableMemory>,
    pub supported_twin_appic_tokens: BTreeMap<Erc20Identifier, Principal, StableMemory>,
}

impl State {
    pub fn update_minter_fees(
        &mut self,
        minter_key: &MinterKey,
        evm_to_icp_fee: Erc20TokenAmount,
        icp_to_evm_fee: Erc20TokenAmount,
    ) {
        if let Some(minter) = self.minters.get(minter_key) {
            let new_minter = Minter {
                evm_to_icp_fee,
                icp_to_evm_fee,
                ..minter
            };
            self.record_minter(new_minter);
        }
    }

    pub fn update_last_observed_event(&mut self, minter_key: &MinterKey, last_observed_event: u64) {
        if let Some(minter) = self.minters.get(minter_key) {
            let new_minter = Minter {
                last_observed_event,
                ..minter
            };
            self.record_minter(new_minter);
        }
    }

    pub fn update_last_scraped_event(&mut self, minter_key: &MinterKey, last_scraped_event: u64) {
        if let Some(minter) = self.minters.get(minter_key) {
            let new_minter = Minter {
                last_scraped_event,
                ..minter
            };
            self.record_minter(new_minter);
        }
    }

    pub fn get_minters(&self) -> Vec<(MinterKey, Minter)> {
        self.minters.iter().collect()
    }

    pub fn if_chain_id_exists(&self, chain_id: ChainId) -> bool {
        for (_minter_key, minter) in self.get_minters() {
            if minter.chain_id == chain_id {
                return true;
            }
        }
        false
    }

    pub fn record_minter(&mut self, minter: Minter) {
        self.minters.insert(MinterKey::from(&minter), minter);
    }

    pub fn get_icrc_twin_for_erc20(
        &self,
        erc20_identifier: &Erc20Identifier,
        oprator: &Oprator,
    ) -> Option<Principal> {
        match oprator {
            Oprator::AppicMinter => self
                .supported_twin_appic_tokens
                .get(erc20_identifier)
                .map(|token_principal| token_principal),
            Oprator::DfinityCkEthMinter => self
                .supported_ckerc20_tokens
                .get(erc20_identifier)
                .map(|token_principal| token_principal),
        }
    }

    pub fn if_evm_to_icp_tx_exists(&self, identifier: &EvmToIcpTxIdentifier) -> bool {
        self.evm_to_icp_txs.get(identifier).is_some()
    }

    pub fn if_icp_to_evm_tx_exists(&self, identifier: &IcpToEvmIdentifier) -> bool {
        self.icp_to_evm_txs.get(identifier).is_some()
    }

    pub fn record_new_evm_to_icp(&mut self, identifier: EvmToIcpTxIdentifier, tx: EvmToIcpTx) {
        self.evm_to_icp_txs.insert(identifier, tx);
    }

    pub fn record_accepted_evm_to_icp(
        &mut self,
        identifier: EvmToIcpTxIdentifier,
        transaction_hash: TransactionHash,
        block_number: Nat,
        from_address: String,
        value: Nat,
        principal: Principal,
        erc20_contract_address: String,
        subaccount: Option<[u8; 32]>,
        chain_id: ChainId,
        oprator: Oprator,
        timestamp: u64,
    ) {
        // Parse addresses once
        let parsed_from_address = Address::from_str(&from_address)
            .expect("Should not fail converting from_address to Address");
        let parsed_erc20_address = Address::from_str(&erc20_contract_address)
            .expect("Should not fail converting erc20_contract_address to Address");

        if let Some(tx) = self.evm_to_icp_txs.get(&identifier) {
            // Update only the necessary fields in the existing transaction
            let new_tx = EvmToIcpTx {
                verified: true,
                block_number: Some(nat_to_block_number(block_number)),
                from_address: parsed_from_address,
                value: nat_to_erc20_amount(value),
                principal,
                erc20_contract_address: parsed_erc20_address,
                subaccount,
                status: EvmToIcpStatus::Accepted,
                ..tx
            };
            self.record_new_evm_to_icp(identifier, new_tx);
        } else {
            // Create a new transaction only if one doses not already exist
            let new_tx = EvmToIcpTx {
                from_address: parsed_from_address,
                transaction_hash,
                value: nat_to_erc20_amount(value),
                block_number: Some(nat_to_block_number(block_number)),
                actual_received: None,
                principal,
                subaccount,
                chain_id,
                total_gas_spent: None,
                erc20_contract_address: parsed_erc20_address,
                icrc_ledger_id: self.get_icrc_twin_for_erc20(
                    &Erc20Identifier(parsed_erc20_address, chain_id),
                    &oprator,
                ),
                status: EvmToIcpStatus::Accepted,
                verified: true,
                time: timestamp,
                oprator,
            };

            self.record_new_evm_to_icp(identifier, new_tx);
        }
    }

    pub fn record_minted_evm_to_icp(
        &mut self,
        identifier: EvmToIcpTxIdentifier,
        evm_to_icp_fee: Erc20TokenAmount,
    ) {
        if let Some(tx) = self.evm_to_icp_txs.get(&identifier) {
            // Fee calculation
            let actual_received = if is_native_token(&tx.erc20_contract_address) {
                Some(tx.value.checked_sub(evm_to_icp_fee).unwrap_or(tx.value))
            } else {
                Some(tx.value)
            };

            // Transaction update
            let new_tx = EvmToIcpTx {
                actual_received,
                status: EvmToIcpStatus::Minted,
                ..tx
            };
            self.record_new_evm_to_icp(identifier, new_tx);
        }
    }

    pub fn record_invalid_evm_to_icp(&mut self, identifier: EvmToIcpTxIdentifier, reason: String) {
        if let Some(tx) = self.evm_to_icp_txs.get(&identifier) {
            let new_tx = EvmToIcpTx {
                status: EvmToIcpStatus::Invalid(reason),
                ..tx
            };
            self.record_new_evm_to_icp(identifier, new_tx);
        }
    }

    pub fn record_quarantined_evm_to_icp(&mut self, identifier: EvmToIcpTxIdentifier) {
        if let Some(tx) = self.evm_to_icp_txs.get(&identifier) {
            let new_tx = EvmToIcpTx {
                status: EvmToIcpStatus::Quarantined,
                ..tx
            };
            self.record_new_evm_to_icp(identifier, new_tx);
        }
    }

    pub fn record_new_icp_to_evm(&mut self, identifier: IcpToEvmIdentifier, tx: IcpToEvmTx) {
        self.icp_to_evm_txs.insert(identifier, tx);
    }

    pub fn record_accepted_icp_to_evm(
        &mut self,
        identifier: IcpToEvmIdentifier,
        max_transaction_fee: Option<Nat>,
        withdrawal_amount: Nat,
        erc20_contract_address: String,
        destination: String,
        native_ledger_burn_index: Nat,
        erc20_ledger_burn_index: Option<Nat>,
        from: Principal,
        from_subaccount: Option<[u8; 32]>,
        created_at: Option<u64>,
        oprator: Oprator,
        chain_id: ChainId,
        timestamp: u64,
    ) {
        let destination_address = Address::from_str(&destination)
            .expect("Should not fail converting destination to Address");
        let erc20_address = Address::from_str(&erc20_contract_address)
            .expect("Should not fail converting ERC20 contract address to Address");
        let max_transaction_fee = max_transaction_fee.map(|max_fee| nat_to_erc20_amount(max_fee));

        let withdrawal_amount = nat_to_erc20_amount(withdrawal_amount);

        let native_ledger_burn_index = LedgerBurnIndex::new(nat_to_u64(&native_ledger_burn_index));

        let erc20_ledger_burn_index =
            erc20_ledger_burn_index.map(|burn_index| LedgerBurnIndex::new(nat_to_u64(&burn_index)));

        if let Some(tx) = self.icp_to_evm_txs.get(&identifier) {
            let new_tx = IcpToEvmTx {
                verified: true,
                max_transaction_fee,
                withdrawal_amount,
                erc20_contract_address: erc20_address,
                destination: destination_address,
                native_ledger_burn_index,
                erc20_ledger_burn_index,
                from,
                from_subaccount,
                status: IcpToEvmStatus::Accepted,
                ..tx
            };

            self.record_new_icp_to_evm(identifier, new_tx);
        } else {
            let icrc_ledger_id =
                self.get_icrc_twin_for_erc20(&Erc20Identifier(erc20_address, chain_id), &oprator);

            let new_tx = IcpToEvmTx {
                native_ledger_burn_index,
                withdrawal_amount,
                actual_received: None,
                destination: destination_address,
                from,
                from_subaccount,
                time: created_at.unwrap_or(timestamp),
                max_transaction_fee,
                erc20_ledger_burn_index,
                icrc_ledger_id,
                chain_id,
                erc20_contract_address: erc20_address,
                verified: true,
                status: IcpToEvmStatus::Accepted,
                oprator,
                effective_gas_price: None,
                gas_used: None,
                transaction_hash: None,
                total_gas_spent: None,
            };

            self.record_new_icp_to_evm(identifier, new_tx);
        }
    }

    pub fn record_created_icp_to_evm(&mut self, identifier: IcpToEvmIdentifier) {
        if let Some(tx) = self.icp_to_evm_txs.get(&identifier) {
            let new_tx = IcpToEvmTx {
                status: IcpToEvmStatus::Created,
                ..tx
            };
            self.record_new_icp_to_evm(identifier, new_tx);
        }
    }

    pub fn record_signed_icp_to_evm(&mut self, identifier: IcpToEvmIdentifier) {
        if let Some(tx) = self.icp_to_evm_txs.get(&identifier) {
            let new_tx = IcpToEvmTx {
                status: IcpToEvmStatus::SignedTransaction,
                ..tx
            };
            self.record_new_icp_to_evm(identifier, new_tx);
        }
    }

    pub fn record_replaced_icp_to_evm(&mut self, identifier: IcpToEvmIdentifier) {
        if let Some(tx) = self.icp_to_evm_txs.get(&identifier) {
            let new_tx = IcpToEvmTx {
                status: IcpToEvmStatus::ReplacedTransaction,
                ..tx
            };
            self.record_new_icp_to_evm(identifier, new_tx);
        }
    }

    pub fn record_finalized_icp_to_evm(
        &mut self,
        identifier: IcpToEvmIdentifier,
        receipt: TransactionReceipt,
        icp_to_evm_fee: Erc20TokenAmount,
    ) {
        if let Some(tx) = self.icp_to_evm_txs.get(&identifier) {
            let gas_used = nat_to_erc20_amount(receipt.gas_used);
            let effective_gas_price = nat_to_erc20_amount(receipt.effective_gas_price);

            let total_gas_spent = gas_used
                .checked_mul(effective_gas_price)
                .unwrap()
                .checked_add(icp_to_evm_fee)
                .unwrap();

            let actual_received = if is_native_token(&tx.erc20_contract_address) {
                tx.withdrawal_amount.checked_sub(total_gas_spent)
            } else {
                Some(tx.withdrawal_amount)
            };

            let status = match receipt.status {
                TransactionStatus::Success => IcpToEvmStatus::Successful,
                TransactionStatus::Failure => IcpToEvmStatus::Failed,
            };
            let new_tx = IcpToEvmTx {
                actual_received,
                transaction_hash: Some(receipt.transaction_hash),
                gas_used: Some(gas_used),
                effective_gas_price: Some(effective_gas_price),
                total_gas_spent: Some(total_gas_spent),
                status,
                ..tx
            };
            self.record_new_icp_to_evm(identifier, new_tx);
        }
    }

    pub fn record_reimbursed_icp_to_evm(&mut self, identifier: IcpToEvmIdentifier) {
        if let Some(tx) = self.icp_to_evm_txs.get(&identifier) {
            let new_tx = IcpToEvmTx {
                status: IcpToEvmStatus::Reimbursed,
                ..tx
            };
            self.record_new_icp_to_evm(identifier, new_tx);
        }
    }

    pub fn record_quarantined_reimbursed_icp_to_evm(&mut self, identifier: IcpToEvmIdentifier) {
        if let Some(tx) = self.icp_to_evm_txs.get(&identifier) {
            let new_tx = IcpToEvmTx {
                status: IcpToEvmStatus::QuarantinedReimbursement,
                ..tx
            };
            self.record_new_icp_to_evm(identifier, new_tx);
        }
    }

    pub fn all_unverified_icp_to_evm(&self) -> Vec<(IcpToEvmIdentifier, u64)> {
        self.icp_to_evm_txs
            .iter()
            .filter(|(_, tx)| !tx.verified) // Filter out verified transactions
            .map(|(identifier, tx)| (identifier, tx.time)) // Map to the desired tuple
            .collect()
    }

    pub fn remove_unverified_icp_to_evm(&mut self, identifier: &IcpToEvmIdentifier) {
        self.icp_to_evm_txs.remove(identifier);
    }

    pub fn all_unverified_evm_to_icp(&self) -> Vec<(EvmToIcpTxIdentifier, u64)> {
        self.evm_to_icp_txs
            .iter()
            .filter(|(_, tx)| !tx.verified) // Filter out verified transactions
            .map(|(identifier, tx)| (identifier, tx.time)) // Map to the desired tuple
            .collect()
    }

    pub fn remove_unverified_evm_to_icp(&mut self, identifier: &EvmToIcpTxIdentifier) {
        self.evm_to_icp_txs.remove(identifier);
    }

    pub fn get_transaction_for_address(&self, address: Address) -> Vec<Transaction> {
        let result: Vec<Transaction> = self
            .evm_to_icp_txs
            .iter()
            .filter(|(_id, tx)| tx.from_address == address)
            .map(|(_id, tx)| Transaction::from(CandidEvmToIcp::from(tx)))
            .chain(
                self.icp_to_evm_txs
                    .iter()
                    .filter(|(_id, tx)| tx.destination == address)
                    .map(|(_id, tx)| Transaction::from(CandidIcpToEvm::from(tx))),
            )
            .collect();

        result
    }

    pub fn get_transaction_for_principal(&self, principal_id: Principal) -> Vec<Transaction> {
        let result: Vec<Transaction> = self
            .evm_to_icp_txs
            .iter()
            .filter(|(_id, tx)| tx.principal == principal_id)
            .map(|(_id, tx)| Transaction::from(CandidEvmToIcp::from(tx)))
            .chain(
                self.icp_to_evm_txs
                    .iter()
                    .filter(|(_id, tx)| tx.from == principal_id)
                    .map(|(_id, tx)| Transaction::from(CandidIcpToEvm::from(tx))),
            )
            .collect();

        result
    }

    pub fn get_suported_twin_token_pairs(&self) -> Vec<TokenPair> {
        self.supported_ckerc20_tokens
            .iter()
            .map(|(erc20_identifier, ledger_id)| TokenPair {
                erc20_address: erc20_identifier.erc20_address().to_string(),
                ledger_id,
                oprator: Oprator::DfinityCkEthMinter,
                chain_id: erc20_identifier.chain_id().into(),
            })
            .chain(
                self.supported_twin_appic_tokens
                    .iter()
                    .map(|(erc20_identifier, ledger_id)| TokenPair {
                        erc20_address: erc20_identifier.erc20_address().to_string(),
                        ledger_id,
                        oprator: Oprator::AppicMinter,
                        chain_id: erc20_identifier.chain_id().into(),
                    }),
            )
            .collect()
    }
}

pub fn is_native_token(address: &Address) -> bool {
    address
        == &Address::from_str(NATIVE_ERC20_ADDRESS).expect("Should not fail converintg to address")
}

impl From<&Nat> for ChainId {
    fn from(value: &Nat) -> Self {
        Self(value.0.to_u64().unwrap())
    }
}

impl From<ChainId> for Nat {
    fn from(value: ChainId) -> Self {
        Nat::from(value.0)
    }
}

pub fn nat_to_ledger_burn_index(value: &Nat) -> LedgerBurnIndex {
    LedgerBurnIndex::new(nat_to_u64(value))
}

pub fn nat_to_block_number(value: Nat) -> BlockNumber {
    BlockNumber::try_from(value).expect("Failed to convert nat into Erc20TokenAmount")
}

pub fn nat_to_erc20_amount(value: Nat) -> Erc20TokenAmount {
    Erc20TokenAmount::try_from(value).expect("Failed to convert nat into Erc20TokenAmount")
}

pub fn nat_to_u64(value: &Nat) -> u64 {
    value.0.to_u64().unwrap()
}

pub fn nat_to_u128(value: &Nat) -> u128 {
    value.0.to_u128().unwrap()
}

pub fn calculate_actual_icp_to_evm_received() {}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ChainId(pub u64);

impl AsRef<u64> for ChainId {
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

pub fn read_state<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|cell| {
        f(cell
            .borrow()
            .as_ref()
            .expect("BUG: state is not initialized"))
    })
}

// / Mutates (part of) the current state using `f`.
// /
// / Panics if there is no state.
pub fn mutate_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut State) -> R,
{
    STATE.with(|cell| {
        f(cell
            .borrow_mut()
            .as_mut()
            .expect("BUG: state is not initialized"))
    })
}

// State configuration
pub type StableMemory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    pub static STATE: RefCell<Option<State>> = RefCell::new(
        Some(State {
                minters: BTreeMap::init(minter_memory()),
                evm_to_icp_txs: BTreeMap::init(evm_to_icp_memory()),
                icp_to_evm_txs: BTreeMap::init(icp_to_evm_memory()),
                supported_ckerc20_tokens: BTreeMap::init(supported_ckerc20_tokens_memory_id()),
                supported_twin_appic_tokens:BTreeMap::init(supported_appic_tokens_memory_id())
            })
    );
}

mod storage_config {
    use super::*;

    thread_local! {
        static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(
            MemoryManager::init(DefaultMemoryImpl::default())
        );

    }

    const MINTERS_MEMORY_ID: MemoryId = MemoryId::new(0);

    pub fn minter_memory() -> StableMemory {
        MEMORY_MANAGER.with(|m| m.borrow().get(MINTERS_MEMORY_ID))
    }

    const EVM_TO_ICP_MEMORY_ID: MemoryId = MemoryId::new(1);

    pub fn evm_to_icp_memory() -> StableMemory {
        MEMORY_MANAGER.with(|m| m.borrow().get(EVM_TO_ICP_MEMORY_ID))
    }

    const ICP_TO_EVM_MEMORY_ID: MemoryId = MemoryId::new(2);

    pub fn icp_to_evm_memory() -> StableMemory {
        MEMORY_MANAGER.with(|m| m.borrow().get(ICP_TO_EVM_MEMORY_ID))
    }

    const SUPPORTED_CK_MEMORY_ID: MemoryId = MemoryId::new(3);

    pub fn supported_ckerc20_tokens_memory_id() -> StableMemory {
        MEMORY_MANAGER.with(|m| m.borrow().get(SUPPORTED_CK_MEMORY_ID))
    }

    const SUPPORTED_APPIC_MEMORY_ID: MemoryId = MemoryId::new(4);

    pub fn supported_appic_tokens_memory_id() -> StableMemory {
        MEMORY_MANAGER.with(|m| m.borrow().get(SUPPORTED_APPIC_MEMORY_ID))
    }

    impl Storable for MinterKey {
        fn to_bytes(&self) -> Cow<[u8]> {
            encode(self)
        }

        fn from_bytes(bytes: Cow<[u8]>) -> Self {
            decode(bytes)
        }

        const BOUND: Bound = Bound::Unbounded;
    }

    impl Storable for Minter {
        fn to_bytes(&self) -> Cow<[u8]> {
            encode(self)
        }

        fn from_bytes(bytes: Cow<[u8]>) -> Self {
            decode(bytes)
        }

        const BOUND: Bound = Bound::Unbounded;
    }

    impl Storable for EvmToIcpTxIdentifier {
        fn to_bytes(&self) -> Cow<[u8]> {
            encode(self)
        }

        fn from_bytes(bytes: Cow<[u8]>) -> Self {
            decode(bytes)
        }

        const BOUND: Bound = Bound::Unbounded;
    }

    impl Storable for EvmToIcpStatus {
        fn to_bytes(&self) -> Cow<[u8]> {
            encode(self)
        }

        fn from_bytes(bytes: Cow<[u8]>) -> Self {
            decode(bytes)
        }

        const BOUND: Bound = Bound::Unbounded;
    }

    impl Storable for EvmToIcpTx {
        fn to_bytes(&self) -> Cow<[u8]> {
            encode(self)
        }

        fn from_bytes(bytes: Cow<[u8]>) -> Self {
            decode(bytes)
        }

        const BOUND: Bound = Bound::Unbounded;
    }

    impl Storable for IcpToEvmIdentifier {
        fn to_bytes(&self) -> Cow<[u8]> {
            encode(self)
        }

        fn from_bytes(bytes: Cow<[u8]>) -> Self {
            decode(bytes)
        }

        const BOUND: Bound = Bound::Unbounded;
    }

    impl Storable for IcpToEvmStatus {
        fn to_bytes(&self) -> Cow<[u8]> {
            encode(self)
        }

        fn from_bytes(bytes: Cow<[u8]>) -> Self {
            decode(bytes)
        }

        const BOUND: Bound = Bound::Unbounded;
    }

    impl Storable for IcpToEvmTx {
        fn to_bytes(&self) -> Cow<[u8]> {
            encode(self)
        }

        fn from_bytes(bytes: Cow<[u8]>) -> Self {
            decode(bytes)
        }

        const BOUND: Bound = Bound::Unbounded;
    }

    impl Storable for Erc20Identifier {
        fn to_bytes(&self) -> Cow<[u8]> {
            encode(self)
        }

        fn from_bytes(bytes: Cow<[u8]>) -> Self {
            decode(bytes)
        }

        const BOUND: Bound = Bound::Unbounded;
    }

    fn encode<T: ?Sized + serde::Serialize>(value: &T) -> Cow<[u8]> {
        let bytes = bincode::serialize(value).expect("failed to encode");
        Cow::Owned(bytes)
    }

    fn decode<T: for<'a> serde::Deserialize<'a>>(bytes: Cow<[u8]>) -> T {
        bincode::deserialize(bytes.as_ref())
            .unwrap_or_else(|e| panic!("failed to decode bytes {}: {e}", hex::encode(bytes)))
    }
}

// Testing which state serialization is faster
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn compare_bincode_and_ciborium() {
        let tx_identifier: EvmToIcpTxIdentifier = EvmToIcpTxIdentifier(
            "0x00000034125423542452345241254235245".to_string(),
            ChainId(56),
        );

        // Bincode Serialization and Deserialization
        let start = Instant::now();
        let bincode_bytes = bincode::serialize(&tx_identifier).unwrap();
        let bincode_serialization_time = start.elapsed();

        let start = Instant::now();
        let bincode_deserialized: EvmToIcpTxIdentifier =
            bincode::deserialize(&bincode_bytes).unwrap();
        let bincode_deserialization_time = start.elapsed();

        assert_eq!(bincode_deserialized, tx_identifier);

        // Ciborium Serialization and Deserialization
        let start = Instant::now();
        let mut ciborium_buf = Vec::new();
        ciborium::ser::into_writer(&tx_identifier, &mut ciborium_buf)
            .expect("Failed to serialize with Ciborium");
        let ciborium_serialization_time = start.elapsed();

        let start = Instant::now();
        let ciborium_deserialized: EvmToIcpTxIdentifier =
            ciborium::de::from_reader(ciborium_buf.as_slice())
                .expect("Failed to deserialize with Ciborium");
        let ciborium_deserialization_time = start.elapsed();

        assert_eq!(ciborium_deserialized, tx_identifier);

        // Print results
        println!(
            "Bincode - Serialization: {:?}, Deserialization: {:?}",
            bincode_serialization_time, bincode_deserialization_time
        );
        println!(
            "Ciborium - Serialization: {:?}, Deserialization: {:?}",
            ciborium_serialization_time, ciborium_deserialization_time
        );
    }
}
