//! Shared test support: a minimal JSON reader (std only), the frozen signal
//! generator, and a few helpers.

#![allow(dead_code)]

use std::collections::BTreeMap;

pub const SIGNALS_JSON: &str = include_str!("../fixtures/conformance-signals.json");

// JSON

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    pub fn get(&self, key: &str) -> &Json {
        match self {
            Json::Object(map) => map.get(key).unwrap_or(&Json::Null),
            _ => &Json::Null,
        }
    }
    pub fn as_f64(&self) -> f64 {
        match self {
            Json::Number(n) => *n,
            other => panic!("expected number, got {other:?}"),
        }
    }
    pub fn as_u64(&self) -> u64 {
        match self {
            Json::String(s) => s.parse().expect("u64 string"),
            Json::Number(n) => *n as u64,
            other => panic!("expected u64, got {other:?}"),
        }
    }
    pub fn as_bool(&self) -> bool {
        match self {
            Json::Bool(b) => *b,
            other => panic!("expected bool, got {other:?}"),
        }
    }
    pub fn as_str(&self) -> &str {
        match self {
            Json::String(s) => s,
            other => panic!("expected string, got {other:?}"),
        }
    }
    pub fn as_array(&self) -> &[Json] {
        match self {
            Json::Array(a) => a,
            other => panic!("expected array, got {other:?}"),
        }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }
    fn peek(&self) -> u8 {
        self.bytes[self.pos]
    }
    fn expect(&mut self, b: u8) {
        assert_eq!(
            self.peek(),
            b,
            "json: expected {:?} at {}",
            b as char,
            self.pos
        );
        self.pos += 1;
    }
    fn value(&mut self) -> Json {
        self.skip_ws();
        match self.peek() {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => Json::String(self.string()),
            b't' => self.literal("true", Json::Bool(true)),
            b'f' => self.literal("false", Json::Bool(false)),
            b'n' => self.literal("null", Json::Null),
            _ => self.number(),
        }
    }
    fn literal(&mut self, word: &str, value: Json) -> Json {
        assert!(self.bytes[self.pos..].starts_with(word.as_bytes()));
        self.pos += word.len();
        value
    }
    fn number(&mut self) -> Json {
        let start = self.pos;
        while self.pos < self.bytes.len()
            && matches!(
                self.bytes[self.pos],
                b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
            )
        {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap();
        Json::Number(
            text.parse()
                .unwrap_or_else(|_| panic!("json: bad number {text:?}")),
        )
    }
    fn string(&mut self) -> String {
        self.expect(b'"');
        let mut out = String::new();
        loop {
            let b = self.peek();
            self.pos += 1;
            match b {
                b'"' => return out,
                b'\\' => {
                    let e = self.peek();
                    self.pos += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'u' => {
                            let hex =
                                std::str::from_utf8(&self.bytes[self.pos..self.pos + 4]).unwrap();
                            self.pos += 4;
                            let code = u32::from_str_radix(hex, 16).unwrap();
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        other => panic!("json: bad escape {:?}", other as char),
                    }
                }
                _ => {
                    // Copy the raw UTF-8 byte run.
                    let start = self.pos - 1;
                    let mut end = self.pos;
                    while end < self.bytes.len()
                        && self.bytes[end] != b'"'
                        && self.bytes[end] != b'\\'
                    {
                        end += 1;
                    }
                    out.push_str(std::str::from_utf8(&self.bytes[start..end]).unwrap());
                    self.pos = end;
                }
            }
        }
    }
    fn array(&mut self) -> Json {
        self.expect(b'[');
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == b']' {
            self.pos += 1;
            return Json::Array(items);
        }
        loop {
            items.push(self.value());
            self.skip_ws();
            match self.peek() {
                b',' => self.pos += 1,
                b']' => {
                    self.pos += 1;
                    return Json::Array(items);
                }
                other => panic!("json: unexpected {:?} in array", other as char),
            }
        }
    }
    fn object(&mut self) -> Json {
        self.expect(b'{');
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == b'}' {
            self.pos += 1;
            return Json::Object(map);
        }
        loop {
            self.skip_ws();
            let key = self.string();
            self.skip_ws();
            self.expect(b':');
            let value = self.value();
            map.insert(key, value);
            self.skip_ws();
            match self.peek() {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    return Json::Object(map);
                }
                other => panic!("json: unexpected {:?} in object", other as char),
            }
        }
    }
}

pub fn parse_json(text: &str) -> Json {
    let mut p = Parser {
        bytes: text.as_bytes(),
        pos: 0,
    };
    let v = p.value();
    p.skip_ws();
    assert_eq!(p.pos, p.bytes.len(), "json: trailing data");
    v
}

// Frozen signals

#[derive(Debug, Clone)]
pub struct Component {
    pub freq_hz: f64,
    pub amp: f64,
    pub phase_rad: f64,
}

#[derive(Debug, Clone)]
pub struct Expected {
    pub expect_pitch: bool,
    pub f0_hz: f64,
    pub tol_cents: f64,
    pub clarity_min: f64,
}

#[derive(Debug, Clone)]
pub struct Signal {
    pub id: String,
    pub sample_rate: u32,
    pub num_samples: usize,
    pub components: Vec<Component>,
    pub noise: Option<(f64, u64)>,
    pub dc: f64,
    pub expected: Expected,
    pub first_8: Vec<f64>,
    pub sum_sq: f64,
}

impl Signal {
    pub fn label(&self) -> String {
        format!("{}@{}", self.id, self.sample_rate)
    }
    pub fn is_pure_tone(&self) -> bool {
        self.components.len() == 1 && self.noise.is_none() && self.dc == 0.0
    }
}

pub struct ReferenceParams {
    pub mpm_window: usize,
    pub mpm_tau_max: usize,
    pub mpm_k: f64,
    pub yin_frame: usize,
    pub yin_window: usize,
    pub yin_threshold: f64,
}

pub struct SignalSuite {
    pub signals: Vec<Signal>,
    pub params: ReferenceParams,
    pub lcg_a: u64,
    pub lcg_c: u64,
}

pub fn load_signals() -> SignalSuite {
    let root = parse_json(SIGNALS_JSON);
    assert_eq!(root.get("schema").as_str(), "pitch-conformance-signals/1");
    assert!(root.get("frozen").as_bool());
    let lcg = root.get("generator").get("noise_lcg");
    let lcg_a = lcg.get("a").as_u64();
    let lcg_c = lcg.get("c").as_u64();
    let rp = root.get("reference_params");
    let params = ReferenceParams {
        mpm_window: rp.get("mpm").get("window").as_f64() as usize,
        mpm_tau_max: rp.get("mpm").get("tau_max").as_f64() as usize,
        mpm_k: rp.get("mpm").get("k_relative_threshold").as_f64(),
        yin_frame: rp.get("yin").get("frame").as_f64() as usize,
        yin_window: rp.get("yin").get("integration_window_W").as_f64() as usize,
        yin_threshold: rp.get("yin").get("absolute_threshold").as_f64(),
    };
    let signals = root
        .get("signals")
        .as_array()
        .iter()
        .map(|s| {
            let expected = s.get("expected");
            let expect_pitch = expected.get("expect_pitch").as_bool();
            Signal {
                id: s.get("id").as_str().to_string(),
                sample_rate: s.get("sample_rate").as_f64() as u32,
                num_samples: s.get("num_samples").as_f64() as usize,
                components: s
                    .get("components")
                    .as_array()
                    .iter()
                    .map(|c| Component {
                        freq_hz: c.get("freq_hz").as_f64(),
                        amp: c.get("amp").as_f64(),
                        phase_rad: c.get("phase_rad").as_f64(),
                    })
                    .collect(),
                noise: if s.get("noise").is_null() {
                    None
                } else {
                    Some((
                        s.get("noise").get("amp").as_f64(),
                        s.get("noise").get("lcg_seed").as_u64(),
                    ))
                },
                dc: s.get("dc").as_f64(),
                expected: Expected {
                    expect_pitch,
                    f0_hz: if expect_pitch {
                        expected.get("f0_hz").as_f64()
                    } else {
                        0.0
                    },
                    tol_cents: if expect_pitch {
                        expected.get("tol_cents").as_f64()
                    } else {
                        0.0
                    },
                    clarity_min: if expect_pitch {
                        expected.get("clarity_min").as_f64()
                    } else {
                        0.0
                    },
                },
                first_8: s
                    .get("fingerprint")
                    .get("first_8")
                    .as_array()
                    .iter()
                    .map(Json::as_f64)
                    .collect(),
                sum_sq: s.get("fingerprint").get("sum_sq").as_f64(),
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(signals.len(), 28, "conformance roster");
    SignalSuite {
        signals,
        params,
        lcg_a,
        lcg_c,
    }
}

/// MMIX LCG, 64-bit wrapping, advanced before the first sample.
pub struct Lcg {
    state: u64,
    a: u64,
    c: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed,
            a: 6364136223846793005,
            c: 1442695040888963407,
        }
    }
    pub fn with_constants(seed: u64, a: u64, c: u64) -> Self {
        Self { state: seed, a, c }
    }
    pub fn next_unit(&mut self) -> f64 {
        self.state = self.state.wrapping_mul(self.a).wrapping_add(self.c);
        ((self.state >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
    }
    pub fn frame(&mut self, len: usize) -> Vec<f64> {
        (0..len).map(|_| self.next_unit()).collect()
    }
}

/// Regenerate a signal from its spec and validate it against the stored
/// fingerprint (first 8 samples to 1e-6 abs, sum of squares to 1e-3 rel).
pub fn generate(suite: &SignalSuite, s: &Signal) -> Vec<f64> {
    let mut lcg = s
        .noise
        .map(|(_, seed)| Lcg::with_constants(seed, suite.lcg_a, suite.lcg_c));
    let sr = f64::from(s.sample_rate);
    let out: Vec<f64> = (0..s.num_samples)
        .map(|i| {
            let t = i as f64 / sr;
            let mut v = s.dc;
            for c in &s.components {
                v += c.amp * (2.0 * std::f64::consts::PI * c.freq_hz * t + c.phase_rad).sin();
            }
            if let (Some((amp, _)), Some(lcg)) = (s.noise, lcg.as_mut()) {
                v += amp * lcg.next_unit();
            }
            v
        })
        .collect();
    for (i, (a, e)) in out.iter().zip(&s.first_8).enumerate() {
        assert!(
            (a - e).abs() <= 1e-6,
            "{}: fingerprint sample {i}: generated {a} stored {e}",
            s.label()
        );
    }
    let sum_sq: f64 = out.iter().map(|v| v * v).sum();
    let tol = 1e-3 * s.sum_sq.abs().max(1e-9);
    assert!(
        (sum_sq - s.sum_sq).abs() <= tol,
        "{}: fingerprint sum_sq: generated {sum_sq} stored {}",
        s.label(),
        s.sum_sq
    );
    out
}

pub fn to_f32(v: &[f64]) -> Vec<f32> {
    v.iter().map(|&x| x as f32).collect()
}

pub fn cents(f: f64, f0: f64) -> f64 {
    1200.0 * (f / f0).log2()
}

pub fn sample_rates(suite: &SignalSuite) -> Vec<u32> {
    let mut rates: Vec<u32> = suite.signals.iter().map(|s| s.sample_rate).collect();
    rates.sort_unstable();
    rates.dedup();
    rates
}
