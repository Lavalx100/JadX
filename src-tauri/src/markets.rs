use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::time::timeout;
use uuid::Uuid;

const PUBLIC_FALLBACK_MESSAGE: &str =
    "Public fallback data may be delayed or rate-limited; it is for research only and cannot be used for order submission.";

// Adapted provider-rotation and HK Tencent parsing patterns from Cricle/akshare-rs
// (MIT OR Apache-2.0). JadX keeps its own provenance and safety rules around them.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Freshness {
    Live,
    Delayed,
    Recent,
    Cached,
    Stale,
    Simulated,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    pub provider: String,
    pub observed_at: Option<String>,
    pub retrieved_at: String,
    pub freshness: Freshness,
    pub realtime: bool,
    pub message: Option<String>,
}

impl Default for Provenance {
    fn default() -> Self {
        Self::unavailable("unknown", "No market data has been retrieved.")
    }
}

impl Provenance {
    fn unavailable(provider: &str, message: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            observed_at: None,
            retrieved_at: Utc::now().to_rfc3339(),
            freshness: Freshness::Unavailable,
            realtime: false,
            message: Some(message.into()),
        }
    }

    fn simulated() -> Self {
        Self {
            provider: "fixture".into(),
            observed_at: None,
            retrieved_at: Utc::now().to_rfc3339(),
            freshness: Freshness::Simulated,
            realtime: false,
            message: Some("SIMULATED development fixture; not market data".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Quote {
    pub symbol: String,
    pub name: String,
    pub exchange: String,
    pub currency: String,
    pub last: Option<f64>,
    pub change: Option<f64>,
    pub change_pct: Option<f64>,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    pub volume: Option<f64>,
    pub turnover: Option<f64>,
    pub day_high: Option<f64>,
    pub day_low: Option<f64>,
    pub open: Option<f64>,
    pub previous_close: Option<f64>,
    pub market_cap: Option<f64>,
    pub price_to_book: Option<f64>,
    pub pe_ratio: Option<f64>,
    pub pe_ttm: Option<f64>,
    pub dividend_yield: Option<f64>,
    pub industry: Option<String>,
    pub market_session: String,
    pub quote_level: Option<String>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Candle {
    pub timestamp: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub turnover: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TechnicalSnapshot {
    pub sma20: Option<f64>,
    pub sma50: Option<f64>,
    pub ema20: Option<f64>,
    pub rsi14: Option<f64>,
    pub atr14: Option<f64>,
    pub volume_vs_sma20: Option<f64>,
    pub return_1d: Option<f64>,
    pub return_20d: Option<f64>,
    pub return_60d: Option<f64>,
    pub max_drawdown: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct History {
    pub symbol: String,
    pub interval: String,
    pub candles: Vec<Candle>,
    pub indicators: TechnicalSnapshot,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MacroFactor {
    pub label: String,
    pub score: f64,
    pub detail: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MacroSnapshot {
    pub as_of: String,
    pub hsi: Option<Quote>,
    pub hstech: Option<Quote>,
    pub sp500: Option<Quote>,
    pub nasdaq: Option<Quote>,
    pub vix: Option<Quote>,
    pub us10y: Option<Quote>,
    pub dollar_index: Option<Quote>,
    pub risk_on_score: Option<f64>,
    pub factors: Vec<MacroFactor>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResearchFact {
    pub label: String,
    pub value: String,
    pub period: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResearchItem {
    pub title: String,
    pub detail: String,
    pub url: Option<String>,
    pub published_at: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CompanyResearch {
    pub symbol: String,
    pub name: String,
    pub profile: Vec<ResearchFact>,
    pub financials: Vec<ResearchFact>,
    pub news: Vec<ResearchItem>,
    pub filings: Vec<ResearchItem>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ThematicCompany {
    pub symbol: String,
    pub name: String,
    pub themes: Vec<String>,
    pub listing_status: String,
    pub note: String,
    pub quote: Option<Quote>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Watchlist {
    pub id: String,
    pub name: String,
    pub symbols: Vec<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Holding {
    pub id: String,
    pub account: String,
    pub symbol: String,
    pub shares: f64,
    pub average_cost: f64,
    pub currency: String,
    pub source: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PortfolioTrade {
    pub id: String,
    pub account: String,
    pub symbol: String,
    pub side: String,
    pub quantity: f64,
    pub price: f64,
    pub commission: f64,
    pub trade_date: String,
    pub notes: String,
    pub source: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PortfolioRow {
    pub holding: Holding,
    pub quote: Quote,
    pub market_value: Option<f64>,
    pub unrealized_pnl: Option<f64>,
    pub unrealized_pnl_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RadarEvent {
    pub id: String,
    pub symbol: String,
    pub title: String,
    pub detail: String,
    pub severity: String,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FutuHealth {
    pub host: String,
    pub port: u16,
    pub tcp_reachable: bool,
    pub sdk_available: bool,
    pub connection_state: String,
    pub login_state: String,
    pub quote_permissions: String,
    pub subscription_usage: String,
    pub checked_at: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Dashboard {
    pub health: FutuHealth,
    pub indices: Vec<Quote>,
    pub watchlists: Vec<Watchlist>,
    pub watchlist_quotes: Vec<Quote>,
    pub portfolio: Vec<PortfolioRow>,
    pub trades: Vec<PortfolioTrade>,
    pub trade_quotes: Vec<Quote>,
    pub radar: Vec<RadarEvent>,
    pub trading_mode: String,
    pub fixture_mode: bool,
    pub retrieved_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UniverseSnapshot {
    pub retrieved_at: String,
    pub constituent_source: String,
    pub hsi: Vec<Quote>,
    pub hstech: Vec<Quote>,
    pub thematic: Vec<ThematicCompany>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OrderPreview {
    pub symbol: String,
    pub side: String,
    pub quantity: f64,
    pub order_type: String,
    pub limit_price: Option<f64>,
    pub estimated_notional: Option<f64>,
    pub market_state: String,
    pub valid: bool,
    pub issues: Vec<String>,
    pub trading_mode: String,
    pub live_submission_available: bool,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MarketOrder {
    pub id: String,
    pub symbol: String,
    pub side: String,
    pub quantity: f64,
    pub order_type: String,
    pub limit_price: Option<f64>,
    pub mode: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct MarketsRuntime {
    pub consecutive_failures: u32,
    pub next_retry_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct MarketsConfig {
    pub enabled: bool,
    pub fixture_mode: bool,
    pub futu_host: String,
    pub futu_port: u16,
    pub trading_mode: String,
}

pub fn config_from_settings(settings: &Value) -> MarketsConfig {
    MarketsConfig {
        enabled: settings
            .get("markets_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        fixture_mode: settings
            .get("markets_fixture_mode")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        futu_host: settings
            .get("markets_futu_host")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("127.0.0.1")
            .to_string(),
        futu_port: settings
            .get("markets_futu_port")
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or(11111),
        trading_mode: settings
            .get("markets_trading_mode")
            .and_then(Value::as_str)
            .filter(|value| matches!(*value, "OFF" | "PAPER" | "LIVE_WITH_APPROVAL"))
            .unwrap_or("OFF")
            .to_string(),
    }
}

pub fn normalize_symbol(input: &str) -> Result<String, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("Enter an exchange-qualified ticker symbol.".into());
    }
    let key = raw.to_ascii_lowercase();
    let aliases = HashMap::from([
        ("hsi", "HK.800000"),
        ("hang seng", "HK.800000"),
        ("hstech", "HK.800700"),
    ]);
    if let Some(symbol) = aliases.get(key.as_str()) {
        return Ok((*symbol).into());
    }
    let cleaned = raw
        .trim_end_matches(".HK")
        .trim_end_matches(".hk")
        .replace("HK.", "")
        .replace("hk.", "")
        .replace(' ', "");
    if cleaned.chars().all(|character| character.is_ascii_digit()) {
        let number: u32 = cleaned
            .parse()
            .map_err(|_| "Ticker number is out of range.")?;
        let is_supported_index = matches!(number, 800000 | 800700);
        if number > 99999 && !is_supported_index {
            return Err("HK ticker must contain at most five digits.".into());
        }
        return Ok(if is_supported_index {
            format!("HK.{number}")
        } else {
            format!("HK.{number:05}")
        });
    }
    if let Some((exchange, ticker)) = raw.split_once('.') {
        let exchange = exchange.to_ascii_uppercase();
        let ticker = ticker.to_ascii_uppercase();
        if matches!(exchange.as_str(), "US" | "SH" | "SZ" | "HK") && !ticker.is_empty() {
            return Ok(format!("{exchange}.{ticker}"));
        }
    }
    if raw
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Ok(format!("US.{}", raw.to_ascii_uppercase()));
    }
    Err(format!(
        "Could not resolve ticker ‘{raw}’. Enter a supported exchange-qualified symbol."
    ))
}

pub fn merge_candles(mut existing: Vec<Candle>, incoming: Vec<Candle>) -> Vec<Candle> {
    let mut by_timestamp = existing
        .drain(..)
        .map(|candle| (candle.timestamp, candle))
        .collect::<HashMap<_, _>>();
    for candle in incoming {
        by_timestamp.insert(candle.timestamp, candle);
    }
    let mut merged = by_timestamp.into_values().collect::<Vec<_>>();
    merged.sort_by_key(|candle| candle.timestamp);
    merged
}

pub fn sma(values: &[f64], period: usize) -> Option<f64> {
    if period == 0 || values.len() < period {
        return None;
    }
    Some(values[values.len() - period..].iter().sum::<f64>() / period as f64)
}

pub fn ema(values: &[f64], period: usize) -> Option<f64> {
    if period == 0 || values.len() < period {
        return None;
    }
    let multiplier = 2.0 / (period as f64 + 1.0);
    let mut result = values[..period].iter().sum::<f64>() / period as f64;
    for value in &values[period..] {
        result = (*value - result) * multiplier + result;
    }
    Some(result)
}

pub fn technicals(candles: &[Candle]) -> TechnicalSnapshot {
    let closes = candles
        .iter()
        .map(|candle| candle.close)
        .collect::<Vec<_>>();
    let volumes = candles
        .iter()
        .map(|candle| candle.volume)
        .collect::<Vec<_>>();
    let rsi = if closes.len() < 15 {
        None
    } else {
        let changes = closes.windows(2).map(|pair| pair[1] - pair[0]);
        let (mut gains, mut losses) = (0.0, 0.0);
        for change in changes.clone().skip(changes.count().saturating_sub(14)) {
            if change >= 0.0 {
                gains += change;
            } else {
                losses -= change;
            }
        }
        if losses == 0.0 {
            Some(100.0)
        } else {
            Some(100.0 - 100.0 / (1.0 + (gains / 14.0) / (losses / 14.0)))
        }
    };
    let atr = if candles.len() < 15 {
        None
    } else {
        let ranges = candles
            .windows(2)
            .map(|pair| {
                let previous_close = pair[0].close;
                pair[1].high.max(previous_close) - pair[1].low.min(previous_close)
            })
            .collect::<Vec<_>>();
        sma(&ranges, 14)
    };
    let max_drawdown = if closes.is_empty() {
        None
    } else {
        let mut peak = closes[0];
        let mut drawdown: f64 = 0.0;
        for close in closes.iter().copied() {
            peak = peak.max(close);
            if peak > 0.0 {
                drawdown = drawdown.min(close / peak - 1.0);
            }
        }
        Some(drawdown)
    };
    TechnicalSnapshot {
        sma20: sma(&closes, 20),
        sma50: sma(&closes, 50),
        ema20: ema(&closes, 20),
        rsi14: rsi,
        atr14: atr,
        volume_vs_sma20: sma(&volumes, 20)
            .and_then(|average| volumes.last().map(|last| last / average)),
        return_1d: closes
            .len()
            .checked_sub(2)
            .and_then(|index| closes.last().zip(closes.get(index)))
            .map(|(last, previous)| last / previous - 1.0),
        return_20d: closes
            .len()
            .checked_sub(21)
            .and_then(|index| closes.last().zip(closes.get(index)))
            .map(|(last, previous)| last / previous - 1.0),
        return_60d: closes
            .len()
            .checked_sub(61)
            .and_then(|index| closes.last().zip(closes.get(index)))
            .map(|(last, previous)| last / previous - 1.0),
        max_drawdown,
    }
}

pub fn quote_from_bridge(symbol: &str, row: &Value) -> Quote {
    let number = |key: &str| row.get(key).and_then(value_as_f64);
    let observed_at = row
        .get("update_time")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            let date = row.get("data_date")?.as_str()?;
            let time = row.get("data_time")?.as_str()?;
            Some(format!("{date} {time}"))
        })
        .or_else(|| {
            row.get("data_time")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .and_then(|value| {
            DateTime::parse_from_rfc3339(&value)
                .ok()
                .map(|date| date.timestamp())
                .or_else(|| {
                    // Futu snapshot update_time and basic-quote date/time are
                    // exchange-local. Never label an unzoned US time as HK time.
                    if symbol.starts_with("HK.")
                        || symbol.starts_with("SH.")
                        || symbol.starts_with("SZ.")
                    {
                        parse_market_timestamp(&value)
                    } else {
                        None
                    }
                })
        })
        .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
        .map(|date| date.to_rfc3339());
    let age = observed_at
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|date| Utc::now().timestamp() - date.timestamp());
    let freshness = match age {
        Some(-60..=300) => Freshness::Live,
        Some(301..=129_600) | None => Freshness::Recent,
        _ => Freshness::Stale,
    };
    let realtime = freshness == Freshness::Live;
    let timestamp_message = if observed_at.is_none() {
        Some("OpenD response has no usable observation timestamp; retrieval time does not establish live freshness.".into())
    } else {
        None
    };
    let last = number("last_price");
    let previous_close = number("prev_close_price").or_else(|| number("last_close"));
    let name = row
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| security_name(symbol));
    Quote {
        symbol: symbol.into(),
        name: name.into(),
        exchange: symbol.split('.').next().unwrap_or("HK").into(),
        currency: if symbol.starts_with("HK.") {
            "HKD"
        } else if symbol.starts_with("SH.") || symbol.starts_with("SZ.") {
            "CNY"
        } else {
            "USD"
        }
        .into(),
        last,
        change: number("change_val").or_else(|| last.zip(previous_close).map(|(l, p)| l - p)),
        change_pct: number("change_rate")
            .map(|value| value / 100.0)
            .or_else(|| {
                last.zip(previous_close)
                    .filter(|(_, previous)| *previous != 0.0)
                    .map(|(l, p)| l / p - 1.0)
            }),
        bid: number("bid_price"),
        ask: number("ask_price"),
        volume: number("volume"),
        turnover: number("turnover"),
        day_high: number("high_price"),
        day_low: number("low_price"),
        open: number("open_price"),
        previous_close,
        market_cap: number("total_market_val").or_else(|| number("market_cap")),
        price_to_book: number("pb_ratio").or_else(|| number("price_to_book")),
        pe_ratio: number("pe_ratio"),
        pe_ttm: number("pe_ttm").or_else(|| number("pe_ratio_ttm")),
        dividend_yield: number("dividend_ratio").or_else(|| number("dividend_yield")),
        industry: row
            .get("industry_name")
            .or_else(|| row.get("industry"))
            .and_then(Value::as_str)
            .map(str::to_string),
        market_session: "OpenD quote response".into(),
        quote_level: Some("OpenD".into()),
        provenance: Provenance {
            provider: "futu-opend".into(),
            observed_at,
            retrieved_at: Utc::now().to_rfc3339(),
            freshness,
            realtime,
            message: timestamp_message,
        },
    }
}

pub fn candle_from_bridge(row: &Value) -> Option<Candle> {
    let timestamp = row
        .get("time_key")
        .and_then(Value::as_str)
        .and_then(parse_market_timestamp)
        .or_else(|| row.get("timestamp").and_then(Value::as_i64))?;
    Some(Candle {
        timestamp,
        open: row.get("open").and_then(value_as_f64)?,
        high: row.get("high").and_then(value_as_f64)?,
        low: row.get("low").and_then(value_as_f64)?,
        close: row.get("close").and_then(value_as_f64)?,
        volume: row.get("volume").and_then(value_as_f64).unwrap_or(0.0),
        turnover: row.get("turnover").and_then(value_as_f64),
    })
}

fn parse_market_timestamp(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .or_else(|_| DateTime::parse_from_rfc3339(&format!("{value}+08:00")))
        .map(|value| value.timestamp())
        .ok()
        .or_else(|| {
            DateTime::parse_from_str(&format!("{value} +0800"), "%Y-%m-%d %H:%M:%S %z")
                .ok()
                .map(|value| value.timestamp())
        })
        .or_else(|| {
            DateTime::parse_from_str(&format!("{value} 00:00:00 +0800"), "%Y-%m-%d %H:%M:%S %z")
                .ok()
                .map(|value| value.timestamp())
        })
}

fn value_as_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|number| number as f64))
}

pub fn security_name(symbol: &str) -> &'static str {
    match symbol {
        "HK.800000" => "Hang Seng Index",
        "HK.800700" => "Hang Seng TECH Index",
        "SH.000001" => "Shanghai Composite",
        "US.SPX" => "S&P 500",
        "US.NASDAQ" => "Nasdaq Composite",
        "US.VIX" => "CBOE Volatility Index",
        "US.US10Y" => "US 10Y Treasury yield",
        "US.DXY" => "US Dollar Index",
        _ => "Unknown security",
    }
}

fn unavailable_quote(symbol: &str, message: impl Into<String>) -> Quote {
    unavailable_quote_with_provider(symbol, "futu-opend", message)
}

fn unavailable_quote_with_provider(
    symbol: &str,
    provider: &str,
    message: impl Into<String>,
) -> Quote {
    Quote {
        symbol: symbol.into(),
        name: security_name(symbol).into(),
        exchange: symbol.split('.').next().unwrap_or("HK").into(),
        currency: if symbol.starts_with("HK.") {
            "HKD"
        } else if symbol.starts_with("SH.") || symbol.starts_with("SZ.") {
            "CNY"
        } else {
            "USD"
        }
        .into(),
        market_session: "Unknown".into(),
        provenance: Provenance::unavailable(provider, message),
        ..Default::default()
    }
}

fn simulated_quote(symbol: &str) -> Quote {
    let (last, change_pct) = (100.0, 0.0);
    let previous_close = last / (1.0 + change_pct);
    Quote {
        symbol: symbol.into(),
        name: security_name(symbol).into(),
        exchange: "HK".into(),
        currency: "HKD".into(),
        last: Some(last),
        change: Some(last - previous_close),
        change_pct: Some(change_pct),
        bid: Some(last - 0.1),
        ask: Some(last + 0.1),
        volume: Some(1_250_000.0),
        turnover: Some(last * 1_250_000.0),
        day_high: Some(last * 1.018),
        day_low: Some(last * 0.982),
        open: Some(previous_close),
        previous_close: Some(previous_close),
        market_cap: None,
        price_to_book: None,
        pe_ratio: None,
        pe_ttm: None,
        dividend_yield: None,
        industry: None,
        market_session: "Fixture session".into(),
        quote_level: Some("SIMULATED".into()),
        provenance: Provenance::simulated(),
    }
}

pub fn simulated_history(symbol: &str, count: usize) -> History {
    let quote = simulated_quote(symbol);
    let base = quote.last.unwrap_or(100.0);
    let mut candles: Vec<Candle> = Vec::with_capacity(count);
    for index in 0..count {
        let phase = ((index as f64 * 0.37)
            + symbol.bytes().map(|byte| byte as usize).sum::<usize>() as f64 * 0.01)
            .sin();
        let close = base * (0.82 + index as f64 / (count.max(1) as f64) * 0.18 + phase * 0.012);
        let open = if index == 0 {
            close * 0.99
        } else {
            candles[index - 1].close
        };
        let high = open.max(close) * 1.012;
        let low = open.min(close) * 0.988;
        candles.push(Candle {
            timestamp: 1_735_776_000 + index as i64 * 86_400,
            open,
            high,
            low,
            close,
            volume: 900_000.0 + (phase.abs() * 600_000.0),
            turnover: Some(close * 1_000_000.0),
        });
    }
    History {
        symbol: symbol.into(),
        interval: "1D".into(),
        indicators: technicals(&candles),
        candles,
        provenance: Provenance::simulated(),
    }
}

pub async fn check_health(host: &str, port: u16) -> FutuHealth {
    let checked_at = Utc::now().to_rfc3339();
    let address = format!("{host}:{port}");
    let socket = address.parse::<SocketAddr>();
    let tcp_reachable = match socket {
        Ok(address) => timeout(Duration::from_millis(700), TcpStream::connect(address))
            .await
            .ok()
            .and_then(Result::ok)
            .is_some(),
        Err(_) => false,
    };
    let sdk_available = futu_python().await.is_some();
    let message = if !tcp_reachable {
        Some("OpenD is not reachable. Start Futu OpenD and verify its API port. Public delayed fallback providers remain available for research.".into())
    } else if !sdk_available {
        Some(
            "OpenD is reachable, but the optional local futu-api Python bridge is not installed. Public delayed fallback providers remain available for research."
                .into(),
        )
    } else {
        Some(
            "OpenD is reachable. Login and quote rights are checked on the next quote request."
                .into(),
        )
    };
    FutuHealth {
        host: host.into(),
        port,
        tcp_reachable,
        sdk_available,
        connection_state: if tcp_reachable {
            "OpenD connected"
        } else {
            "OpenD unavailable"
        }
        .into(),
        login_state: "Not probed".into(),
        quote_permissions: "Not probed".into(),
        subscription_usage: "Not probed".into(),
        checked_at,
        message,
    }
}

async fn futu_call_value(
    operation: &str,
    host: &str,
    port: u16,
    symbols: &[String],
    count: usize,
) -> Result<Value, String> {
    let script = r#"
import json, math, sys
try:
    from futu import OpenQuoteContext, RET_OK, KLType, AuType
except Exception as exc:
    print(json.dumps({"error": "futu-api unavailable: " + str(exc)}))
    raise SystemExit(2)
def clean(value):
    if isinstance(value, float) and not math.isfinite(value):
        return None
    if isinstance(value, dict):
        return {key: clean(item) for key, item in value.items()}
    if isinstance(value, list):
        return [clean(item) for item in value]
    return value
operation, host, port, symbols, count = sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4].split(',') if sys.argv[4] else [], int(sys.argv[5])
ctx = OpenQuoteContext(host=host, port=port)
try:
    if operation == 'quote':
        ret, data = ctx.get_market_snapshot(symbols)
        if ret != RET_OK:
            print(json.dumps({"error": str(data)}))
            raise SystemExit(3)
        print(json.dumps(clean(data.to_dict(orient='records')), default=str))
    elif operation == 'history':
        ret, data, _ = ctx.request_history_kline(symbols[0], ktype=KLType.K_DAY, autype=AuType.QFQ, max_count=count)
        if ret != RET_OK:
            print(json.dumps({"error": str(data)}))
            raise SystemExit(3)
        print(json.dumps(clean(data.to_dict(orient='records')), default=str))
    elif operation == 'profile':
        ret, data = ctx.get_company_profile(symbols[0])
        if ret != RET_OK:
            print(json.dumps({"error": str(data)}))
            raise SystemExit(3)
        print(json.dumps(clean(data.to_dict(orient='records')), default=str))
    elif operation == 'financials':
        ret, data = ctx.get_financials_statements(symbols[0], num=count)
        if ret != RET_OK:
            print(json.dumps({"error": str(data)}))
            raise SystemExit(3)
        print(json.dumps(clean(data), default=str))
finally:
    ctx.close()
"#;
    let python = futu_python().await.ok_or_else(|| {
        "futu-api is not installed in any configured Python environment.".to_string()
    })?;
    let output = timeout(
        Duration::from_secs(12),
        Command::new(python)
            .arg("-c")
            .arg(script)
            .args([
                operation,
                host,
                &port.to_string(),
                &symbols.join(","),
                &count.to_string(),
            ])
            .output(),
    )
    .await
    .map_err(|_| "Futu request timed out after 12 seconds.".to_string())?
    .map_err(|error| format!("could not start local Futu bridge: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: Value = stdout
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str(line.trim()).ok())
        .ok_or_else(|| {
            let stderr = String::from_utf8_lossy(&output.stderr);
            format!("Futu bridge returned no JSON response: {}", stderr.trim())
        })?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(error.into());
    }
    Ok(value)
}

async fn futu_call(
    operation: &str,
    host: &str,
    port: u16,
    symbols: &[String],
    count: usize,
) -> Result<Vec<Value>, String> {
    futu_call_value(operation, host, port, symbols, count)
        .await?
        .as_array()
        .cloned()
        .ok_or_else(|| "Futu bridge returned an unexpected response.".into())
}

async fn futu_python() -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("JADX_FUTU_PYTHON") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home.clone()).join(".venvs/jadx-futu/bin/python"));
        candidates.push(PathBuf::from(home).join(".venvs/jadx-futu/bin/python3"));
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/python3"),
        PathBuf::from("/usr/local/bin/python3"),
        PathBuf::from("python3"),
    ]);
    for candidate in candidates {
        let output = Command::new(&candidate)
            .args(["-c", "import futu"])
            .output()
            .await;
        if output
            .map(|output| output.status.success())
            .unwrap_or(false)
        {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

fn public_ticker(symbol: &str) -> Option<String> {
    match symbol {
        "HK.800000" => Some("^HSI".into()),
        "HK.800700" => Some("^HSTECH".into()),
        "US.SPX" => Some("^GSPC".into()),
        "US.NASDAQ" => Some("^IXIC".into()),
        "US.VIX" => Some("^VIX".into()),
        "US.US10Y" => Some("^TNX".into()),
        "US.DXY" => Some("DX-Y.NYB".into()),
        "HK.000001" => Some("000001.SS".into()),
        value if value.starts_with("HK.") => value[3..]
            .parse::<u32>()
            .ok()
            .map(|ticker| format!("{ticker:04}.HK")),
        value if value.starts_with("SH.") => Some(format!("{}.SS", &value[3..])),
        value if value.starts_with("SZ.") => Some(format!("{}.SZ", &value[3..])),
        value if value.starts_with("US.") => Some(value[3..].into()),
        _ => None,
    }
}

fn stooq_ticker(symbol: &str) -> Option<String> {
    match symbol {
        value if value.starts_with("HK.") => value[3..]
            .parse::<u32>()
            .ok()
            .map(|ticker| format!("{ticker:04}.hk")),
        value if value.starts_with("US.") => {
            Some(format!("{}.us", &value[3..].to_ascii_lowercase()))
        }
        _ => None,
    }
}

fn public_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("JadX Markets/1.0")
        .build()
        .map_err(|error| format!("could not create public market-data client: {error}"))
}

fn value_at(values: Option<&Vec<Value>>, index: usize) -> Option<f64> {
    values
        .and_then(|values| values.get(index))
        .and_then(value_as_f64)
}

fn public_quote_from_candles(
    symbol: &str,
    candles: &[Candle],
    provider: &str,
    previous_close_override: Option<f64>,
    observed_at: Option<String>,
) -> Quote {
    let last_candle = candles.last();
    let last = last_candle.map(|candle| candle.close);
    let previous_close = previous_close_override.or_else(|| {
        candles
            .get(candles.len().saturating_sub(2))
            .map(|candle| candle.close)
    });
    Quote {
        symbol: symbol.into(),
        name: security_name(symbol).into(),
        exchange: symbol.split('.').next().unwrap_or("US").into(),
        currency: if symbol.starts_with("HK.") {
            "HKD"
        } else if symbol.starts_with("SH.") || symbol.starts_with("SZ.") {
            "CNY"
        } else {
            "USD"
        }
        .into(),
        last,
        change: last
            .zip(previous_close)
            .map(|(last, previous)| last - previous),
        change_pct: last
            .zip(previous_close)
            .filter(|(_, previous)| *previous != 0.0)
            .map(|(last, previous)| last / previous - 1.0),
        bid: None,
        ask: None,
        volume: last_candle.map(|candle| candle.volume),
        turnover: None,
        day_high: last_candle.map(|candle| candle.high),
        day_low: last_candle.map(|candle| candle.low),
        open: last_candle.map(|candle| candle.open),
        previous_close,
        market_cap: None,
        price_to_book: None,
        pe_ratio: None,
        pe_ttm: None,
        dividend_yield: None,
        industry: None,
        market_session: "Public delayed data".into(),
        quote_level: Some("PUBLIC_FALLBACK".into()),
        provenance: Provenance {
            provider: provider.into(),
            observed_at,
            retrieved_at: Utc::now().to_rfc3339(),
            freshness: Freshness::Delayed,
            realtime: false,
            message: Some(PUBLIC_FALLBACK_MESSAGE.into()),
        },
    }
}

fn truncate_candles(mut candles: Vec<Candle>, count: usize) -> Vec<Candle> {
    if candles.len() > count {
        candles.drain(0..candles.len() - count);
    }
    candles
}

fn parse_yahoo_chart(symbol: &str, value: &Value) -> Result<(Quote, Vec<Candle>), String> {
    let result = value
        .pointer("/chart/result/0")
        .ok_or_else(|| "Public Yahoo chart returned no result.".to_string())?;
    let timestamps = result
        .get("timestamp")
        .and_then(Value::as_array)
        .ok_or_else(|| "Public Yahoo chart returned no timestamps.".to_string())?;
    let quote_values = result
        .pointer("/indicators/quote/0")
        .ok_or_else(|| "Public Yahoo chart returned no OHLC data.".to_string())?;
    let opens = quote_values.get("open").and_then(Value::as_array);
    let highs = quote_values.get("high").and_then(Value::as_array);
    let lows = quote_values.get("low").and_then(Value::as_array);
    let closes = quote_values.get("close").and_then(Value::as_array);
    let volumes = quote_values.get("volume").and_then(Value::as_array);
    let mut candles = Vec::new();
    for (index, timestamp) in timestamps.iter().enumerate() {
        let Some(timestamp) = timestamp.as_i64() else {
            continue;
        };
        let (Some(open), Some(high), Some(low), Some(close)) = (
            value_at(opens, index),
            value_at(highs, index),
            value_at(lows, index),
            value_at(closes, index),
        ) else {
            continue;
        };
        candles.push(Candle {
            timestamp,
            open,
            high,
            low,
            close,
            volume: value_at(volumes, index).unwrap_or(0.0),
            turnover: None,
        });
    }
    if candles.is_empty() {
        return Err("Public Yahoo chart returned no usable candles.".into());
    }
    let meta = result.get("meta").unwrap_or(&Value::Null);
    let observed_at = meta
        .get("regularMarketTime")
        .and_then(Value::as_i64)
        .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
        .map(|value| value.to_rfc3339());
    let previous_close = meta
        .get("previousClose")
        .and_then(value_as_f64)
        .or_else(|| meta.get("chartPreviousClose").and_then(value_as_f64));
    Ok((
        public_quote_from_candles(
            symbol,
            &candles,
            "public-yahoo-chart",
            previous_close,
            observed_at,
        ),
        candles,
    ))
}

async fn public_yahoo_data(symbol: &str) -> Result<(Quote, Vec<Candle>), String> {
    let ticker = public_ticker(symbol)
        .ok_or_else(|| format!("No public Yahoo ticker mapping is available for {symbol}."))?;
    let client = public_http_client()?;
    let mut errors = Vec::new();
    for host in ["query1.finance.yahoo.com", "query2.finance.yahoo.com"] {
        let url = format!(
            "https://{host}/v8/finance/chart/{ticker}?range=5y&interval=1d&includePrePost=false&events=div%2Csplits"
        );
        match client.get(url).send().await {
            Ok(response) => match response.error_for_status() {
                Ok(response) => match response.json::<Value>().await {
                    Ok(value) => match parse_yahoo_chart(symbol, &value) {
                        Ok(data) => return Ok(data),
                        Err(error) => errors.push(error),
                    },
                    Err(error) => errors.push(format!("Yahoo response was not JSON: {error}")),
                },
                Err(error) => errors.push(format!("Yahoo request failed: {error}")),
            },
            Err(error) => errors.push(format!("Yahoo request failed: {error}")),
        }
    }
    Err(errors.join(" | "))
}

async fn macro_public_quote(symbol: &str) -> Option<Quote> {
    let (mut quote, _) = public_yahoo_data(symbol).await.ok()?;
    quote.name = security_name(symbol).into();
    Some(quote)
}

fn macro_move_score(quotes: &[(&str, Option<&Quote>)]) -> Option<(f64, String)> {
    let mut available = quotes
        .iter()
        .filter_map(|(_, quote)| quote.and_then(|quote| quote.change_pct))
        .map(|value| clamp_public(value, -0.08, 0.08))
        .collect::<Vec<_>>();
    if available.is_empty() {
        return None;
    }
    available.sort_by(f64::total_cmp);
    let middle = available.len() / 2;
    let median = if available.len() % 2 == 0 {
        (available[middle - 1] + available[middle]) / 2.0
    } else {
        available[middle]
    };
    Some((
        clamp_public(median / 0.02, -1.0, 1.0),
        format!(
            "{} broad indexes available · median day move {:+.2}%",
            available.len(),
            median * 100.0
        ),
    ))
}

fn clamp_public(value: f64, minimum: f64, maximum: f64) -> f64 {
    value.max(minimum).min(maximum)
}

pub async fn macro_snapshot(config: &MarketsConfig) -> Result<MacroSnapshot, String> {
    if config.fixture_mode {
        let hsi = simulated_quote("HK.800000");
        let hstech = simulated_quote("HK.800700");
        return Ok(MacroSnapshot {
            as_of: Utc::now().to_rfc3339(),
            hsi: Some(hsi.clone()),
            hstech: Some(hstech.clone()),
            risk_on_score: Some(0.5),
            factors: vec![MacroFactor {
                label: "Market breadth".into(),
                score: 0.5,
                detail: "Simulated development context; not market data.".into(),
                source: "fixture".into(),
            }],
            provenance: Provenance::simulated(),
            ..Default::default()
        });
    }

    let hk_symbols = vec!["HK.800000".to_string(), "HK.800700".to_string()];
    let hk_quotes = quote_many(config, &hk_symbols).await.unwrap_or_default();
    let hk_by_symbol = hk_quotes
        .into_iter()
        .map(|quote| (quote.symbol.clone(), quote))
        .collect::<HashMap<_, _>>();
    let (sp500, nasdaq, vix, us10y, dollar_index) = tokio::join!(
        macro_public_quote("US.SPX"),
        macro_public_quote("US.NASDAQ"),
        macro_public_quote("US.VIX"),
        macro_public_quote("US.US10Y"),
        macro_public_quote("US.DXY"),
    );
    let hsi = hk_by_symbol.get("HK.800000").cloned();
    let hstech = hk_by_symbol.get("HK.800700").cloned();
    let mut factors = Vec::new();
    if let Some((score, detail)) = macro_move_score(&[
        ("HSI", hsi.as_ref()),
        ("HSTECH", hstech.as_ref()),
        ("S&P 500", sp500.as_ref()),
        ("Nasdaq", nasdaq.as_ref()),
    ]) {
        factors.push(MacroFactor {
            label: "Market breadth / rally".into(),
            score,
            detail: format!(
                "{detail}; positive breadth supports cyclical beta, while a broad fall reduces conviction in isolated SELL calls."
            ),
            source: "Futu OpenD + public Yahoo chart".into(),
        });
    }
    if let Some(value) = vix.as_ref().and_then(|quote| quote.last) {
        let score = clamp_public((22.0 - value) / 10.0, -1.0, 1.0);
        factors.push(MacroFactor {
            label: "Volatility regime".into(),
            score,
            detail: format!(
                "VIX {:.2}; lower volatility supports risk-taking, while a spike signals a wider error bar.",
                value
            ),
            source: "public Yahoo chart".into(),
        });
    }
    if let Some(value) = us10y.as_ref().and_then(|quote| quote.last) {
        let score = clamp_public((4.25 - value) / 1.25, -1.0, 1.0) * 0.65;
        factors.push(MacroFactor {
            label: "Rates pressure".into(),
            score,
            detail: format!(
                "US 10Y yield {:.2}%; higher real discount rates can compress long-duration growth multiples.",
                value
            ),
            source: "public Yahoo chart".into(),
        });
    }
    if let Some(value) = dollar_index.as_ref().and_then(|quote| quote.last) {
        let score = clamp_public((104.0 - value) / 4.0, -1.0, 1.0) * 0.45;
        factors.push(MacroFactor {
            label: "Dollar impulse".into(),
            score,
            detail: format!(
                "Dollar index {:.2}; a stronger dollar can tighten global financial conditions.",
                value
            ),
            source: "public Yahoo chart".into(),
        });
    }
    let risk_on_score = (!factors.is_empty())
        .then(|| factors.iter().map(|factor| factor.score).sum::<f64>() / factors.len() as f64);
    let mut providers = factors
        .iter()
        .map(|factor| factor.source.clone())
        .collect::<Vec<_>>();
    providers.sort();
    providers.dedup();
    let has_live_hk = hsi
        .as_ref()
        .is_some_and(|quote| quote.provenance.freshness == Freshness::Live)
        || hstech
            .as_ref()
            .is_some_and(|quote| quote.provenance.freshness == Freshness::Live);
    let provenance = if risk_on_score.is_some() {
        Provenance {
            provider: if providers.is_empty() {
                "macro-sources".into()
            } else {
                providers.join(" + ")
            },
            observed_at: None,
            retrieved_at: Utc::now().to_rfc3339(),
            freshness: if has_live_hk {
                Freshness::Recent
            } else {
                Freshness::Delayed
            },
            realtime: has_live_hk,
            message: Some(
                "Macro overlay is contextual research data; public cross-asset series may be delayed and never authorizes an order."
                    .into(),
            ),
        }
    } else {
        Provenance::unavailable(
            "macro-sources",
            "No broad-market or cross-asset macro quote was returned.",
        )
    };
    Ok(MacroSnapshot {
        as_of: Utc::now().to_rfc3339(),
        hsi,
        hstech,
        sp500,
        nasdaq,
        vix,
        us10y,
        dollar_index,
        risk_on_score,
        factors,
        provenance,
    })
}

fn public_tencent_market_code(symbol: &str) -> Option<(String, String)> {
    let (market, code) = if let Some(code) = symbol.strip_prefix("HK.") {
        ("hk", format!("{:05}", code.parse::<u32>().ok()?))
    } else if let Some(code) = symbol.strip_prefix("SH.") {
        ("sh", format!("{:06}", code.parse::<u32>().ok()?))
    } else if let Some(code) = symbol.strip_prefix("SZ.") {
        ("sz", format!("{:06}", code.parse::<u32>().ok()?))
    } else {
        return None;
    };
    Some((market.into(), code))
}

fn parse_tencent_number(fields: &[&str], index: usize) -> Option<f64> {
    fields.get(index)?.trim().parse::<f64>().ok()
}

fn parse_tencent_timestamp(value: &str) -> Option<i64> {
    DateTime::parse_from_str(&format!("{value} +0800"), "%Y%m%d%H%M%S %z")
        .ok()
        .map(|value| value.timestamp())
        .or_else(|| parse_market_timestamp(&value.replace('/', "-")))
}

fn parse_tencent_quote(symbol: &str, body: &[u8]) -> Result<Quote, String> {
    let text = String::from_utf8_lossy(body);
    let payload = text
        .split('"')
        .nth(1)
        .ok_or_else(|| "Tencent quote returned no delimited payload.".to_string())?;
    let fields = payload.split('~').collect::<Vec<_>>();
    if fields.len() < 39 {
        return Err("Tencent quote returned too few fields.".into());
    }
    let last = parse_tencent_number(&fields, 3)
        .ok_or_else(|| "Tencent quote returned no last price.".to_string())?;
    let previous_close = parse_tencent_number(&fields, 4);
    let timestamp = fields
        .get(31)
        .and_then(|value| parse_tencent_timestamp(value));
    let candle = Candle {
        timestamp: timestamp.unwrap_or_else(|| Utc::now().timestamp()),
        open: parse_tencent_number(&fields, 5).unwrap_or(last),
        high: parse_tencent_number(&fields, 34).unwrap_or(last),
        low: parse_tencent_number(&fields, 35).unwrap_or(last),
        close: last,
        volume: parse_tencent_number(&fields, 6).unwrap_or(0.0),
        turnover: parse_tencent_number(&fields, 38),
    };
    let mut quote = public_quote_from_candles(
        symbol,
        std::slice::from_ref(&candle),
        "public-tencent",
        previous_close,
        timestamp
            .and_then(|value| DateTime::<Utc>::from_timestamp(value, 0))
            .map(|value| value.to_rfc3339()),
    );
    quote.turnover = candle.turnover;
    quote.market_session = "Tencent public quote".into();
    quote.market_cap = parse_tencent_number(&fields, 45).map(|value| value * 100_000_000.0);
    Ok(quote)
}

fn parse_tencent_candles(symbol: &str, value: &Value) -> Result<Vec<Candle>, String> {
    let (market, code) = public_tencent_market_code(symbol)
        .ok_or_else(|| format!("Tencent mapping is unavailable for {symbol}."))?;
    let rows = value
        .pointer(&format!("/data/{market}{code}/day"))
        .and_then(Value::as_array)
        .ok_or_else(|| "Tencent history returned no daily rows.".to_string())?;
    let candles = rows
        .iter()
        .filter_map(|row| {
            let row = row.as_array()?;
            if row.len() < 6 {
                return None;
            }
            Some(Candle {
                timestamp: parse_market_timestamp(row.first()?.as_str()?)?,
                open: row.get(1)?.as_str()?.parse().ok()?,
                close: row.get(2)?.as_str()?.parse().ok()?,
                high: row.get(3)?.as_str()?.parse().ok()?,
                low: row.get(4)?.as_str()?.parse().ok()?,
                volume: row.get(5)?.as_str()?.parse().ok()?,
                turnover: row
                    .get(6)
                    .and_then(Value::as_str)
                    .and_then(|value| value.parse().ok()),
            })
        })
        .collect::<Vec<_>>();
    if candles.is_empty() {
        return Err("Tencent history returned no usable daily rows.".into());
    }
    Ok(candles)
}

async fn public_tencent_data(symbol: &str, count: usize) -> Result<(Quote, Vec<Candle>), String> {
    let (market, code) = public_tencent_market_code(symbol)
        .ok_or_else(|| format!("Tencent mapping is unavailable for {symbol}."))?;
    let client = public_http_client()?;
    let quote_body = client
        .get(format!("https://qt.gtimg.cn/q={market}{code}"))
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|error| format!("Tencent quote request failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Tencent quote request failed: {error}"))?
        .bytes()
        .await
        .map_err(|error| format!("Tencent quote response could not be read: {error}"))?;
    let quote = parse_tencent_quote(symbol, &quote_body)?;
    let history_body = client
        .get(format!(
            "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get?param={market}{code},day,,,{count},qfq"
        ))
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|error| format!("Tencent history request failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Tencent history request failed: {error}"))?
        .json::<Value>()
        .await
        .map_err(|error| format!("Tencent history response was not JSON: {error}"))?;
    let candles = parse_tencent_candles(symbol, &history_body)?;
    Ok((quote, candles))
}

async fn public_stooq_data(symbol: &str) -> Result<(Quote, Vec<Candle>), String> {
    let ticker = stooq_ticker(symbol)
        .ok_or_else(|| format!("No public Stooq ticker mapping is available for {symbol}."))?;
    let client = public_http_client()?;
    let url = format!("https://stooq.com/q/d/l/?s={ticker}&i=d");
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("Stooq request failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Stooq request failed: {error}"))?;
    let body = response
        .text()
        .await
        .map_err(|error| format!("Stooq response could not be read: {error}"))?;
    let mut candles = body
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields = line.split(',').collect::<Vec<_>>();
            if fields.len() < 6 {
                return None;
            }
            Some(Candle {
                timestamp: parse_market_timestamp(fields[0])?,
                open: fields[1].parse().ok()?,
                high: fields[2].parse().ok()?,
                low: fields[3].parse().ok()?,
                close: fields[4].parse().ok()?,
                volume: fields[5].parse().unwrap_or(0.0),
                turnover: None,
            })
        })
        .collect::<Vec<_>>();
    candles.sort_by_key(|candle| candle.timestamp);
    if candles.is_empty() {
        return Err("Stooq returned no usable daily candles.".into());
    }
    let observed_at = candles
        .last()
        .and_then(|candle| DateTime::<Utc>::from_timestamp(candle.timestamp, 0))
        .map(|value| value.to_rfc3339());
    let quote = public_quote_from_candles(symbol, &candles, "public-stooq", None, observed_at);
    Ok((quote, candles))
}

async fn public_fallback_data(symbol: &str) -> Result<(Quote, Vec<Candle>), String> {
    let mut errors = Vec::new();
    if symbol.starts_with("HK.") || symbol.starts_with("SH.") || symbol.starts_with("SZ.") {
        match public_tencent_data(symbol, 500).await {
            Ok(data) => return Ok(data),
            Err(error) => errors.push(format!("Tencent: {error}")),
        }
    }
    match public_yahoo_data(symbol).await {
        Ok(data) => return Ok(data),
        Err(error) => errors.push(format!("Yahoo: {error}")),
    }
    match public_stooq_data(symbol).await {
        Ok(data) => return Ok(data),
        Err(error) => errors.push(format!("Stooq: {error}")),
    }
    Err(errors.join(" | "))
}

fn rss_tag(chunk: &str, tag: &str) -> Option<String> {
    let start = chunk.find(&format!("<{tag}"))?;
    let content_start = chunk[start..].find('>')? + start + 1;
    let end = chunk[content_start..].find(&format!("</{tag}>"))? + content_start;
    let value = chunk[content_start..end]
        .replace("<![CDATA[", "")
        .replace("]]>", "")
        .replace("&amp;", "&")
        .replace("&quot;", "\u{22}")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    Some(value.trim().to_string())
}

async fn public_news(symbol: &str) -> Vec<ResearchItem> {
    let Ok(client) = public_http_client() else {
        return Vec::new();
    };

    if let Some(ticker) = public_ticker(symbol) {
        if let Ok(url) = reqwest::Url::parse_with_params(
            "https://feeds.finance.yahoo.com/rss/2.0/headline",
            &[("s", ticker.as_str()), ("region", "US"), ("lang", "en-US")],
        ) {
            if let Ok(response) = client.get(url).send().await {
                if let Ok(response) = response.error_for_status() {
                    if let Ok(body) = response.text().await {
                        let items = body
                            .split("<item")
                            .skip(1)
                            .filter_map(|chunk| {
                                let title = rss_tag(chunk, "title")?;
                                let link = rss_tag(chunk, "link");
                                let detail = rss_tag(chunk, "description").unwrap_or_else(|| {
                                    "Public RSS item; open the source for the full article.".into()
                                });
                                Some(ResearchItem {
                                    title,
                                    detail,
                                    url: link,
                                    published_at: rss_tag(chunk, "pubDate"),
                                    source: "public-yahoo-rss".into(),
                                })
                            })
                            .take(8)
                            .collect::<Vec<_>>();
                        if !items.is_empty() {
                            return items;
                        }
                    }
                }
            }
        }
    }

    let query = format!("{} {}", security_name(symbol), symbol.replace('.', " "));
    let Ok(url) = reqwest::Url::parse_with_params(
        "https://news.google.com/rss/search",
        &[
            ("q", query.as_str()),
            ("hl", "en-US"),
            ("gl", "US"),
            ("ceid", "US:en"),
        ],
    ) else {
        return Vec::new();
    };
    let Ok(response) = client.get(url).send().await else {
        return Vec::new();
    };
    let Ok(response) = response.error_for_status() else {
        return Vec::new();
    };
    let Ok(body) = response.text().await else {
        return Vec::new();
    };
    body.split("<item")
        .skip(1)
        .filter_map(|chunk| {
            let title = rss_tag(chunk, "title")?;
            let link = rss_tag(chunk, "link");
            let detail = rss_tag(chunk, "description").unwrap_or_else(|| {
                "Public Google News RSS item; open the source for the full article.".into()
            });
            Some(ResearchItem {
                title,
                detail,
                url: link,
                published_at: rss_tag(chunk, "pubDate"),
                source: "public-google-news-rss".into(),
            })
        })
        .take(8)
        .collect()
}

fn public_filing_search(symbol: &str) -> ResearchItem {
    let (title, detail, url, source) = if symbol.starts_with("HK.") {
        (
            "HKEX issuer announcements search",
            "Official HKEX search entry point. Search the issuer code to review announcements and filings.",
            "https://www.hkexnews.hk/search/titlesearch.xhtml?lang=en",
            "hkex-public-search",
        )
    } else {
        (
            "SEC company filings search",
            "Official SEC search entry point. Search the issuer ticker or name to review filings.",
            "https://www.sec.gov/edgar/search/",
            "sec-public-search",
        )
    };
    ResearchItem {
        title: title.into(),
        detail: format!("{detail} Security: {symbol}."),
        url: Some(url.into()),
        published_at: None,
        source: source.into(),
    }
}

fn research_value(value: &Value) -> Option<String> {
    if let Some(number) = value.as_f64() {
        if !number.is_finite() {
            return None;
        }
        let absolute = number.abs();
        return Some(if absolute >= 1_000_000_000.0 {
            format!("{:.2}B", number / 1_000_000_000.0)
        } else if absolute >= 1_000_000.0 {
            format!("{:.2}M", number / 1_000_000.0)
        } else {
            format!("{number:.4}")
        });
    }
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub async fn company_research(
    config: &MarketsConfig,
    input: &str,
) -> Result<CompanyResearch, String> {
    let symbol = normalize_symbol(input)?;
    if config.fixture_mode {
        return Ok(CompanyResearch {
            symbol: symbol.clone(),
            name: security_name(&symbol).into(),
            profile: vec![ResearchFact {
                label: "Data mode".into(),
                value: "Clearly labelled development fixture".into(),
                period: None,
                source: "fixture".into(),
            }],
            financials: Vec::new(),
            news: Vec::new(),
            filings: vec![public_filing_search(&symbol)],
            provenance: Provenance::simulated(),
        });
    }

    let symbols = std::slice::from_ref(&symbol);
    let profile_value =
        futu_call_value("profile", &config.futu_host, config.futu_port, symbols, 10)
            .await
            .ok();
    let financial_value = futu_call_value(
        "financials",
        &config.futu_host,
        config.futu_port,
        symbols,
        3,
    )
    .await
    .ok();

    let profile = profile_value
        .as_ref()
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let label = row.get("name").and_then(Value::as_str)?.trim();
                    let value = row.get("value").and_then(Value::as_str)?.trim();
                    if label.is_empty() || value.is_empty() || matches!(label, "Fax" | "Email") {
                        return None;
                    }
                    Some(ResearchFact {
                        label: label.into(),
                        value: value.into(),
                        period: None,
                        source: "futu-company-profile".into(),
                    })
                })
                .take(12)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let name = profile
        .iter()
        .find(|fact| fact.label == "Company Name")
        .map(|fact| fact.value.clone())
        .unwrap_or_else(|| security_name(&symbol).into());

    let financials = financial_value
        .as_ref()
        .and_then(|value| value.get("report_list"))
        .and_then(Value::as_array)
        .map(|reports| {
            reports
                .iter()
                .take(3)
                .flat_map(|report| {
                    let period = report
                        .get("period_text")
                        .and_then(Value::as_str)
                        .unwrap_or("latest")
                        .to_string();
                    let currency = report
                        .get("currency_code")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    report
                        .get("item_list")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(move |item| {
                            let label = item.get("display_name").and_then(Value::as_str)?;
                            let normalized = label.to_ascii_lowercase();
                            let supported = matches!(
                                label,
                                "Total Revenue"
                                    | "Gross Profit"
                                    | "Net Profit"
                                    | "Diluted EPS"
                                    | "Operating Profit"
                                    | "Operating Income"
                                    | "Total Assets"
                                    | "Total Liabilities"
                                    | "Cash and Cash Equivalents"
                                    | "Net Cash Flow from Operating Activities"
                            ) || normalized.contains("research and development")
                                || normalized == "r&d expense"
                                || normalized.contains("capital expenditure");
                            if !supported {
                                return None;
                            }
                            let mut value = research_value(item.get("data")?)?;
                            if !currency.is_empty() && label != "Diluted EPS" {
                                value = format!("{currency} {value}");
                            }
                            Some(ResearchFact {
                                label: label.into(),
                                value,
                                period: Some(period.clone()),
                                source: "futu-financial-statements".into(),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let news = public_news(&symbol).await;
    let filings = vec![public_filing_search(&symbol)];
    let provenance = if !profile.is_empty() || !financials.is_empty() {
        Provenance {
            provider: "futu-opend".into(),
            observed_at: None,
            retrieved_at: Utc::now().to_rfc3339(),
            freshness: Freshness::Recent,
            realtime: false,
            message: Some(
                "Profile and financial facts came from the local Futu OpenD read-only bridge."
                    .into(),
            ),
        }
    } else if !news.is_empty() {
        Provenance {
            provider: "public-yahoo-rss".into(),
            observed_at: None,
            retrieved_at: Utc::now().to_rfc3339(),
            freshness: Freshness::Delayed,
            realtime: false,
            message: Some(format!(
                "{PUBLIC_FALLBACK_MESSAGE} Official filing search links remain available."
            )),
        }
    } else {
        Provenance::unavailable(
            "futu-opend+public-research",
            "No profile or public news response was returned. The official filing search link remains available.",
        )
    };
    Ok(CompanyResearch {
        symbol,
        name,
        profile,
        financials,
        news,
        filings,
        provenance,
    })
}

async fn fallback_quote(symbol: &str, futu_error: &str) -> Quote {
    match public_fallback_data(symbol).await {
        Ok((mut quote, _)) => {
            quote.provenance.message = Some(format!(
                "Futu was unavailable ({futu_error}). {PUBLIC_FALLBACK_MESSAGE}"
            ));
            quote
        }
        Err(public_error) => unavailable_quote_with_provider(
            symbol,
            "futu-opend+public-fallbacks",
            format!("Futu failed: {futu_error}; public fallbacks failed: {public_error}"),
        ),
    }
}

pub async fn quote(config: &MarketsConfig, input: &str) -> Result<Quote, String> {
    let symbol = normalize_symbol(input)?;
    Ok(quote_many(config, std::slice::from_ref(&symbol))
        .await?
        .into_iter()
        .next()
        .unwrap_or_else(|| unavailable_quote(&symbol, "Futu returned no quote for this security.")))
}

pub async fn quote_many(config: &MarketsConfig, inputs: &[String]) -> Result<Vec<Quote>, String> {
    let symbols = inputs
        .iter()
        .map(|input| normalize_symbol(input))
        .collect::<Result<Vec<_>, _>>()?;
    if config.fixture_mode {
        return Ok(symbols
            .iter()
            .map(|symbol| simulated_quote(symbol))
            .collect());
    }
    let batch_rows = futu_call(
        "quote",
        &config.futu_host,
        config.futu_port,
        &symbols,
        symbols.len(),
    )
    .await;
    let mut quotes = Vec::with_capacity(symbols.len());
    for symbol in &symbols {
        let quote = batch_rows.as_ref().ok().and_then(|rows| {
            rows.iter()
                .find(|row| row.get("code").and_then(Value::as_str) == Some(symbol))
                .map(|row| quote_from_bridge(symbol, row))
        });
        if let Some(quote) = quote {
            quotes.push(quote);
        } else {
            // Futu rejects an entire snapshot when one symbol lacks permission
            // (for example, a mainland index without CN market entitlements).
            // Retry the symbol alone so permitted HK/US instruments stay live.
            match futu_call(
                "quote",
                &config.futu_host,
                config.futu_port,
                std::slice::from_ref(symbol),
                1,
            )
            .await
            {
                Ok(rows) => {
                    if let Some(row) = rows.first() {
                        quotes.push(quote_from_bridge(symbol, row));
                        continue;
                    }
                    quotes.push(
                        fallback_quote(symbol, "Futu returned no quote for this security.").await,
                    );
                }
                Err(error) => quotes.push(fallback_quote(symbol, &error).await),
            }
        }
    }
    Ok(quotes)
}

pub fn hsi_constituent_symbols() -> Vec<String> {
    Vec::new()
}

pub fn hstech_constituent_symbols() -> Vec<String> {
    Vec::new()
}

pub fn thematic_technology_companies() -> Vec<ThematicCompany> {
    Vec::new()
}

fn thematic_hk_symbols() -> Vec<String> {
    thematic_technology_companies()
        .into_iter()
        .filter(|company| company.symbol.starts_with("HK."))
        .map(|company| company.symbol)
        .collect()
}

pub async fn universe(config: &MarketsConfig) -> Result<UniverseSnapshot, String> {
    let hsi_symbols = hsi_constituent_symbols();
    let hstech_symbols = hstech_constituent_symbols();
    let thematic_symbols = thematic_hk_symbols();
    let mut symbols = hsi_symbols.clone();
    symbols.extend(hstech_symbols.iter().cloned());
    symbols.extend(thematic_symbols.iter().cloned());
    symbols.sort();
    symbols.dedup();
    let quotes = quote_many(config, &symbols).await?;
    let by_symbol = quotes
        .into_iter()
        .map(|quote| (quote.symbol.clone(), quote))
        .collect::<HashMap<_, _>>();
    let ordered_quotes = |symbols: &[String]| {
        symbols
            .iter()
            .map(|symbol| {
                by_symbol.get(symbol).cloned().unwrap_or_else(|| {
                    unavailable_quote(symbol, "No quote returned for this constituent.")
                })
            })
            .collect::<Vec<_>>()
    };
    let mut thematic = thematic_technology_companies();
    for company in &mut thematic {
        if let Some(quote) = by_symbol.get(&company.symbol) {
            company.quote = Some(quote.clone());
        }
    }
    Ok(UniverseSnapshot {
        retrieved_at: Utc::now().to_rfc3339(),
        constituent_source: "No constituent or thematic company lists are bundled. Add symbols manually; quotes are fetched from configured providers.".into(),
        hsi: ordered_quotes(&hsi_symbols),
        hstech: ordered_quotes(&hstech_symbols),
        thematic,
    })
}

pub async fn history(config: &MarketsConfig, input: &str, count: usize) -> Result<History, String> {
    let symbol = normalize_symbol(input)?;
    let count = count.clamp(30, 500);
    if config.fixture_mode {
        return Ok(simulated_history(&symbol, count));
    }
    match futu_call(
        "history",
        &config.futu_host,
        config.futu_port,
        std::slice::from_ref(&symbol),
        count,
    )
    .await
    {
        Ok(rows) => {
            let candles = rows
                .iter()
                .filter_map(candle_from_bridge)
                .collect::<Vec<_>>();
            if !candles.is_empty() {
                Ok(History {
                    symbol,
                    interval: "1D".into(),
                    indicators: technicals(&candles),
                    candles,
                    provenance: Provenance {
                        provider: "futu-opend".into(),
                        observed_at: None,
                        retrieved_at: Utc::now().to_rfc3339(),
                        freshness: Freshness::Recent,
                        realtime: false,
                        message: Some("Historical candles returned by Futu OpenD.".into()),
                    },
                })
            } else {
                public_history_fallback(&symbol, count, "Futu returned no usable candles.").await
            }
        }
        Err(error) => public_history_fallback(&symbol, count, &error).await,
    }
}

async fn public_history_fallback(
    symbol: &str,
    count: usize,
    futu_error: &str,
) -> Result<History, String> {
    match public_fallback_data(symbol).await {
        Ok((mut quote, candles)) => {
            let candles = truncate_candles(candles, count);
            quote.provenance.message = Some(format!(
                "Futu was unavailable ({futu_error}). {PUBLIC_FALLBACK_MESSAGE}"
            ));
            Ok(History {
                symbol: symbol.into(),
                interval: "1D".into(),
                indicators: technicals(&candles),
                candles,
                provenance: quote.provenance,
            })
        }
        Err(public_error) => Ok(History {
            symbol: symbol.into(),
            interval: "1D".into(),
            candles: Vec::new(),
            indicators: TechnicalSnapshot::default(),
            provenance: Provenance::unavailable(
                "futu-opend+public-fallbacks",
                format!("Futu failed: {futu_error}; public fallbacks failed: {public_error}"),
            ),
        }),
    }
}

pub fn pnl(
    shares: f64,
    average_cost: f64,
    price: Option<f64>,
) -> (Option<f64>, Option<f64>, Option<f64>) {
    let Some(price) = price else {
        return (None, None, None);
    };
    let market_value = shares * price;
    let cost = shares * average_cost;
    let profit = market_value - cost;
    (
        Some(market_value),
        Some(profit),
        (cost != 0.0).then_some(profit / cost),
    )
}

pub fn preview_order(
    symbol: &str,
    side: &str,
    quantity: f64,
    limit_price: Option<f64>,
    quote: &Quote,
    trading_mode: &str,
) -> OrderPreview {
    let mut issues = Vec::new();
    let normalized = normalize_symbol(symbol).unwrap_or_else(|_| symbol.to_ascii_uppercase());
    let side = side.to_ascii_uppercase();
    if !matches!(side.as_str(), "BUY" | "SELL") {
        issues.push("Side must be BUY or SELL.".into());
    }
    if quantity <= 0.0 || !quantity.is_finite() {
        issues.push("Quantity must be a positive finite number.".into());
    }
    if normalized.starts_with("HK.") && quantity.fract() != 0.0 {
        issues.push("HK shares must be a whole number of shares.".into());
    }
    if quote.provenance.freshness == Freshness::Unavailable || quote.last.is_none() {
        issues.push("A current quote is unavailable; order submission is blocked.".into());
    }
    if quote.provenance.freshness == Freshness::Delayed {
        issues.push(
            "The quote is public fallback data and may be delayed; order submission is blocked."
                .into(),
        );
    }
    if quote.provenance.freshness == Freshness::Stale
        || quote.provenance.freshness == Freshness::Cached
    {
        issues.push("The quote is stale or cached; refresh before trading.".into());
    }
    if trading_mode == "OFF" {
        issues.push("Trading is OFF. Enable PAPER explicitly for a simulated order.".into());
    }
    let estimated_notional = limit_price.or(quote.last).map(|price| price * quantity);
    OrderPreview {
        symbol: normalized,
        side,
        quantity,
        order_type: if limit_price.is_some() {
            "LIMIT"
        } else {
            "MARKET"
        }
        .into(),
        limit_price,
        estimated_notional,
        market_state: quote.market_session.clone(),
        valid: issues.is_empty() && trading_mode == "PAPER",
        issues,
        trading_mode: trading_mode.into(),
        live_submission_available: false,
        provenance: quote.provenance.clone(),
    }
}

pub fn radar_from_quotes(quotes: &[Quote]) -> Vec<RadarEvent> {
    quotes
        .iter()
        .filter_map(|quote| {
            let change = quote.change_pct?;
            if quote.provenance.freshness == Freshness::Unavailable || change.abs() < 0.03 {
                return None;
            }
            Some(RadarEvent {
                id: Uuid::new_v4().to_string(),
                symbol: quote.symbol.clone(),
                title: format!("{} moved {:+.1}%", quote.symbol, change * 100.0),
                detail: "Deterministic price-move event; investigate filings and news before drawing conclusions.".into(),
                severity: if change.abs() >= 0.05 { "high".into() } else { "medium".into() },
                provenance: quote.provenance.clone(),
            })
        })
        .collect()
}

pub fn health_json(health: &FutuHealth) -> Value {
    json!({
        "connection": health.connection_state,
        "tcp_reachable": health.tcp_reachable,
        "sdk_available": health.sdk_available,
        "login": health.login_state,
        "quote_permissions": health.quote_permissions,
        "checked_at": health.checked_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candle(timestamp: i64, close: f64) -> Candle {
        Candle {
            timestamp,
            open: close,
            high: close,
            low: close,
            close,
            volume: 1.0,
            turnover: None,
        }
    }

    #[test]
    fn normalizes_hk_index_aliases_and_tickers() {
        assert_eq!(normalize_symbol("12345").unwrap(), "HK.12345");
        assert_eq!(normalize_symbol("12345.HK").unwrap(), "HK.12345");
        assert_eq!(normalize_symbol("HK.800000").unwrap(), "HK.800000");
        assert_eq!(normalize_symbol("HSI").unwrap(), "HK.800000");
        assert_eq!(normalize_symbol("HK.12345").unwrap(), "HK.12345");
    }

    #[test]
    fn exposes_current_hang_seng_universe_sizes() {
        assert!(hsi_constituent_symbols().is_empty());
        assert!(hstech_constituent_symbols().is_empty());
        assert!(thematic_technology_companies().is_empty());
    }

    #[test]
    fn prefers_provider_security_names() {
        let quote = quote_from_bridge(
            "HK.12345",
            &json!({
                "name": "SAMPLE-HK",
                "last_price": 1.5,
                "prev_close_price": 1.4,
            }),
        );
        assert_eq!(quote.name, "SAMPLE-HK");
    }

    #[test]
    fn parses_futu_datetime_rows() {
        let row = json!({
            "time_key": "2025-01-02 09:30:00",
            "open": 1.0,
            "high": 2.0,
            "low": 0.5,
            "close": 1.5,
            "volume": 10,
        });
        assert_eq!(
            candle_from_bridge(&row).unwrap().timestamp,
            DateTime::parse_from_rfc3339("2025-01-02T09:30:00+08:00")
                .unwrap()
                .timestamp()
        );
    }

    #[test]
    fn futu_quote_observation_uses_snapshot_or_basic_quote_time() {
        let snapshot = quote_from_bridge(
            "HK.12345",
            &json!({"last_price": 1.5, "update_time": "2025-01-02 16:00:00"}),
        );
        let basic = quote_from_bridge(
            "HK.12345",
            &json!({"last_price": 1.5, "data_date": "2025-01-02", "data_time": "16:00:00"}),
        );
        assert_eq!(
            snapshot.provenance.observed_at.as_deref(),
            Some("2025-01-02T08:00:00+00:00")
        );
        assert_eq!(
            snapshot.provenance.observed_at,
            basic.provenance.observed_at
        );
        let missing = quote_from_bridge(
            "HK.12345",
            &json!({"last_price": 1.5, "data_time": "16:00:00"}),
        );
        assert!(missing.provenance.observed_at.is_none());
        assert!(!missing.provenance.realtime);
        assert_ne!(missing.provenance.freshness, Freshness::Live);
        let us = quote_from_bridge("US.EXAMPLE", &json!({"update_time": "2025-01-02 16:00:00"}));
        assert!(us.provenance.observed_at.is_none());
    }

    #[test]
    fn maps_supported_symbols_to_public_fallback_tickers() {
        assert_eq!(public_ticker("HK.12345").as_deref(), Some("12345.HK"));
        assert_eq!(public_ticker("HK.800000").as_deref(), Some("^HSI"));
        assert_eq!(public_ticker("SH.000001").as_deref(), Some("000001.SS"));
        assert_eq!(public_ticker("US.EXAMPLE").as_deref(), Some("EXAMPLE"));
        assert_eq!(stooq_ticker("HK.12345").as_deref(), Some("12345.hk"));
        assert_eq!(
            public_tencent_market_code("SH.000001"),
            Some(("sh".into(), "000001".into()))
        );
        assert_eq!(
            public_tencent_market_code("SZ.000001"),
            Some(("sz".into(), "000001".into()))
        );
    }

    #[test]
    fn merges_candles_by_timestamp_and_sorts() {
        let merged = merge_candles(
            vec![candle(2, 2.0), candle(1, 1.0)],
            vec![candle(2, 3.0), candle(3, 4.0)],
        );
        assert_eq!(
            merged.iter().map(|value| value.close).collect::<Vec<_>>(),
            vec![1.0, 3.0, 4.0]
        );
    }

    #[test]
    fn computes_basic_indicators_without_llm_math() {
        let candles = (0..70)
            .map(|index| candle(index, 100.0 + index as f64))
            .collect::<Vec<_>>();
        let snapshot = technicals(&candles);
        assert_eq!(snapshot.sma20, Some(159.5));
        assert!(snapshot.ema20.is_some());
        assert!(snapshot.rsi14.unwrap() > 99.0);
        assert_eq!(snapshot.max_drawdown, Some(0.0));
    }

    #[test]
    fn blocks_live_order_submission_and_stale_quotes() {
        let mut quote = simulated_quote("HK.12345");
        quote.provenance.freshness = Freshness::Cached;
        let preview = preview_order("700", "BUY", 100.0, None, &quote, "LIVE_WITH_APPROVAL");
        assert!(!preview.valid);
        assert!(preview.issues.iter().any(|issue| issue.contains("stale")));
        assert!(!preview.live_submission_available);
    }

    #[test]
    fn blocks_order_preview_for_public_delayed_quotes() {
        let mut quote = simulated_quote("HK.12345");
        quote.provenance.freshness = Freshness::Delayed;
        let preview = preview_order("700", "BUY", 100.0, None, &quote, "PAPER");
        assert!(!preview.valid);
        assert!(preview
            .issues
            .iter()
            .any(|issue| issue.contains("public fallback")));
    }
}
