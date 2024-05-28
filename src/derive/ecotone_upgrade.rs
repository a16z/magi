//! Module containing a [Transaction] builder for the Ecotone network updgrade transactions.
//!
//! [Transaction]: alloy_consensus::Transaction

use crate::common::RawTransaction;
use std::string::String;
use std::vec::Vec;
use alloy_primitives::{address, keccak256, B256, bytes, Address, Bytes, TxKind, U256};
use alloy_rlp::Encodable;
use op_alloy_consensus::{OpTxEnvelope, TxDeposit};
use spin::Lazy;

/// Source domain identifiers for deposit transactions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DepositSourceDomainIdentifier {
    /// A user deposit source.
    User = 0,
    /// A L1 info deposit source.
    L1Info = 1,
    /// An upgrade deposit source.
    Upgrade = 2,
}

/// Source domains for deposit transactions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DepositSourceDomain {
    /// A user deposit source.
    User(UserDepositSource),
    /// A L1 info deposit source.
    L1Info(L1InfoDepositSource),
    /// An upgrade deposit source.
    Upgrade(UpgradeDepositSource),
}

impl DepositSourceDomain {
    /// Returns the source hash.
    pub fn source_hash(&self) -> B256 {
        match self {
            Self::User(ds) => ds.source_hash(),
            Self::L1Info(ds) => ds.source_hash(),
            Self::Upgrade(ds) => ds.source_hash(),
        }
    }
}


/// A deposit transaction source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UserDepositSource {
    /// The L1 block hash.
    pub l1_block_hash: B256,
    /// The log index.
    pub log_index: u64,
}

impl UserDepositSource {
    /// Creates a new [UserDepositSource].
    pub fn new(l1_block_hash: B256, log_index: u64) -> Self {
        Self { l1_block_hash, log_index }
    }

    /// Returns the source hash.
    pub fn source_hash(&self) -> B256 {
        let mut input = [0u8; 32 * 2];
        input[..32].copy_from_slice(&self.l1_block_hash[..]);
        input[32 * 2 - 8..].copy_from_slice(&self.log_index.to_be_bytes());
        let deposit_id_hash = keccak256(input);
        let mut domain_input = [0u8; 32 * 2];
        let identifier_bytes: [u8; 8] = (DepositSourceDomainIdentifier::User as u64).to_be_bytes();
        domain_input[32 - 8..32].copy_from_slice(&identifier_bytes);
        domain_input[32..].copy_from_slice(&deposit_id_hash[..]);
        keccak256(domain_input)
    }
}

/// A L1 info deposit transaction source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct L1InfoDepositSource {
    /// The L1 block hash.
    pub l1_block_hash: B256,
    /// The sequence number.
    pub seq_number: u64,
}

impl L1InfoDepositSource {
    /// Creates a new [L1InfoDepositSource].
    pub fn new(l1_block_hash: B256, seq_number: u64) -> Self {
        Self { l1_block_hash, seq_number }
    }

    /// Returns the source hash.
    pub fn source_hash(&self) -> B256 {
        let mut input = [0u8; 32 * 2];
        input[..32].copy_from_slice(&self.l1_block_hash[..]);
        input[32 * 2 - 8..].copy_from_slice(&self.seq_number.to_be_bytes());
        let deposit_id_hash = keccak256(input);
        let mut domain_input = [0u8; 32 * 2];
        let identifier_bytes: [u8; 8] =
            (DepositSourceDomainIdentifier::L1Info as u64).to_be_bytes();
        domain_input[32 - 8..32].copy_from_slice(&identifier_bytes);
        domain_input[32..].copy_from_slice(&deposit_id_hash[..]);
        keccak256(domain_input)
    }
}

/// An upgrade deposit transaction source.
/// This implements the translation of upgrade-tx identity information to a deposit source-hash,
/// which makes the deposit uniquely identifiable.
/// System-upgrade transactions have their own domain for source-hashes,
/// to not conflict with user-deposits or deposited L1 information.
/// The intent identifies the upgrade-tx uniquely, in a human-readable way.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UpgradeDepositSource {
    /// The intent.
    pub intent: String,
}

impl UpgradeDepositSource {
    /// Creates a new [UpgradeDepositSource].
    pub fn new(intent: String) -> Self {
        Self { intent }
    }

    /// Returns the source hash.
    pub fn source_hash(&self) -> B256 {
        let intent_hash = keccak256(self.intent.as_bytes());
        let mut domain_input = [0u8; 32 * 2];
        let identifier_bytes: [u8; 8] =
            (DepositSourceDomainIdentifier::Upgrade as u64).to_be_bytes();
        domain_input[32 - 8..32].copy_from_slice(&identifier_bytes);
        domain_input[32..].copy_from_slice(&intent_hash[..]);
        keccak256(domain_input)
    }
}

/// The UpdgradeTo Function Signature
pub const UPDGRADE_TO_FUNC_SIGNATURE: &str = "upgradeTo(address)";

/// L1 Block Deployer Address
pub const L1_BLOCK_DEPLOYER_ADDRESS: Address = address!("4210000000000000000000000000000000000000");

/// The Gas Price Oracle Deployer Address
pub const GAS_PRICE_ORACLE_DEPLOYER_ADDRESS: Address =
    address!("4210000000000000000000000000000000000001");

/// The new L1 Block Address
/// This is computed by using go-ethereum's `crypto.CreateAddress` function,
/// with the L1 Block Deployer Address and nonce 0.
pub const NEW_L1_BLOCK_ADDRESS: Address = address!("07dbe8500fc591d1852b76fee44d5a05e13097ff");

/// The Gas Price Oracle Address
/// This is computed by using go-ethereum's `crypto.CreateAddress` function,
/// with the Gas Price Oracle Deployer Address and nonce 0.
pub const GAS_PRICE_ORACLE_ADDRESS: Address = address!("b528d11cc114e026f138fe568744c6d45ce6da7a");

/// The Enable Ecotone Input Method 4Byte Signature
pub const ENABLE_ECOTONE_INPUT: &[u8] = &[0x22, 0xb9, 0x08, 0xb3];

/// UpgradeTo Function 4Byte Signature
pub const UPGRADE_TO_FUNC_BYTES_4: &[u8] = &[0x36, 0x59, 0xcf, 0xe6];

/// EIP-4788 From Address
pub const EIP4788_FROM: Address = address!("0B799C86a49DEeb90402691F1041aa3AF2d3C875");

static DEPLOY_L1_BLOCK_SOURCE: Lazy<UpgradeDepositSource> =
    Lazy::new(|| UpgradeDepositSource { intent: String::from("Ecotone: L1 Block Deployment") });

static DEPLOY_GAS_PRICE_ORACLE_SOURCE: Lazy<UpgradeDepositSource> = Lazy::new(|| {
    UpgradeDepositSource { intent: String::from("Ecotone: Gas Price Oracle Deployment") }
});

static UPDATE_L1_BLOCK_PROXY_SOURCE: Lazy<UpgradeDepositSource> =
    Lazy::new(|| UpgradeDepositSource { intent: String::from("Ecotone: L1 Block Proxy Update") });

static UPDATE_GAS_PRICE_ORACLE_SOURCE: Lazy<UpgradeDepositSource> = Lazy::new(|| {
    UpgradeDepositSource { intent: String::from("Ecotone: Gas Price Oracle Proxy Update") }
});

static ENABLE_ECOTONE_SOURCE: Lazy<UpgradeDepositSource> = Lazy::new(|| UpgradeDepositSource {
    intent: String::from("Ecotone: Gas Price Oracle Set Ecotone"),
});

static BEACON_ROOTS_SOURCE: Lazy<UpgradeDepositSource> = Lazy::new(|| UpgradeDepositSource {
    intent: String::from("Ecotone: beacon block roots contract deployment"),
});

/// Turns the given address into calldata for the `upgradeTo` function.
pub fn upgrade_to_calldata(addr: Address) -> Bytes {
    let mut v = UPGRADE_TO_FUNC_BYTES_4.to_vec();
    v.extend_from_slice(addr.as_slice());
    Bytes::from(v)
}

/// Builder wrapper for the Ecotone network updgrade.
#[derive(Debug, Default)]
pub struct EcotoneTransactionBuilder;

impl EcotoneTransactionBuilder {
    /// Constructs the Ecotone network upgrade transactions.
    pub fn build_txs() -> anyhow::Result<Vec<RawTransaction>> {
        let mut txs = vec![];

        let eip4788_creation_data = bytes!("60618060095f395ff33373fffffffffffffffffffffffffffffffffffffffe14604d57602036146024575f5ffd5b5f35801560495762001fff810690815414603c575f5ffd5b62001fff01545f5260205ff35b5f5ffd5b62001fff42064281555f359062001fff015500");

        let l1_block_deployment_bytecode = bytes!("608060405234801561001057600080fd5b5061053e806100206000396000f3fe608060405234801561001057600080fd5b50600436106100f55760003560e01c80638381f58a11610097578063c598591811610066578063c598591814610229578063e591b28214610249578063e81b2c6d14610289578063f82061401461029257600080fd5b80638381f58a146101e35780638b239f73146101f75780639e8c496614610200578063b80777ea1461020957600080fd5b806354fd4d50116100d357806354fd4d50146101335780635cf249691461017c57806364ca23ef1461018557806368d5dca6146101b257600080fd5b8063015d8eb9146100fa57806309bd5a601461010f578063440a5e201461012b575b600080fd5b61010d61010836600461044c565b61029b565b005b61011860025481565b6040519081526020015b60405180910390f35b61010d6103da565b61016f6040518060400160405280600581526020017f312e322e3000000000000000000000000000000000000000000000000000000081525081565b60405161012291906104be565b61011860015481565b6003546101999067ffffffffffffffff1681565b60405167ffffffffffffffff9091168152602001610122565b6003546101ce9068010000000000000000900463ffffffff1681565b60405163ffffffff9091168152602001610122565b6000546101999067ffffffffffffffff1681565b61011860055481565b61011860065481565b6000546101999068010000000000000000900467ffffffffffffffff1681565b6003546101ce906c01000000000000000000000000900463ffffffff1681565b61026473deaddeaddeaddeaddeaddeaddeaddeaddead000181565b60405173ffffffffffffffffffffffffffffffffffffffff9091168152602001610122565b61011860045481565b61011860075481565b3373deaddeaddeaddeaddeaddeaddeaddeaddead000114610342576040517f08c379a000000000000000000000000000000000000000000000000000000000815260206004820152603b60248201527f4c31426c6f636b3a206f6e6c7920746865206465706f7369746f72206163636f60448201527f756e742063616e20736574204c3120626c6f636b2076616c7565730000000000606482015260840160405180910390fd5b6000805467ffffffffffffffff98891668010000000000000000027fffffffffffffffffffffffffffffffff00000000000000000000000000000000909116998916999099179890981790975560019490945560029290925560038054919094167fffffffffffffffffffffffffffffffffffffffffffffffff00000000000000009190911617909255600491909155600555600655565b3373deaddeaddeaddeaddeaddeaddeaddeaddead00011461040357633cc50b456000526004601cfd5b60043560801c60035560143560801c600055602435600155604435600755606435600255608435600455565b803567ffffffffffffffff8116811461044757600080fd5b919050565b600080600080600080600080610100898b03121561046957600080fd5b6104728961042f565b975061048060208a0161042f565b9650604089013595506060890135945061049c60808a0161042f565b979a969950949793969560a0850135955060c08501359460e001359350915050565b600060208083528351808285015260005b818110156104eb578581018301518582016040015282016104cf565b818111156104fd576000604083870101525b50601f017fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe01692909201604001939250505056fea164736f6c634300080f000a");

        let gas_price_oracle_deployment_bytecode =
        bytes!("608060405234801561001057600080fd5b50610fb5806100206000396000f3fe608060405234801561001057600080fd5b50600436106100f55760003560e01c806354fd4d5011610097578063de26c4a111610066578063de26c4a1146101da578063f45e65d8146101ed578063f8206140146101f5578063fe173b97146101cc57600080fd5b806354fd4d501461016657806368d5dca6146101af5780636ef25c3a146101cc578063c5985918146101d257600080fd5b8063313ce567116100d3578063313ce5671461012757806349948e0e1461012e5780634ef6e22414610141578063519b4bd31461015e57600080fd5b80630c18c162146100fa57806322b90ab3146101155780632e0f26251461011f575b600080fd5b6101026101fd565b6040519081526020015b60405180910390f35b61011d61031e565b005b610102600681565b6006610102565b61010261013c366004610b73565b610541565b60005461014e9060ff1681565b604051901515815260200161010c565b610102610565565b6101a26040518060400160405280600581526020017f312e322e3000000000000000000000000000000000000000000000000000000081525081565b60405161010c9190610c42565b6101b76105c6565b60405163ffffffff909116815260200161010c565b48610102565b6101b761064b565b6101026101e8366004610b73565b6106ac565b610102610760565b610102610853565b6000805460ff1615610296576040517f08c379a000000000000000000000000000000000000000000000000000000000815260206004820152602860248201527f47617350726963654f7261636c653a206f76657268656164282920697320646560448201527f707265636174656400000000000000000000000000000000000000000000000060648201526084015b60405180910390fd5b73420000000000000000000000000000000000001573ffffffffffffffffffffffffffffffffffffffff16638b239f736040518163ffffffff1660e01b8152600401602060405180830381865afa1580156102f5573d6000803e3d6000fd5b505050506040513d601f19601f820116820180604052508101906103199190610cb5565b905090565b73420000000000000000000000000000000000001573ffffffffffffffffffffffffffffffffffffffff1663e591b2826040518163ffffffff1660e01b8152600401602060405180830381865afa15801561037d573d6000803e3d6000fd5b505050506040513d601f19601f820116820180604052508101906103a19190610cce565b73ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff1614610481576040517f08c379a000000000000000000000000000000000000000000000000000000000815260206004820152604160248201527f47617350726963654f7261636c653a206f6e6c7920746865206465706f73697460448201527f6f72206163636f756e742063616e2073657420697345636f746f6e6520666c6160648201527f6700000000000000000000000000000000000000000000000000000000000000608482015260a40161028d565b60005460ff1615610514576040517f08c379a000000000000000000000000000000000000000000000000000000000815260206004820152602660248201527f47617350726963654f7261636c653a2045636f746f6e6520616c72656164792060448201527f6163746976650000000000000000000000000000000000000000000000000000606482015260840161028d565b600080547fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff00166001179055565b6000805460ff161561055c57610556826108b4565b92915050565b61055682610958565b600073420000000000000000000000000000000000001573ffffffffffffffffffffffffffffffffffffffff16635cf249696040518163ffffffff1660e01b8152600401602060405180830381865afa1580156102f5573d6000803e3d6000fd5b600073420000000000000000000000000000000000001573ffffffffffffffffffffffffffffffffffffffff166368d5dca66040518163ffffffff1660e01b8152600401602060405180830381865afa158015610627573d6000803e3d6000fd5b505050506040513d601f19601f820116820180604052508101906103199190610d04565b600073420000000000000000000000000000000000001573ffffffffffffffffffffffffffffffffffffffff1663c59859186040518163ffffffff1660e01b8152600401602060405180830381865afa158015610627573d6000803e3d6000fd5b6000806106b883610ab4565b60005490915060ff16156106cc5792915050565b73420000000000000000000000000000000000001573ffffffffffffffffffffffffffffffffffffffff16638b239f736040518163ffffffff1660e01b8152600401602060405180830381865afa15801561072b573d6000803e3d6000fd5b505050506040513d601f19601f8201168201806040525081019061074f9190610cb5565b6107599082610d59565b9392505050565b6000805460ff16156107f4576040517f08c379a000000000000000000000000000000000000000000000000000000000815260206004820152602660248201527f47617350726963654f7261636c653a207363616c61722829206973206465707260448201527f6563617465640000000000000000000000000000000000000000000000000000606482015260840161028d565b73420000000000000000000000000000000000001573ffffffffffffffffffffffffffffffffffffffff16639e8c49666040518163ffffffff1660e01b8152600401602060405180830381865afa1580156102f5573d6000803e3d6000fd5b600073420000000000000000000000000000000000001573ffffffffffffffffffffffffffffffffffffffff1663f82061406040518163ffffffff1660e01b8152600401602060405180830381865afa1580156102f5573d6000803e3d6000fd5b6000806108c083610ab4565b905060006108cc610565565b6108d461064b565b6108df906010610d71565b63ffffffff166108ef9190610d9d565b905060006108fb610853565b6109036105c6565b63ffffffff166109139190610d9d565b905060006109218284610d59565b61092b9085610d9d565b90506109396006600a610efa565b610944906010610d9d565b61094e9082610f06565b9695505050505050565b60008061096483610ab4565b9050600073420000000000000000000000000000000000001573ffffffffffffffffffffffffffffffffffffffff16639e8c49666040518163ffffffff1660e01b8152600401602060405180830381865afa1580156109c7573d6000803e3d6000fd5b505050506040513d601f19601f820116820180604052508101906109eb9190610cb5565b6109f3610565565b73420000000000000000000000000000000000001573ffffffffffffffffffffffffffffffffffffffff16638b239f736040518163ffffffff1660e01b8152600401602060405180830381865afa158015610a52573d6000803e3d6000fd5b505050506040513d601f19601f82011682018060405250810190610a769190610cb5565b610a809085610d59565b610a8a9190610d9d565b610a949190610d9d565b9050610aa26006600a610efa565b610aac9082610f06565b949350505050565b80516000908190815b81811015610b3757848181518110610ad757610ad7610f41565b01602001517fff0000000000000000000000000000000000000000000000000000000000000016600003610b1757610b10600484610d59565b9250610b25565b610b22601084610d59565b92505b80610b2f81610f70565b915050610abd565b50610aac82610440610d59565b7f4e487b7100000000000000000000000000000000000000000000000000000000600052604160045260246000fd5b600060208284031215610b8557600080fd5b813567ffffffffffffffff80821115610b9d57600080fd5b818401915084601f830112610bb157600080fd5b813581811115610bc357610bc3610b44565b604051601f82017fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe0908116603f01168101908382118183101715610c0957610c09610b44565b81604052828152876020848701011115610c2257600080fd5b826020860160208301376000928101602001929092525095945050505050565b600060208083528351808285015260005b81811015610c6f57858101830151858201604001528201610c53565b81811115610c81576000604083870101525b50601f017fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe016929092016040019392505050565b600060208284031215610cc757600080fd5b5051919050565b600060208284031215610ce057600080fd5b815173ffffffffffffffffffffffffffffffffffffffff8116811461075957600080fd5b600060208284031215610d1657600080fd5b815163ffffffff8116811461075957600080fd5b7f4e487b7100000000000000000000000000000000000000000000000000000000600052601160045260246000fd5b60008219821115610d6c57610d6c610d2a565b500190565b600063ffffffff80831681851681830481118215151615610d9457610d94610d2a565b02949350505050565b6000817fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff0483118215151615610dd557610dd5610d2a565b500290565b600181815b80851115610e3357817fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff04821115610e1957610e19610d2a565b80851615610e2657918102915b93841c9390800290610ddf565b509250929050565b600082610e4a57506001610556565b81610e5757506000610556565b8160018114610e6d5760028114610e7757610e93565b6001915050610556565b60ff841115610e8857610e88610d2a565b50506001821b610556565b5060208310610133831016604e8410600b8410161715610eb6575081810a610556565b610ec08383610dda565b807fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff04821115610ef257610ef2610d2a565b029392505050565b60006107598383610e3b565b600082610f3c577f4e487b7100000000000000000000000000000000000000000000000000000000600052601260045260246000fd5b500490565b7f4e487b7100000000000000000000000000000000000000000000000000000000600052603260045260246000fd5b60007fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff8203610fa157610fa1610d2a565b506001019056fea164736f6c634300080f000a");

        // Deploy the L1 Block Contract
        let mut buffer = Vec::new();
        OpTxEnvelope::Deposit(TxDeposit {
            source_hash: DEPLOY_L1_BLOCK_SOURCE.source_hash(),
            from: L1_BLOCK_DEPLOYER_ADDRESS,
            to: TxKind::Create,
            mint: 0.into(),
            value: U256::ZERO,
            gas_limit: 375_000,
            is_system_transaction: false,
            input: l1_block_deployment_bytecode,
        })
        .encode(&mut buffer);
        txs.push(RawTransaction::from(buffer));

        // Deploy the Gas Price Oracle
        buffer = Vec::new();
        OpTxEnvelope::Deposit(TxDeposit {
            source_hash: DEPLOY_GAS_PRICE_ORACLE_SOURCE.source_hash(),
            from: GAS_PRICE_ORACLE_DEPLOYER_ADDRESS,
            to: TxKind::Create,
            mint: 0.into(),
            value: U256::ZERO,
            gas_limit: 1_000_000,
            is_system_transaction: false,
            input: gas_price_oracle_deployment_bytecode,
        })
        .encode(&mut buffer);
        txs.push(RawTransaction::from(buffer));

        // Update the l1 block proxy
        buffer = Vec::new();
        OpTxEnvelope::Deposit(TxDeposit {
            source_hash: UPDATE_L1_BLOCK_PROXY_SOURCE.source_hash(),
            from: Address::default(),
            to: TxKind::Call(L1_BLOCK_DEPLOYER_ADDRESS),
            mint: 0.into(),
            value: U256::ZERO,
            gas_limit: 50_000,
            is_system_transaction: false,
            input: upgrade_to_calldata(NEW_L1_BLOCK_ADDRESS),
        })
        .encode(&mut buffer);
        txs.push(RawTransaction::from(buffer));

        // Update gas price oracle proxy
        buffer = Vec::new();
        OpTxEnvelope::Deposit(TxDeposit {
            source_hash: UPDATE_GAS_PRICE_ORACLE_SOURCE.source_hash(),
            from: Address::default(),
            to: TxKind::Call(GAS_PRICE_ORACLE_DEPLOYER_ADDRESS),
            mint: 0.into(),
            value: U256::ZERO,
            gas_limit: 50_000,
            is_system_transaction: false,
            input: upgrade_to_calldata(GAS_PRICE_ORACLE_ADDRESS),
        })
        .encode(&mut buffer);
        txs.push(RawTransaction::from(buffer));

        // Enable ecotone
        buffer = Vec::new();
        OpTxEnvelope::Deposit(TxDeposit {
            source_hash: ENABLE_ECOTONE_SOURCE.source_hash(),
            from: L1_BLOCK_DEPLOYER_ADDRESS,
            to: TxKind::Call(GAS_PRICE_ORACLE_ADDRESS),
            mint: 0.into(),
            value: U256::ZERO,
            gas_limit: 80_000,
            is_system_transaction: false,
            input: ENABLE_ECOTONE_INPUT.into(),
        })
        .encode(&mut buffer);
        txs.push(RawTransaction::from(buffer));

        // Deploy EIP4788
        buffer = Vec::new();
        OpTxEnvelope::Deposit(TxDeposit {
            source_hash: BEACON_ROOTS_SOURCE.source_hash(),
            from: EIP4788_FROM,
            to: TxKind::Create,
            mint: 0.into(),
            value: U256::ZERO,
            gas_limit: 250_000,
            is_system_transaction: false,
            input: eip4788_creation_data,
        })
        .encode(&mut buffer);
        txs.push(RawTransaction::from(buffer));

        Ok(txs)
    }
}

