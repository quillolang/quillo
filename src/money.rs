//! Money: an amount of a currency, as a map `{ amount: dec, currency: "USD" }`.
//!
//! ```text
//! let price = money.of(19.99m, "USD")          // { amount: 19.99m, currency: "USD" }
//! money.of("12.345", "USD")                    // err: more decimals than USD has
//! money.from_minor(1250, "USD")  money.to_minor(price)   // 12.50 and 1999 cents
//! money.add(a, b)  money.sub(a, b)  money.mul(price, 3)  money.neg(price)
//! money.allocate(money.of(100, "USD"), [1, 1, 1])        // 33.34, 33.33, 33.33
//! money.format(price, "de-DE")                 // "19,99 $"
//! ```
//!
//! A money value is a plain map, so JSON, the database, the browser and
//! every map function take it as they are; `money` checks it each time it
//! is given one. Amounts always have the currency's minor units (ISO 4217:
//! JPY 0, USD 2, BHD 3): `money.of` refuses more decimals unless told how
//! to round, and what multiplies rounds to them. Two currencies never mix.
//!
//! `money.format` writes an amount as CLDR's data has it for a locale (the
//! same as `Intl.NumberFormat` in a browser): the locales in `LOCALES`, and
//! another by its language (`de-LU` as `de-DE`), else as `en-US`.

use crate::decimal::{Dec, Rounding};
use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use std::collections::HashMap;
use std::sync::OnceLock;

type R = Result<Value, Ctrl>;

/// The active ISO 4217 currencies and their minor units (as of 2026; ANG,
/// BGN, HRK, SLL and ZWL are withdrawn, and codes without minor units, such
/// as XAU and XDR, are not money here). runtime.js has the same table.
pub const CURRENCIES: &str = "AED2 AFN2 ALL2 AMD2 AOA2 ARS2 AUD2 AWG2 AZN2 BAM2 BBD2 BDT2 BHD3 BIF0 BMD2 BND2 BOB2 BOV2 BRL2 BSD2 \
    BTN2 BWP2 BYN2 BZD2 CAD2 CDF2 CHE2 CHF2 CHW2 CLF4 CLP0 CNY2 COP2 COU2 CRC2 CUC2 CUP2 CVE2 CZK2 DJF0 \
    DKK2 DOP2 DZD2 EGP2 ERN2 ETB2 EUR2 FJD2 FKP2 GBP2 GEL2 GHS2 GIP2 GMD2 GNF0 GTQ2 GYD2 HKD2 HNL2 HTG2 \
    HUF2 IDR2 ILS2 INR2 IQD3 IRR2 ISK0 JMD2 JOD3 JPY0 KES2 KGS2 KHR2 KMF0 KPW2 KRW0 KWD3 KYD2 KZT2 LAK2 \
    LBP2 LKR2 LRD2 LSL2 LYD3 MAD2 MDL2 MGA2 MKD2 MMK2 MNT2 MOP2 MRU2 MUR2 MVR2 MWK2 MXN2 MXV2 MYR2 MZN2 \
    NAD2 NGN2 NIO2 NOK2 NPR2 NZD2 OMR3 PAB2 PEN2 PGK2 PHP2 PKR2 PLN2 PYG0 QAR2 RON2 RSD2 RUB2 RWF0 SAR2 \
    SBD2 SCR2 SDG2 SEK2 SGD2 SHP2 SLE2 SOS2 SRD2 SSP2 STN2 SVC2 SYP2 SZL2 THB2 TJS2 TMT2 TND3 TOP2 TRY2 \
    TTD2 TWD2 TZS2 UAH2 UGX0 USD2 USN2 UYI0 UYU2 UYW4 UZS2 VED2 VES2 VND0 VUV0 WST2 XAF0 XCD2 XCG2 XOF0 \
    XPF0 YER2 ZAR2 ZMW2 ZWG2";

/// Currency symbols most locales use, where they use one (others write the
/// code); `LOCALES` has each locale's own.
pub const SYMBOLS: &str = "USD=US$,EUR=\u{20ac},GBP=\u{a3},JPY=JP\u{a5},CNY=CN\u{a5},INR=\u{20b9},CAD=CA$,AUD=AU$,NZD=NZ$,BRL=R$,MXN=MX$,KRW=\u{20a9},TWD=NT$,HKD=HK$,VND=\u{20ab},ILS=\u{20aa}";

/// Each locale, from CLDR (as ICU 77 has it): decimal and group marks in
/// amounts of money, and in other numbers when they differ (de-AT groups
/// money with "." and numbers with a space), minus sign, flags (1: Indian grouping, 3 then 2s; 2: no group in a
/// four-digit number), the patterns for an amount and a negative one with
/// a symbol (`¤` the symbol, `#` the number) and with a code or a symbol
/// that is letters at the number's side, and the symbols that differ from
/// `SYMBOLS`.
pub const LOCALES: &str = "en-US\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tUSD=$,JPY=\u{a5},AUD=A$,PHP=\u{20b1}\n\
en-GB\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=A$,PHP=\u{20b1}\n\
en-CA\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tCAD=$,AUD=A$,PHP=\u{20b1}\n\
en-AU\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tUSD=USD,EUR=EUR,GBP=GBP,JPY=JPY,CNY=CNY,INR=INR,CAD=CAD,AUD=$,NZD=NZD,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n\
en-NZ\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=A$,NZD=$,PHP=\u{20b1}\n\
en-IE\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=A$,PHP=\u{20b1}\n\
en-IN\t.\t,\t\t\t-\t1\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tUSD=$,AUD=A$,PHP=\u{20b1}\n\
en-SG\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=A$,SGD=$,PHP=\u{20b1}\n\
en-ZA\t,\t\u{a0}\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=A$,PHP=\u{20b1},ZAR=R\n\
de-DE\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tUSD=$,JPY=\u{a5},THB=\u{e3f}\n\
de-AT\t,\t.\t\t\u{a0}\t-\t0\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tUSD=$,JPY=\u{a5},THB=\u{e3f}\n\
de-CH\t.\t\u{2019}\t\t\t-\t0\t\u{a4}\u{a0}#\t\u{a4}-#\t\u{a4}\u{a0}#\t\u{a4}-#\tUSD=$,EUR=EUR,JPY=\u{a5},THB=\u{e3f}\n\
fr-FR\t,\t\u{202f}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tUSD=$US,GBP=\u{a3}GB,JPY=JPY,CNY=CNY,CAD=$CA,AUD=$AU,NZD=$NZ,MXN=$MX,ARS=$AR,TWD=TWD,HKD=HKD,SGD=$SG\n\
fr-BE\t,\t\u{202f}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tUSD=$US,GBP=\u{a3}GB,JPY=JPY,CNY=CNY,CAD=$CA,AUD=$AU,NZD=$NZ,MXN=$MX,ARS=$AR,TWD=TWD,HKD=HKD,SGD=$SG\n\
fr-CA\t,\t\u{a0}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tUSD=$\u{a0}US,JPY=\u{a5},INR=INR,CAD=$,AUD=$\u{a0}AU,NZD=$\u{a0}NZ,MXN=MXN,KRW=KRW,TWD=TWD,HKD=$\u{a0}HK,SGD=$\u{a0}SG,VND=VND,ILS=ILS\n\
fr-CH\t.\t\u{202f}\t,\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tUSD=$US,GBP=\u{a3}GB,JPY=JPY,CNY=CNY,CAD=$CA,AUD=$AU,NZD=$NZ,MXN=$MX,ARS=$AR,TWD=TWD,HKD=HKD,SGD=$SG\n\
it-IT\t,\t.\t\t\t-\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tUSD=USD,JPY=JPY,INR=INR,AUD=A$,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,THB=\u{e3f},VND=VND,PHP=\u{20b1}\n\
it-CH\t.\t\u{2019}\t\t\t-\t2\t\u{a4}\u{a0}#\t\u{a4}-#\t\u{a4}\u{a0}#\t\u{a4}-#\tUSD=USD,JPY=JPY,INR=INR,AUD=A$,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,THB=\u{e3f},VND=VND,PHP=\u{20b1}\n\
es-ES\t,\t.\t\t\t-\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tGBP=GBP,JPY=JPY,CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,THB=\u{e3f},ILS=ILS\n\
es-MX\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tUSD=USD,EUR=EUR,GBP=GBP,JPY=JPY,CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,BRL=BRL,MXN=$,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n\
es-AR\t,\t.\t\t\t-\t0\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tEUR=EUR,GBP=GBP,JPY=JPY,CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,BRL=BRL,MXN=MXN,ARS=$,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n\
pt-BR\t,\t.\t\t\t-\t0\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tTHB=\u{e3f}\n\
pt-PT\t,\t\u{a0}\t\t\t-\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tTHB=\u{e3f}\n\
nl-NL\t,\t.\t\t\t-\t0\t\u{a4}\u{a0}#\t\u{a4}\u{a0}-#\t\u{a4}\u{a0}#\t\u{a4}\u{a0}-#\tCAD=C$,THB=\u{e3f}\n\
nl-BE\t,\t.\t\t\t-\t0\t\u{a4}\u{a0}#\t\u{a4}\u{a0}-#\t\u{a4}\u{a0}#\t\u{a4}\u{a0}-#\tCAD=C$,THB=\u{e3f}\n\
sv-SE\t,\t\u{a0}\t\t\t\u{2212}\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tGBP=GBP,JPY=JPY,CNY=CNY,INR=INR,AUD=AUD,NZD=NZD,SEK=kr,NOK=Nkr,DKK=Dkr,BRL=BR$,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,EGP=EG\u{a3}\n\
nb-NO\t,\t\u{a0}\t\t\t\u{2212}\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tUSD=USD,JPY=JPY,CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,NOK=kr,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n\
da-DK\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tDKK=kr.,THB=\u{e3f}\n\
fi-FI\t,\t\u{a0}\t\t\t\u{2212}\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tUSD=$,JPY=\u{a5},CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n\
pl-PL\t,\t\u{a0}\t\t\t-\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tUSD=USD,GBP=GBP,JPY=JPY,CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,PLN=z\u{142},MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n\
cs-CZ\t,\t\u{a0}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tINR=INR,CZK=K\u{10d},VND=VND,ILS=ILS\n\
hu-HU\t,\t\u{a0}\t\t\t-\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tUSD=USD,EUR=EUR,GBP=GBP,JPY=\u{a5},CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,HUF=Ft,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n\
ro-RO\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tUSD=USD,EUR=EUR,GBP=GBP,JPY=JPY,CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n\
el-GR\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tUSD=$,AUD=A$,THB=\u{e3f}\n\
tr-TR\t,\t.\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tUSD=$,JPY=\u{a5},TRY=\u{20ba},THB=\u{e3f}\n\
ru-RU\t,\t\u{a0}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tUSD=$,JPY=\u{a5},AUD=A$,RUB=\u{20bd},UAH=\u{20b4},THB=\u{e3f}\n\
uk-UA\t,\t\u{a0}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tUSD=USD,EUR=EUR,GBP=GBP,JPY=\u{a5},CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,UAH=\u{20b4},BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n\
ja-JP\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tUSD=$,JPY=\u{ffe5},CNY=\u{5143},AUD=A$\n\
zh-CN\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tCNY=\u{a5}\n\
zh-TW\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tJPY=\u{a5},KRW=\u{ffe6},TWD=$\n\
zh-HK\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tJPY=\u{a5}\n\
ko-KR\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\t\n\
hi-IN\t.\t,\t\t\t-\t1\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tUSD=$,AUD=A$,THB=\u{e3f}\n\
id-ID\t,\t.\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tINR=Rs,THB=\u{e3f},IDR=Rp\n\
th-TH\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tJPY=\u{a5},THB=\u{e3f}\n\
vi-VN\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tJPY=\u{a5},THB=\u{e3f}\n\
ms-MY\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tUSD=USD,CAD=CAD,AUD=A$,MXN=MXN,MYR=RM\n\
fil-PH\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tUSD=$,JPY=\u{a5},AUD=A$,THB=\u{e3f},PHP=\u{20b1}";

pub const BUILTINS: &[(&str, BuiltinFn)] = &[
    ("money.of", of),
    ("money.from_minor", from_minor),
    ("money.to_minor", to_minor),
    ("money.add", add),
    ("money.sub", sub),
    ("money.mul", mul),
    ("money.neg", neg),
    ("money.compare", compare),
    ("money.zero", zero),
    ("money.is_zero", is_zero),
    ("money.allocate", allocate),
    ("money.round", round),
    ("money.minor_units", minor_units),
    ("money.format", format),
];

pub fn install(m: &mut Map) {
    for (name, f) in BUILTINS {
        m.insert(name.trim_start_matches("money.").into(), Value::Builtin(name, *f));
    }
}

// ---- tables --------------------------------------------------------------

/// A currency's minor units, by its code (upper case).
pub fn units_of(code: &str) -> Option<u32> {
    static T: OnceLock<HashMap<&'static str, u32>> = OnceLock::new();
    T.get_or_init(|| CURRENCIES.split_whitespace().map(|c| (&c[..3], c[3..].parse().unwrap())).collect()).get(code).copied()
}

struct Locale {
    name: &'static str,
    decimal: &'static str,
    group: &'static str,
    /// A number's marks, when not money's.
    number: (&'static str, &'static str),
    minus: &'static str,
    indian: bool,
    min2: bool,
    /// With a symbol: amount, negative; with letters next to the number: the same.
    pats: [&'static str; 4],
    symbols: HashMap<&'static str, &'static str>,
}

fn symbol_pairs(s: &'static str) -> HashMap<&'static str, &'static str> {
    s.split(',').filter_map(|p| p.split_once('=')).collect()
}

fn locales() -> &'static [Locale] {
    static T: OnceLock<Vec<Locale>> = OnceLock::new();
    T.get_or_init(|| {
        LOCALES
            .lines()
            .map(|l| {
                let f: Vec<&'static str> = l.split('\t').collect();
                let flags: u32 = f[6].parse().unwrap();
                let or = |x: &'static str, y: &'static str| if x.is_empty() { y } else { x };
                Locale {
                    name: f[0],
                    decimal: f[1],
                    group: f[2],
                    number: (or(f[3], f[1]), or(f[4], f[2])),
                    minus: f[5],
                    indian: flags & 1 != 0,
                    min2: flags & 2 != 0,
                    pats: [f[7], f[8], f[9], f[10]],
                    symbols: symbol_pairs(f.get(11).copied().unwrap_or("")),
                }
            })
            .collect()
    })
}

/// A locale's data: by its name (`en-US`, `en_us`), else the first of its
/// language (`de-LU`, `de`: de-DE), else en-US.
fn locale_data(name: &str) -> &'static Locale {
    let all = locales();
    let name = name.replace('_', "-");
    let lang = name.split('-').next().unwrap_or("");
    all.iter()
        .find(|l| l.name.eq_ignore_ascii_case(&name))
        .or_else(|| all.iter().find(|l| l.name.split('-').next().map_or(false, |x| x.eq_ignore_ascii_case(lang))))
        .unwrap_or(&all[0])
}

fn symbol(loc: &Locale, code: &str) -> String {
    static BASE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    loc.symbols
        .get(code)
        .or_else(|| BASE.get_or_init(|| symbol_pairs(SYMBOLS)).get(code))
        .map_or_else(|| code.to_string(), |s| s.to_string())
}

/// The digits of a decimal's magnitude, grouped, with these marks.
fn digits(loc: &Locale, (decimal, group): (&str, &str), d: Dec) -> String {
    let text = d.abs().to_string();
    let (whole, frac) = text.split_once('.').map_or((text.as_str(), ""), |(w, f)| (w, f));
    let mut out = String::new();
    let n = whole.len();
    let grouped = !(loc.min2 && n < 5);
    for (i, c) in whole.chars().enumerate() {
        let left = n - i;
        if i > 0 && grouped {
            let cut = if loc.indian { left == 3 || (left > 3 && (left - 3) % 2 == 0) } else { left % 3 == 0 };
            if cut {
                out.push_str(group);
            }
        }
        out.push(c);
    }
    if !frac.is_empty() {
        out.push_str(decimal);
        out.push_str(frac);
    }
    out
}

/// `d` written as `loc` writes an amount of `code`, with `places` decimals.
pub fn format_amount(d: Dec, code: &str, places: u32, loc_name: &str) -> String {
    let loc = locale_data(loc_name);
    let d = d.round(places as i32, Rounding::HalfEven).unwrap_or(d);
    let sym = symbol(loc, code);
    let neg = d.is_negative();
    // Before the number, the symbol's last character meets it; after, its first.
    let before = loc.pats[0].find('\u{a4}') < loc.pats[0].find('#');
    let side = if before { sym.chars().last() } else { sym.chars().next() };
    let letters = side.map_or(false, |c| c.is_alphabetic());
    let pat = loc.pats[if letters { 2 } else { 0 } + neg as usize];
    pat.replace('-', loc.minus).replace('\u{a4}', &sym).replace('#', &digits(loc, (loc.decimal, loc.group), d))
}

/// `format_number(x, locale, digits)`: a number as `locale` writes one.
pub fn format_number(d: Dec, places: Option<u32>, loc_name: &str) -> String {
    let loc = locale_data(loc_name);
    let d = match places {
        Some(p) => d.round(p as i32, Rounding::HalfEven).unwrap_or(d),
        None => d,
    };
    let sign = if d.is_negative() { loc.minus } else { "" };
    format!("{}{}", sign, digits(loc, loc.number, d))
}

// ---- values --------------------------------------------------------------

/// A money value: its amount and currency, and the currency's minor units.
#[derive(Clone)]
struct Money {
    amount: Dec,
    code: String,
    units: u32,
}

impl Money {
    fn value(&self) -> Value {
        let mut m = Map::new();
        m.insert("amount".into(), Value::Dec(self.amount));
        m.insert("currency".into(), Value::str(&self.code));
        Value::map(m)
    }
}

fn tn(v: Option<&Value>) -> String {
    v.map_or("nothing".into(), |v| format!("{} {}", v.type_name(), crate::interp::short(v)))
}

/// A currency code given as argument `i`: Ok(Err(..)) is an error value
/// for an unknown one (it may come from data); a non-string is a mistake.
fn currency(args: &[Value], i: usize, f: &str, line: u32) -> Result<Result<(String, u32), Value>, Ctrl> {
    match args.get(i) {
        Some(Value::Str(s)) => {
            let code = s.trim().to_ascii_uppercase();
            Ok(units_of(&code).map(|u| (code, u)).ok_or_else(|| Value::err(Value::str(format!("unknown currency {:?}", &**s)))))
        }
        other => fail(line, format!("`{}` expects a currency code (a str) as argument {}, got {}", f, i + 1, tn(other))),
    }
}

/// Argument `i` as money: a map with a known `currency` and an `amount`
/// (a decimal, an int, or numeric text, as JSON brings it).
fn money(args: &[Value], i: usize, f: &str, line: u32) -> Result<Money, Ctrl> {
    let bad = |why: &str| fail(line, format!("`{}` expects money ({{ amount, currency }}) as argument {}, got {}{}", f, i + 1, tn(args.get(i)), why));
    let Some(Value::Map(m)) = args.get(i) else { return bad("") };
    let code = match m.get("currency") {
        Some(Value::Str(s)) => s.to_ascii_uppercase(),
        _ => return bad(": no currency"),
    };
    let Some(units) = units_of(&code) else { return bad(": unknown currency") };
    let amount = match m.get("amount") {
        Some(v @ (Value::Dec(_) | Value::Int(_) | Value::Str(_))) => crate::interp::as_dec(v),
        _ => None,
    };
    let Some(amount) = amount else { return bad(": its amount must be a dec") };
    Ok(Money { amount, code, units })
}

/// A rounding mode given as argument `i` (half_even when left out).
fn mode(args: &[Value], i: usize, f: &str, line: u32) -> Result<Rounding, Ctrl> {
    match args.get(i) {
        None | Some(Value::Nil) => Ok(Rounding::HalfEven),
        Some(Value::Str(s)) => match Rounding::parse(s) {
            Some(m) => Ok(m),
            None => fail(line, format!("`{}`: unknown rounding {:?} (one of {})", f, &**s, Rounding::NAMES)),
        },
        other => fail(line, format!("`{}` expects a rounding mode (a str) as argument {}, got {}", f, i + 1, tn(other))),
    }
}

fn dec_err(line: u32) -> impl Fn(crate::decimal::DecError) -> Ctrl {
    move |e| Ctrl::Error(crate::QuilloError::new(e.message(), line))
}

/// `amount` with exactly the currency's decimals.
fn to_units(amount: Dec, units: u32, mode: Rounding, line: u32) -> Result<Dec, Ctrl> {
    amount.round(units as i32, mode).map_err(dec_err(line))
}

fn same_currency(a: &Money, b: &Money, verb: &str, line: u32) -> Result<(), Ctrl> {
    if a.code != b.code {
        return fail(line, format!("cannot {} {} and {}: convert one currency to the other first", verb, a.code, b.code));
    }
    Ok(())
}

// ---- builtins ------------------------------------------------------------

/// `money.of(amount, currency, mode)`: an amount (a decimal, an int or
/// numeric text) of a currency. More decimals than the currency has are
/// an error value, unless a rounding mode says how to drop them.
fn of(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (code, units) = match currency(&args, 1, "money.of", line)? {
        Ok(c) => c,
        Err(e) => return Ok(e),
    };
    let amount = match args.first() {
        Some(Value::Dec(d)) => *d,
        Some(Value::Int(n)) => Dec::from_i64(*n),
        Some(Value::Str(s)) => match Dec::parse(s.trim()) {
            Some(d) => d,
            None => return Ok(Value::err(Value::str(format!("not an amount: {:?}", &**s)))),
        },
        Some(Value::Float(_)) => return fail(line, "`money.of` takes a dec, an int or text, not a float: convert it with dec(x)"),
        other => return fail(line, format!("`money.of` expects an amount (a dec, int or str) as argument 1, got {}", tn(other))),
    };
    let rounded = match args.get(2) {
        None | Some(Value::Nil) => {
            let exact = to_units(amount, units, Rounding::Down, line)?;
            if exact != amount {
                return Ok(Value::err(Value::str(format!(
                    "{} has more decimals than {} has ({}); give a rounding mode to round it",
                    amount, code, units
                ))));
            }
            exact
        }
        _ => to_units(amount, units, mode(&args, 2, "money.of", line)?, line)?,
    };
    Ok(Money { amount: rounded, code, units }.value())
}

/// `money.from_minor(1250, "USD")`: 12.50 dollars, from cents.
fn from_minor(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (code, units) = match currency(&args, 1, "money.from_minor", line)? {
        Ok(c) => c,
        Err(e) => return Ok(e),
    };
    let n = match args.first() {
        Some(Value::Int(n)) => *n,
        other => return fail(line, format!("`money.from_minor` expects an int as argument 1, got {}", tn(other))),
    };
    let amount = Dec::new(n as i128, units).expect("an int's digits fit a decimal");
    Ok(Money { amount, code, units }.value())
}

/// `money.to_minor(m)`: the amount in minor units (cents), an int.
fn to_minor(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let m = money(&args, 0, "money.to_minor", line)?;
    let d = to_units(m.amount, m.units, Rounding::HalfEven, line)?;
    match i64::try_from(d.mantissa()) {
        Ok(n) if d.scale() == m.units => Ok(Value::Int(n)),
        _ => fail(line, "integer overflow"),
    }
}

fn add(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (a, b) = (money(&args, 0, "money.add", line)?, money(&args, 1, "money.add", line)?);
    same_currency(&a, &b, "add", line)?;
    let sum = a.amount.add(b.amount).map_err(dec_err(line))?;
    Ok(Money { amount: to_units(sum, a.units, Rounding::HalfEven, line)?, ..a }.value())
}

fn sub(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (a, b) = (money(&args, 0, "money.sub", line)?, money(&args, 1, "money.sub", line)?);
    same_currency(&a, &b, "subtract", line)?;
    let diff = a.amount.sub(b.amount).map_err(dec_err(line))?;
    Ok(Money { amount: to_units(diff, a.units, Rounding::HalfEven, line)?, ..a }.value())
}

/// `money.mul(m, factor, mode)`: times a decimal or an int, rounded to the
/// currency (half to even unless told otherwise).
fn mul(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let m = money(&args, 0, "money.mul", line)?;
    let factor = match args.get(1) {
        Some(Value::Dec(d)) => *d,
        Some(Value::Int(n)) => Dec::from_i64(*n),
        Some(Value::Float(_)) => return fail(line, "`money.mul` takes a dec or an int, not a float: convert it with dec(x)"),
        other => return fail(line, format!("`money.mul` expects a dec or an int as argument 2, got {}", tn(other))),
    };
    let mode = mode(&args, 2, "money.mul", line)?;
    let product = m.amount.mul(factor).map_err(dec_err(line))?;
    Ok(Money { amount: to_units(product, m.units, mode, line)?, ..m }.value())
}

fn neg(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let m = money(&args, 0, "money.neg", line)?;
    Ok(Money { amount: m.amount.neg(), ..m }.value())
}

/// `money.compare(a, b)`: -1, 0 or 1, for amounts of one currency.
fn compare(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (a, b) = (money(&args, 0, "money.compare", line)?, money(&args, 1, "money.compare", line)?);
    same_currency(&a, &b, "compare", line)?;
    Ok(Value::Int(a.amount.cmp(b.amount) as i64))
}

fn zero(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (code, units) = match currency(&args, 0, "money.zero", line)? {
        Ok(c) => c,
        Err(e) => return Ok(e),
    };
    Ok(Money { amount: Dec::new(0, units).unwrap(), code, units }.value())
}

fn is_zero(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Bool(money(&args, 0, "money.is_zero", line)?.amount.is_zero()))
}

/// `money.round(m, mode)`: the amount rounded to the currency's decimals.
fn round(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let m = money(&args, 0, "money.round", line)?;
    let mode = mode(&args, 1, "money.round", line)?;
    Ok(Money { amount: to_units(m.amount, m.units, mode, line)?, ..m }.value())
}

fn minor_units(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(match currency(&args, 0, "money.minor_units", line)? {
        Ok((_, units)) => Value::Int(units as i64),
        Err(e) => e,
    })
}

/// `money.allocate(m, ratios)`: the amount split by ratios (ints or
/// decimals; a number n for n equal parts) without losing or making a
/// minor unit. Each share gets its ratio's part, rounded down; the units
/// left over go one each to the first shares with a ratio (Fowler's
/// allocation), so the shares add up to the amount.
fn allocate(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    const F: &str = "money.allocate";
    let m = money(&args, 0, F, line)?;
    let ratios: Vec<Dec> = match args.get(1) {
        Some(Value::Int(n)) if *n >= 1 && *n <= 100_000 => vec![Dec::from_i64(1); *n as usize],
        Some(Value::List(l)) if !l.is_empty() => {
            let mut out = Vec::with_capacity(l.len());
            for (k, v) in l.iter().enumerate() {
                match v {
                    Value::Int(n) if *n >= 0 => out.push(Dec::from_i64(*n)),
                    Value::Dec(d) if !d.is_negative() => out.push(*d),
                    other => return fail(line, format!("`{}`: ratio {} must be an int or a dec, 0 or more, got {}", F, k, tn(Some(other)))),
                }
            }
            out
        }
        other => return fail(line, format!("`{}` expects a list of ratios or a number of parts as argument 2, got {}", F, tn(other))),
    };
    // The ratios as whole numbers of one scale.
    let scale = ratios.iter().map(|r| r.scale()).max().unwrap_or(0);
    let whole: Option<Vec<i128>> = ratios.iter().map(|r| r.mantissa().checked_mul(10i128.pow(scale - r.scale()))).collect();
    let Some(whole) = whole else { return fail(line, format!("`{}`: the ratios are too large", F)) };
    let total_ratio = whole.iter().try_fold(0i128, |a, r| a.checked_add(*r));
    let Some(total_ratio) = total_ratio.filter(|t| *t > 0) else {
        return fail(line, format!("`{}`: the ratios must add up to more than 0", F));
    };
    let minor = to_units(m.amount, m.units, Rounding::HalfEven, line)?.mantissa();
    let (neg, units) = (minor < 0, minor.unsigned_abs() as i128);
    let mut shares = Vec::with_capacity(whole.len());
    for r in &whole {
        match units.checked_mul(*r) {
            Some(p) => shares.push(p / total_ratio),
            None => return fail(line, format!("`{}`: the ratios are too large", F)),
        }
    }
    let mut left = units - shares.iter().sum::<i128>();
    for (s, r) in shares.iter_mut().zip(&whole) {
        if left == 0 {
            break;
        }
        if *r > 0 {
            *s += 1;
            left -= 1;
        }
    }
    let out = shares
        .into_iter()
        .map(|s| Money { amount: Dec::new(if neg { -s } else { s }, m.units).unwrap(), code: m.code.clone(), units: m.units }.value())
        .collect();
    Ok(Value::list(out))
}

/// The locale to write in: the one given, else the program's (`locale()`,
/// with `translations`), else en-US.
fn locale_arg(it: &mut Interp, args: &[Value], i: usize, f: &str, line: u32) -> Result<String, Ctrl> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s.to_string()),
        None | Some(Value::Nil) => {
            it.note_locale();
            Ok(it.locale().map_or_else(|| "en-US".to_string(), |l| l.to_string()))
        }
        other => fail(line, format!("`{}` expects a locale (a str) as argument {}, got {}", f, i + 1, tn(other))),
    }
}

/// `money.format(m, locale)`: "$1,234.50" (en-US), "1.234,50 €" (de-DE).
fn format(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let m = money(&args, 0, "money.format", line)?;
    let loc = locale_arg(it, &args, 1, "money.format", line)?;
    Ok(Value::str(format_amount(m.amount, &m.code, m.units, &loc)))
}

/// `format_number(x, locale, digits)`: "1,234.5" (en-US), "1.234,5"
/// (de-DE); with `digits`, rounded half to even to that many decimals.
pub fn format_number_fn(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    const F: &str = "format_number";
    let d = match args.first() {
        Some(Value::Dec(d)) => *d,
        Some(Value::Int(n)) => Dec::from_i64(*n),
        Some(Value::Float(f)) => match Dec::from_f64(*f) {
            Some(d) => d,
            None => return Ok(Value::str(Value::Float(*f).display())),
        },
        other => return fail(line, format!("`{}` expects a number as argument 1, got {}", F, tn(other))),
    };
    let loc = locale_arg(it, &args, 1, F, line)?;
    let places = match args.get(2) {
        None | Some(Value::Nil) => None,
        Some(Value::Int(n)) => Some((*n).clamp(0, 28) as u32),
        other => return fail(line, format!("`{}` expects an int as argument 3, got {}", F, tn(other))),
    };
    Ok(Value::str(format_number(d, places, &loc)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_read_whole() {
        assert_eq!(CURRENCIES.split_whitespace().count(), 165);
        assert_eq!((units_of("JPY"), units_of("USD"), units_of("BHD"), units_of("CLF"), units_of("XAU")), (Some(0), Some(2), Some(3), Some(4), None));
        assert!(locales().iter().all(|l| l.pats.iter().all(|p| p.contains('#') && p.contains('\u{a4}'))));
        assert_eq!(locale_data("de_lu").name, "de-DE");
        assert_eq!(locale_data("xx").name, "en-US");
    }

    #[test]
    fn the_browser_has_the_same_tables() {
        let js = include_str!("client/runtime.js");
        // The JS writes non-ASCII as \uXXXX; compare what the escapes stand for.
        let unescape = |s: &str| {
            let mut out = String::new();
            let mut it = s.chars().peekable();
            while let Some(c) = it.next() {
                if c == '\\' && it.peek() == Some(&'u') {
                    it.next();
                    let h: String = it.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&h, 16).unwrap()).unwrap());
                } else if c == '\\' && it.peek() == Some(&'t') {
                    it.next();
                    out.push('\t');
                } else if c == '\\' && it.peek() == Some(&'n') {
                    it.next();
                    out.push('\n');
                } else {
                    out.push(c);
                }
            }
            out
        };
        let constant = |name: &str| {
            let start = js.find(&format!("const {} = ", name)).unwrap();
            let end = start + js[start..].find(";\n").unwrap();
            let parts: Vec<&str> = js[start..end].split('"').skip(1).step_by(2).collect();
            unescape(&parts.concat())
        };
        assert_eq!(constant("MONEY_CURRENCIES"), CURRENCIES);
        assert_eq!(constant("MONEY_SYMBOLS"), SYMBOLS);
        assert_eq!(constant("MONEY_LOCALES"), LOCALES);
    }
}
