//! Reflector oracle client interface — copied near-verbatim from
//! `reflector-network/reflector-contract`'s own published integration
//! example (its README literally says "copy and save it in your smart
//! contract project"), trimmed to the read-only subset this contract
//! actually calls. Not a crate dependency: Reflector doesn't publish one,
//! and cross-contract calls in Soroban match on wire shape (the XDR
//! encoding of the type), not Rust type identity — this only has to
//! describe the remote contract's interface correctly, not literally
//! share code with it.
//!
//! Live testnet addresses (verified 5 Sept 2026 via `stellar contract
//! invoke ... -- assets`, see `HANDOFF.md`):
//!   - Fiat exchange rates: `CCSSOHTBL3LEWUCBBEB5NJFC2OKFRC74OWEIJIZLRJBGAAU4VMU5NV4W`
//!     (base `USD`, 14 decimals, quotes EUR/GBP/CHF/CAD/MXN/ARS/BRL/THB/XAU
//!     — **not NGN**, a real gap this session found, not an assumption;
//!     see the module doc in `lib.rs`)
//!   - External CEXs & DEXs: `CCYOZJCOPG34LLQQ7N24YXBM7LL62R7ONMZ3G6WZAAYPB5OYKOMJRN63`
//!   - Stellar Mainnet DEX: `CAVLP5DH2GJPZMVO7IJY4CVOD5MWEFTJFVPD2YY2FQXOQHRGHK4D6HLP`

use soroban_sdk::{contracttype, Address, Symbol, Vec};

// The trait itself is never called directly -- only `ReflectorPulseClient`,
// which #[contractclient] generates from it, is. Clippy's dead_code lint
// doesn't see through that, so it's silenced here rather than left as an
// unexplained warning.
#[allow(dead_code)]
#[soroban_sdk::contractclient(name = "ReflectorPulseClient")]
pub trait Contract {
    /// Base asset every quoted price is denominated in (e.g. `Other("USD")`
    /// for the fiat exchange rate oracle).
    fn base() -> Asset;
    /// All assets quoted by this oracle instance.
    fn assets() -> Vec<Asset>;
    /// Decimal places used to represent price for every asset this oracle
    /// quotes. `price / 10^decimals()` is the actual rate.
    fn decimals() -> u32;
    /// Quotes `asset`'s price at a specific past `timestamp`, if recorded.
    fn price(asset: Asset, timestamp: u64) -> Option<PriceData>;
    /// Quotes the most recent price for `asset`. `None` if the oracle
    /// doesn't quote that asset at all (see `assets()`).
    fn lastprice(asset: Asset) -> Option<PriceData>;
}

#[contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum Asset {
    /// A Stellar Classic or Soroban asset, by contract address.
    Stellar(Address),
    /// Any external currency/token/symbol not native to Stellar — fiat
    /// currency codes like `NGN` or `USD` are quoted this way.
    Other(Symbol),
}

#[contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct PriceData {
    pub price: i128,
    pub timestamp: u64,
}
