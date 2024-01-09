use std::prelude::v1::*;

use core::marker::PhantomData;
use eth_types::{
    AccessListResult, AccountResult, BlockSelector, BlockSimple, EngineTypes, EthereumEngineTypes,
    FetchState, FetchStateResult, HexBytes, Log, StorageResult, TxTrait, SH160, SH256, SU256, SU64,
};
pub use jsonrpc::{JsonrpcClient, MixRpcClient, RpcClient, RpcError};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct ExecutionClient<C: RpcClient = MixRpcClient, E: EngineTypes = EthereumEngineTypes> {
    client: JsonrpcClient<C>,
    phantom: PhantomData<E>,
}

impl<C: RpcClient, E: EngineTypes> ExecutionClient<C, E> {
    pub fn new(client: C) -> Self {
        Self {
            client: JsonrpcClient::new(client),
            phantom: PhantomData,
        }
    }

    pub fn raw(&self) -> &JsonrpcClient<C> {
        &self.client
    }

    pub fn to_tx_map(caller: &SH160, tx: &E::Transaction) -> serde_json::Value {
        let mut tx = tx.to_json_map();
        tx.insert("from".into(), serde_json::to_value(caller).unwrap());
        tx.remove("r");
        tx.remove("s");
        tx.remove("v");
        return serde_json::Value::Object(tx);
    }

    pub fn chain_id(&self) -> Result<u64, RpcError> {
        let chain_id: SU64 = self.client.rpc("eth_chainId", ())?;
        Ok(chain_id.as_u64())
    }

    pub fn balance(&self, addr: &SH160, block: BlockSelector) -> Result<SU256, RpcError> {
        self.client.rpc("eth_getBalance", (addr, block))
    }

    pub fn head(&self) -> Result<SU64, RpcError> {
        self.client.rpc("eth_blockNumber", ())
    }

    pub fn nonce(&self, addr: &SH160, block: BlockSelector) -> Result<SU64, RpcError> {
        self.client.rpc("eth_getTransactionCount", (addr, block))
    }

    pub fn gas_price(&self) -> Result<SU256, RpcError> {
        self.client.rpc("eth_gasPrice", ())
    }

    pub fn send_raw_transaction(&self, tx: &E::Transaction) -> Result<SH256, RpcError> {
        self.client.rpc("eth_sendRawTransaction", (tx,))
    }

    pub fn estimate_gas(
        &self,
        tx: &E::Transaction,
        block: BlockSelector,
    ) -> Result<SU256, RpcError> {
        let tx = tx.to_json_map();
        self.client.rpc("eth_estimateGas", (tx, block))
    }

    pub fn eth_call<T>(&self, call: EthCall, block: BlockSelector) -> Result<T, RpcError>
    where
        T: DeserializeOwned,
    {
        self.client.rpc("eth_call", (call, block))
    }

    pub fn create_access_list(
        &self,
        caller: &SH160,
        tx: &E::Transaction,
        blk: BlockSelector,
    ) -> Result<AccessListResult, RpcError> {
        let mut result: AccessListResult = self
            .client
            .rpc("eth_createAccessList", (Self::to_tx_map(caller, tx), blk))?;
        result.ensure(caller, tx.to());
        Ok(result)
    }

    pub fn get_code(&self, address: &SH160, blk: BlockSelector) -> Result<HexBytes, RpcError> {
        self.client.rpc("eth_getCode", (address, blk))
    }

    pub fn get_codes(
        &self,
        address: &[SH160],
        blk: BlockSelector,
    ) -> Result<Vec<HexBytes>, RpcError> {
        let params_list: Vec<_> = address.into_iter().map(|addr| (addr, blk)).collect();
        self.client.batch_rpc("eth_getCode", &params_list)
    }

    pub fn get_storage(
        &self,
        address: &SH160,
        key: &SH256,
        blk: BlockSelector,
    ) -> Result<SH256, RpcError> {
        self.client.rpc("eth_getStorageAt", (address, key, blk))
    }

    pub fn get_block_generic<T>(
        &self,
        selector: BlockSelector,
        with_tx: bool,
    ) -> Result<T, RpcError>
    where
        T: DeserializeOwned,
    {
        match selector {
            BlockSelector::Hash(hash) => self.client.rpc("eth_getBlockByHash", (&hash, with_tx)),
            BlockSelector::Number(number) => {
                self.client.rpc("eth_getBlockByNumber", (&number, with_tx))
            }
            BlockSelector::Latest => self.client.rpc("eth_getBlockByNumber", ("latest", with_tx)),
        }
    }

    pub fn get_block_simple(
        &self,
        selector: BlockSelector,
    ) -> Result<BlockSimple<E::BlockHeader, E::Withdrawal>, RpcError> {
        self.get_block_generic(selector, false)
    }

    pub fn get_block_header(&self, selector: BlockSelector) -> Result<E::BlockHeader, RpcError> {
        self.get_block_generic(selector, false)
    }

    pub fn get_block(&self, selector: BlockSelector) -> Result<E::Block, RpcError> {
        self.get_block_generic(selector, true)
    }

    pub fn get_logs(&self, filter: &LogFilter) -> Result<Vec<Log>, RpcError> {
        self.client.rpc("eth_getLogs", (filter,))
    }

    pub fn get_block_number(&self) -> Result<SU64, RpcError> {
        self.client.rpc("eth_blockNumber", ())
    }

    pub fn get_proof(
        &self,
        account: &SH160,
        keys: &[SH256],
        block: BlockSelector,
    ) -> Result<AccountResult, RpcError> {
        self.client.rpc("eth_getProof", (account, keys, block))
    }

    pub fn fetch_states(
        &self,
        list: &[FetchState],
        block: BlockSelector,
        with_proof: bool,
    ) -> Result<Vec<FetchStateResult>, RpcError> {
        if with_proof {
            return self.fetch_states_with_proof(list, block);
        }
        let mut request = Vec::new();
        for item in list {
            let addr = match item.get_addr() {
                Some(addr) => addr,
                None => continue,
            };

            request.push(self.client.req("eth_getBalance", &(addr, block))?);
            request.push(self.client.req("eth_getTransactionCount", &(addr, block))?);

            if let Some(addr) = item.code {
                request.push(self.client.req("eth_getCode", &(addr, block))?);
            }
            if let Some(item) = &item.access_list {
                for key in &item.storage_keys {
                    let params = (&item.address, key, block);
                    request.push(self.client.req("eth_getStorageAt", &params)?);
                }
            }
        }
        let response = self.client.multi_chunk_rpc(request, 1000)?;
        let mut idx = 0;
        let mut out = Vec::with_capacity(list.len());
        for item in list {
            let addr = match item.get_addr() {
                Some(addr) => addr,
                None => continue,
            };
            let mut result = FetchStateResult::default();
            let mut acc = AccountResult::default();
            acc.address = addr.clone();
            acc.balance = serde_json::from_raw_value(&response[idx]).unwrap();
            idx += 1;
            acc.nonce = serde_json::from_raw_value(&response[idx]).unwrap();
            idx += 1;

            if let Some(_) = &item.code {
                let code = serde_json::from_raw_value(&response[idx]).unwrap();
                idx += 1;
                result.code = Some(code);
            }
            if let Some(item) = &item.access_list {
                acc.storage_proof = Vec::with_capacity(item.storage_keys.len());
                for key in &item.storage_keys {
                    acc.storage_proof.push(StorageResult {
                        key: key.as_bytes().into(),
                        value: serde_json::from_raw_value(&response[idx]).unwrap(),
                        proof: Vec::new(),
                    });
                    idx += 1;
                }
            }
            result.acc = Some(acc);
            out.push(result);
        }
        Ok(out)
    }

    pub fn fetch_states_with_proof(
        &self,
        list: &[FetchState],
        block: BlockSelector,
    ) -> Result<Vec<FetchStateResult>, RpcError> {
        let mut request = Vec::with_capacity(list.len());
        for item in list {
            if let Some(item) = &item.access_list {
                let params = (&item.address, &item.storage_keys, block);
                request.push(self.client.req("eth_getProof", &params)?);
            }
            if let Some(addr) = &item.code {
                let params = (addr, block);
                request.push(self.client.req("eth_getCode", &params)?);
            }
        }

        let result = self.client.multi_chunk_rpc(request, 1000)?;
        let mut out: Vec<FetchStateResult> = Vec::with_capacity(result.len() / 2);
        let mut iter = result.into_iter();
        for item in list {
            let mut state = FetchStateResult::default();
            if let Some(_) = item.access_list {
                let acc = match iter.next() {
                    Some(item) => serde_json::from_raw_value(&item).map_err(|err| {
                        RpcError::SerdeResponseError("fetch_states_with_proof".into(), item.to_string(), err)
                    })?,
                    None => break,
                };
                state.acc = Some(acc);
            }
            if let Some(_) = item.code {
                let code = match iter.next() {
                    Some(item) => serde_json::from_raw_value(&item).map_err(|err| {
                        RpcError::SerdeResponseError("fetch_states_with_proof".into(), item.to_string(), err)
                    })?,
                    None => break,
                };
                state.code = Some(code);
            }
            out.push(state);
        }
        assert_eq!(out.len(), list.len());
        Ok(out)
    }

    pub fn get_dbnodes(&self, key: &[SH256]) -> Result<Vec<HexBytes>, RpcError> {
        let params_list = key.iter().map(|item| [item]).collect::<Vec<_>>();
        self.client.batch_rpc("debug_dbGet", &params_list)
    }

    pub fn get_transaction(&self, tx: &SH256) -> Result<E::RpcTransaction, RpcError> {
        self.client.rpc("eth_getTransactionByHash", [tx])
    }

    pub fn get_receipt(&self, hash: &SH256) -> Result<Option<E::Receipt>, RpcError> {
        self.client.rpc("eth_getTransactionReceipt", (hash,))
    }

    pub fn get_receipts(&self, hashes: &[SH256]) -> Result<Vec<Option<E::Receipt>>, RpcError> {
        let hashes = hashes.iter().map(|n| [n]).collect::<Vec<_>>();
        self.client.batch_rpc("eth_getTransactionReceipt", &hashes)
    }

    pub fn trace_prestate(&self, block: BlockSelector) -> Result<Vec<TxPrestateResult>, RpcError> {
        let cfg = TraceConfig {
            tracer: Some("prestateTracer".into()),
            enable_memory: false,
        };
        match block {
            BlockSelector::Hash(hash) => self.client.rpc("debug_traceBlockByHash", (hash, cfg)),
            BlockSelector::Number(number) => {
                self.client.rpc("debug_traceBlockByNumber", (number, cfg))
            }
            BlockSelector::Latest => unimplemented!(),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EthCall {
    pub to: SH160,
    pub from: Option<SH160>,
    pub gas: Option<SU64>,
    pub gas_price: Option<SU256>,
    pub data: HexBytes,
}

#[derive(Debug, Serialize, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFilter {
    pub address: Vec<SH160>,
    pub topics: Vec<Vec<SH256>>,
    pub to_block: Option<SU256>,
    pub from_block: Option<SU256>,
    pub block_hash: Option<SH256>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceConfig {
    pub tracer: Option<String>,
    pub enable_memory: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxPrestateResult {
    pub tx_hash: SH256,
    pub result: Option<BTreeMap<SH160, PrestateAccount>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrestateAccount {
    pub balance: SU256,
    #[serde(default)]
    pub code: HexBytes,
    #[serde(default)]
    pub nonce: u64,
    #[serde(default)]
    pub storage: BTreeMap<SH256, SH256>,
}
