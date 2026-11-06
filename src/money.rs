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

/// The active ISO 4217 currencies and their minor units, as of 2026. Codes
/// without minor units, such as XAU and XDR, are not money here; withdrawn
/// ones (ANG, BGN, HRK, ...) are in `WITHDRAWN`. runtime.js has the same table.
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
/// code); `LOCALES` has each locale's own. With `LOCALES`, written from ICU 77's
/// CLDR data for every currency: XOF, XAF, XPF, XCD and XCG had none, and the
/// French locales none for $CL and a dozen others (FEEDBACK50.md, Q-627; branch
/// feedback50-fixes).
pub const SYMBOLS: &str = "AUD=A$,BRL=R$,CAD=CA$,CNY=CN\u{a5},EUR=\u{20ac},GBP=\u{a3},HKD=HK$,ILS=\u{20aa},INR=\u{20b9},JPY=JP\u{a5},KRW=\u{20a9},MXN=MX$,NZD=NZ$,TWD=NT$,USD=US$,VND=\u{20ab},XAF=FCFA,XCD=EC$,XCG=Cg.,XOF=F\u{202f}CFA,XPF=CFPF";

/// Each locale, from CLDR (as ICU 77 has it): decimal and group marks in
/// amounts of money, and in other numbers when they differ (de-AT groups
/// money with "." and numbers with a space), minus sign, flags (1: Indian grouping, 3 then 2s; 2: no group in a
/// four-digit number; 4: Indian grouping in other numbers), the patterns for an amount and a negative one with
/// a symbol (`¤` the symbol, `#` the number) and with a code or a symbol
/// whose side at the number is not a currency sign, and the symbols that differ from
/// `SYMBOLS`. 122 locales, written from Intl.NumberFormat's formatToParts under ICU 77 by
/// scripts/money-tables.mjs: the locales whose digits are not 0-9, and those
/// whose money the four patterns cannot write (right-to-left marks around symbols, en-150's
/// pattern for each currency), are left out and written as another, with a warning
/// (FEEDBACK50.md, Q-627; branch feedback50-fixes).
pub const LOCALES: &str = "en-US\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tJPY=\u{a5},PHP=\u{20b1},USD=$\n\
en-GB\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tPHP=\u{20b1}\n\
en-CA\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tCAD=$,PHP=\u{20b1}\n\
en-AU\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=$,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,SCR=Rs,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF,XPF=CFP\n\
en-NZ\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tNZD=$,PHP=\u{20b1}\n\
en-IE\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tPHP=\u{20b1}\n\
en-IN\t.\t,\t\t\t-\t5\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tPHP=\u{20b1},USD=$\n\
en-SG\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tPHP=\u{20b1},SGD=$\n\
en-ZA\t,\t\u{a0}\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tPHP=\u{20b1},ZAR=R\n\
de-DE\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AU$,JPY=\u{a5},THB=\u{e3f},USD=$\n\
de-AT\t,\t.\t\t\u{a0}\t-\t0\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AU$,JPY=\u{a5},THB=\u{e3f},USD=$\n\
de-CH\t.\t\u{2019}\t\t\t-\t0\t\u{a4}\u{a0}#\t\u{a4}-#\t\u{a4}\u{a0}#\t\u{a4}-#\tAUD=AU$,EUR=EUR,JPY=\u{a5},THB=\u{e3f},USD=$\n\
fr-FR\t,\t\u{202f}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tARS=$AR,AUD=$AU,BMD=$BM,BND=$BN,BZD=$BZ,CAD=$CA,CLP=$CL,CNY=CNY,COP=$CO,FJD=$FJ,FKP=\u{a3}FK,GBP=\u{a3}GB,GIP=\u{a3}GI,HKD=HKD,JPY=JPY,LBP=\u{a3}LB,MXN=$MX,NAD=$NA,NZD=$NZ,SBD=$SB,SGD=$SG,SRD=$SR,TTD=$TT,TWD=TWD,USD=$US,UYU=$UY,WST=$WS,XCD=XCD,XPF=FCFP,CYP=\u{a3}CY\n\
fr-BE\t,\t\u{202f}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tARS=$AR,AUD=$AU,BMD=$BM,BND=$BN,BZD=$BZ,CAD=$CA,CLP=$CL,CNY=CNY,COP=$CO,FJD=$FJ,FKP=\u{a3}FK,GBP=\u{a3}GB,GIP=\u{a3}GI,HKD=HKD,JPY=JPY,LBP=\u{a3}LB,MXN=$MX,NAD=$NA,NZD=$NZ,SBD=$SB,SGD=$SG,SRD=$SR,TTD=$TT,TWD=TWD,USD=$US,UYU=$UY,WST=$WS,XCD=XCD,XPF=FCFP,CYP=\u{a3}CY\n\
fr-CA\t,\t\u{a0}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=$\u{a0}AU,CAD=$,HKD=$\u{a0}HK,ILS=ILS,INR=INR,JPY=\u{a5},KRW=KRW,MXN=MXN,NZD=$\u{a0}NZ,SGD=$\u{a0}SG,TWD=TWD,USD=$\u{a0}US,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF,XPF=XPF,CYP=\u{a3}CY\n\
fr-CH\t.\t\u{202f}\t,\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tARS=$AR,AUD=$AU,BMD=$BM,BND=$BN,BZD=$BZ,CAD=$CA,CLP=$CL,CNY=CNY,COP=$CO,FJD=$FJ,FKP=\u{a3}FK,GBP=\u{a3}GB,GIP=\u{a3}GI,HKD=HKD,JPY=JPY,LBP=\u{a3}LB,MXN=$MX,NAD=$NA,NZD=$NZ,SBD=$SB,SGD=$SG,SRD=$SR,TTD=$TT,TWD=TWD,USD=$US,UYU=$UY,WST=$WS,XCD=XCD,XPF=FCFP,CYP=\u{a3}CY\n\
it-IT\t,\t.\t\t\t-\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tBRL=BRL,HKD=HKD,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,PHP=\u{20b1},THB=\u{e3f},TWD=TWD,USD=USD,VND=VND\n\
it-CH\t.\t\u{2019}\t\t\t-\t2\t\u{a4}\u{a0}#\t\u{a4}-#\t\u{a4}\u{a0}#\t\u{a4}-#\tBRL=BRL,HKD=HKD,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,PHP=\u{20b1},THB=\u{e3f},TWD=TWD,USD=USD,VND=VND\n\
es-ES\t,\t.\t\t\t-\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,THB=\u{e3f},TWD=TWD,XAF=XAF,XCD=XCD,XOF=XOF\n\
es-MX\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MRU=UM,MXN=$,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF,MRO=MRU\n\
es-AR\t,\t.\t\t\t-\t0\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tARS=$,AUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n\
pt-BR\t,\t.\t\t\t-\t0\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AU$,THB=\u{e3f}\n\
pt-PT\t,\t\u{a0}\t\t\t-\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AU$,THB=\u{e3f}\n\
nl-NL\t,\t.\t\t\t-\t0\t\u{a4}\u{a0}#\t\u{a4}\u{a0}-#\t\u{a4}\u{a0}#\t\u{a4}\u{a0}-#\tAUD=AU$,CAD=C$,FJD=FJ$,SBD=SI$,THB=\u{e3f},XPF=XPF\n\
nl-BE\t,\t.\t\t\t-\t0\t\u{a4}\u{a0}#\t\u{a4}\u{a0}-#\t\u{a4}\u{a0}#\t\u{a4}\u{a0}-#\tAUD=AU$,CAD=C$,FJD=FJ$,SBD=SI$,THB=\u{e3f},XPF=XPF\n\
sv-SE\t,\t\u{a0}\t\t\t\u{2212}\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,BBD=Bds$,BMD=BM$,BRL=BR$,BSD=BS$,BZD=BZ$,CNY=CNY,DKK=Dkr,DOP=RD$,EGP=EG\u{a3},GBP=GBP,HKD=HKD,INR=INR,ISK=Ikr,JMD=JM$,JPY=JPY,KRW=KRW,NOK=Nkr,NZD=NZD,SEK=kr,TWD=TWD,VND=VND,EEK=Ekr\n\
nb-NO\t,\t\u{a0}\t\t\t\u{2212}\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NOK=kr,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XPF=XPF\n\
da-DK\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AU$,DKK=kr.,THB=\u{e3f}\n\
fi-FI\t,\t\u{a0}\t\t\t\u{2212}\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,HKD=HKD,ILS=ILS,INR=INR,JPY=\u{a5},KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=$,VND=VND,XCD=XCD,XPF=XPF\n\
pl-PL\t,\t\u{a0}\t\t\t-\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,CAD=CAD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,PLN=z\u{142},TWD=TWD,USD=USD,VND=VND\n\
cs-CZ\t,\t\u{a0}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AU$,CZK=K\u{10d},ILS=ILS,INR=INR,VND=VND\n\
hu-HU\t,\t\u{a0}\t\t\t-\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,HUF=Ft,ILS=ILS,INR=INR,JPY=\u{a5},KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XCD=XCD\n\
ro-RO\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XCD=XCD\n\
el-GR\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tTHB=\u{e3f},USD=$\n\
tr-TR\t,\t.\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AU$,JPY=\u{a5},THB=\u{e3f},TRY=\u{20ba},USD=$\n\
ru-RU\t,\t\u{a0}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tJPY=\u{a5},RUB=\u{20bd},THB=\u{e3f},TMT=\u{422}\u{41c}\u{422},UAH=\u{20b4},USD=$\n\
uk-UA\t,\t\u{a0}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=\u{a5},KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,UAH=\u{20b4},USD=USD,VND=VND,XCD=XCD\n\
ja-JP\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tCNY=\u{5143},JPY=\u{ffe5},USD=$\n\
zh-CN\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AU$,CNY=\u{a5}\n\
zh-TW\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AU$,JPY=\u{a5},KRW=\u{ffe6},TWD=$\n\
zh-HK\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AU$,JPY=\u{a5}\n\
ko-KR\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AU$\n\
hi-IN\t.\t,\t\t\t-\t5\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tTHB=\u{e3f},USD=$\n\
id-ID\t,\t.\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AU$,IDR=Rp,INR=Rs,THB=\u{e3f}\n\
th-TH\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AU$,JPY=\u{a5},THB=\u{e3f}\n\
vi-VN\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AU$,JPY=\u{a5},THB=\u{e3f}\n\
ms-MY\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tCAD=CAD,MXN=MXN,MYR=RM,USD=USD\n\
fil-PH\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tJPY=\u{a5},PHP=\u{20b1},THB=\u{e3f},USD=$\n\
es-419\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n\
es-CO\t,\t.\t\t\t-\t0\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,COP=$,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n\
es-CL\t,\t.\t\t\t-\t0\t\u{a4}#\t\u{a4}-#\t\u{a4}\u{a0}#\t\u{a4}-#\tAUD=AUD,BRL=BRL,CAD=CAD,CLP=$,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n\
es-PE\t.\t,\t\t\t-\t0\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,PEN=S/,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n\
es-US\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=\u{a5},KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=$,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n\
es-UY\t,\t.\t\t\t-\t0\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,UYU=$,UYW=UP,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n\
es-VE\t,\t.\t\t\t-\t0\t\u{a4}#\t\u{a4}-#\t\u{a4}\u{a0}#\t\u{a4}-#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VES=Bs.S,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF,VEF=Bs.\n\
es-EC\t,\t.\t\t\t-\t0\t\u{a4}#\t\u{a4}-#\t\u{a4}\u{a0}#\t\u{a4}-#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=$,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n\
es-GT\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,GTQ=Q,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n\
es-CR\t,\t\u{a0}\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,CRC=\u{20a1},EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n\
es-DO\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,DOP=RD$,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n\
es-BO\t,\t.\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AUD,BOB=Bs,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n\
es-PY\t,\t.\t\t\t-\t0\t\u{a4}\u{a0}#\t\u{a4}\u{a0}-#\t\u{a4}\u{a0}#\t\u{a4}\u{a0}-#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,PYG=Gs.,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n\
es-PA\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,PAB=B/.,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n\
es-PR\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=$,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n\
pt-AO\t,\t\u{a0}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAOA=Kz,AUD=AU$,THB=\u{e3f}\n\
pt-MZ\t,\t\u{a0}\t\t\t-\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AU$,MZN=MTn,THB=\u{e3f}\n\
en-NG\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tNGN=\u{20a6},PHP=\u{20b1}\n\
en-KE\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tKES=Ksh,PHP=\u{20b1}\n\
en-PK\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tPHP=\u{20b1},PKR=Rs\n\
en-PH\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tJPY=\u{a5},PHP=\u{20b1},USD=$\n\
en-MY\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tMYR=RM,PHP=\u{20b1}\n\
en-AE\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tJPY=\u{a5},PHP=\u{20b1},USD=$\n\
en-HK\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tPHP=\u{20b1}\n\
en-GH\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tGHS=GH\u{20b5},PHP=\u{20b1}\n\
en-IL\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tPHP=\u{20b1}\n\
en-001\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tPHP=\u{20b1}\n\
fr-MA\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tARS=$AR,AUD=$AU,BMD=$BM,BND=$BN,BZD=$BZ,CAD=$CA,CLP=$CL,CNY=CNY,COP=$CO,FJD=$FJ,FKP=\u{a3}FK,GBP=\u{a3}GB,GIP=\u{a3}GI,HKD=HKD,JPY=JPY,LBP=\u{a3}LB,MXN=$MX,NAD=$NA,NZD=$NZ,SBD=$SB,SGD=$SG,SRD=$SR,TTD=$TT,TWD=TWD,USD=$US,UYU=$UY,WST=$WS,XCD=XCD,XPF=FCFP,CYP=\u{a3}CY\n\
fr-SN\t,\t\u{202f}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tARS=$AR,AUD=$AU,BMD=$BM,BND=$BN,BZD=$BZ,CAD=$CA,CLP=$CL,CNY=CNY,COP=$CO,FJD=$FJ,FKP=\u{a3}FK,GBP=\u{a3}GB,GIP=\u{a3}GI,HKD=HKD,JPY=JPY,LBP=\u{a3}LB,MXN=$MX,NAD=$NA,NZD=$NZ,SBD=$SB,SGD=$SG,SRD=$SR,TTD=$TT,TWD=TWD,USD=$US,UYU=$UY,WST=$WS,XCD=XCD,XPF=FCFP,CYP=\u{a3}CY\n\
fr-CI\t,\t\u{202f}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tARS=$AR,AUD=$AU,BMD=$BM,BND=$BN,BZD=$BZ,CAD=$CA,CLP=$CL,CNY=CNY,COP=$CO,FJD=$FJ,FKP=\u{a3}FK,GBP=\u{a3}GB,GIP=\u{a3}GI,HKD=HKD,JPY=JPY,LBP=\u{a3}LB,MXN=$MX,NAD=$NA,NZD=$NZ,SBD=$SB,SGD=$SG,SRD=$SR,TTD=$TT,TWD=TWD,USD=$US,UYU=$UY,WST=$WS,XCD=XCD,XPF=FCFP,CYP=\u{a3}CY\n\
fr-LU\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tARS=$AR,AUD=$AU,BMD=$BM,BND=$BN,BZD=$BZ,CAD=$CA,CLP=$CL,CNY=CNY,COP=$CO,FJD=$FJ,FKP=\u{a3}FK,GBP=\u{a3}GB,GIP=\u{a3}GI,HKD=HKD,JPY=JPY,LBP=\u{a3}LB,MXN=$MX,NAD=$NA,NZD=$NZ,SBD=$SB,SGD=$SG,SRD=$SR,TTD=$TT,TWD=TWD,USD=$US,UYU=$UY,WST=$WS,XCD=XCD,XPF=FCFP,CYP=\u{a3}CY\n\
de-LU\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AU$,JPY=\u{a5},THB=\u{e3f},USD=$\n\
de-LI\t.\t\u{2019}\t\t\t-\t0\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AU$,EUR=EUR,JPY=\u{a5},THB=\u{e3f},USD=$\n\
hr-HR\t,\t.\t\t\t\u{2212}\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XCD=XCD,XPF=XPF,HRK=kn\n\
sl-SI\t,\t.\t\t\t\u{2212}\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,BRL=BRL,CAD=CAD,GBP=GBP,JPY=\u{a5},MXN=MXN,NZD=NZD,TWD=TWD,USD=$,XCD=XCD\n\
sk-SK\t,\t\u{a0}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=NIS,INR=INR,JPY=JPY,KRW=KRW,NZD=NZD,TWD=TWD,USD=USD,VND=VND\n\
bg-BG\t,\t\u{a0}\t\t\t-\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=\u{449}.\u{434}.,VND=VND,XCD=XCD,BGN=\u{43b}\u{432}.\n\
lt-LT\t,\t\u{a0}\t\t\t\u{2212}\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF,XPF=XPF\n\
lv-LV\t,\t\u{a0}\t\t\t-\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AU$,JPY=\u{a5},THB=\u{e3f},USD=$,LVL=Ls\n\
et-EE\t,\t\u{a0}\t\t\t\u{2212}\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AU$,JPY=\u{a5},THB=\u{e3f},USD=$,EEK=kr\n\
is-IS\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,BRL=BRL,CAD=CAD,EUR=EUR,GBP=GBP,INR=INR,ISK=kr.,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND\n\
mk-MK\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MKD=\u{434}\u{435}\u{43d}.,NZD=NZD,TWD=TWD,VND=VND\n\
sr-Latn-RS\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,BAM=KM,JPY=\u{a5},KRW=KRW,NZD=NZD,VND=VND\n\
bs-Latn-BA\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,BAM=KM,BRL=BRL,CAD=CAD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=ILS,JPY=\u{a5},MXN=MXN,NZD=NZD,RSD=din.,THB=\u{e3f},USD=USD,XCD=XCD,XPF=XPF,HRK=kn\n\
sq-AL\t,\t\u{a0}\t\t\t-\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tALL=Lek\u{eb},BRL=BRL,THB=\u{e3f}\n\
ka-GE\t,\t\u{a0}\t\t\t-\t2\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,CNY=CNY,GEL=\u{20be},HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,NZD=NZD,VND=VND\n\
hy-AM\t,\t\u{a0}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAMD=\u{58f},THB=\u{e3f},USD=$\n\
az-Latn-AZ\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAZN=\u{20bc},THB=\u{e3f}\n\
kk-KZ\t,\t\u{a0}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tKZT=\u{20b8},LSL=\u{41b}\u{421}\u{41b},RUB=\u{20bd},THB=\u{e3f},USD=$\n\
uz-Latn-UZ\t,\t\u{a0}\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tUZS=so\u{2bb}m\n\
sw-TZ\t.\t,\t\t\t-\t0\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tKES=Ksh,THB=\u{e3f},TZS=TSh\n\
sw-KE\t.\t,\t\t\t-\t0\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tKES=Ksh,THB=\u{e3f},TZS=TSh,USD=$\n\
zh-SG\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AU$,SGD=$\n\
zh-MO\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AU$,JPY=\u{a5},MOP=MOP$\n\
ta-IN\t.\t,\t\t\t-\t5\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tJPY=\u{a5},THB=\u{e3f},USD=$\n\
te-IN\t.\t,\t\t\t-\t5\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tTHB=\u{e3f},USD=$\n\
gu-IN\t.\t,\t\t\t-\t5\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tTHB=\u{e3f}\n\
kn-IN\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tTHB=\u{e3f},USD=$\n\
ml-IN\t.\t,\t\t\t-\t4\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tJPY=\u{a5},THB=\u{e3f},USD=$\n\
pa-IN\t.\t,\t\t\t-\t5\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tTHB=\u{e3f}\n\
km-KH\t.\t,\t\t\t-\t0\t#\u{a4}\t-#\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tKHR=\u{17db},LSL=\u{17a1}\u{17bc}\u{1791}\u{17b8},THB=\u{e3f},USD=$\n\
lo-LA\t,\t.\t\t\t-\t0\t\u{a4}#\t\u{a4}-#\t\u{a4}\u{a0}#\t\u{a4}-#\tLAK=\u{20ad},THB=\u{e3f}\n\
si-LK\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tLKR=\u{dbb}\u{dd4}.,THB=\u{e3f},XOF=\u{dc3}\u{dd2}\u{dc6}\u{dca}\u{d91}\n\
am-ET\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tAUD=AU$,ETB=\u{1265}\u{122d},THB=\u{e3f}\n\
zu-ZA\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tTHB=\u{e3f},USD=$,ZAR=R\n\
af-ZA\t,\t\u{a0}\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tCAD=CAD,MXN=MXN,THB=\u{e3f},USD=USD,ZAR=R\n\
ca-ES\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AU$,BRL=BRL,CAD=CAD,CNY=CNY,JPY=\u{a5},MXN=MXN,THB=\u{e3f},USD=USD,XCD=XCD\n\
eu-ES\t,\t.\t\t\t\u{2212}\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tTHB=\u{e3f}\n\
gl-ES\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tMXN=$MX,THB=\u{e3f},USD=$,XCD=XCD\n\
ga-IE\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tJPY=\u{a5},PHP=\u{20b1},THB=\u{e3f},USD=$\n\
cy-GB\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\tKRW=KRW,THB=\u{e3f}\n\
mt-MT\t.\t,\t\t\t-\t0\t\u{a4}#\t-\u{a4}#\t\u{a4}\u{a0}#\t-\u{a4}\u{a0}#\t\n\
lb-LU\t,\t.\t\t\t-\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AU$,JPY=\u{a5},THB=\u{e3f},USD=$\n\
nn-NO\t,\t\u{a0}\t\t\t\u{2212}\t0\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\t#\u{a0}\u{a4}\t-#\u{a0}\u{a4}\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NOK=kr,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XPF=XPF";

/// The decimals CLDR writes for a currency where they are not its ISO 4217 minor units
/// (`Intl.NumberFormat` writes IRR 1,234, not IRR 1,234.00): what `money.format` shows. An
/// amount keeps its minor units (FEEDBACK50.md, Q-627; branch feedback50-fixes).
pub const DISPLAY: &str = "AFN0 ALL0 IQD0 IRR0 KPW0 LAK0 LBP0 MGA0 MMK0 RSD0 SOS0 SYP0 YER0 MRO0 SLL0 STD0 TMM0 ZMK0 ZWD0";

pub const BUILTINS: &[(&str, BuiltinFn)] = &[
    ("money.of", of),
    ("money.parse", parse),
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

/// Currencies withdrawn since 2007, with their minor units: amounts kept in them still
/// read, so last year's invoice in leva is not an error in every `money` function
/// (FEEDBACK50.md, Q-630; branch feedback50-fixes). runtime.js has the same table.
pub const WITHDRAWN: &str = "ANG2 BGN2 BYR0 CYP2 EEK2 GHC2 HRK2 LTL2 LVL2 MRO2 MTL2 SDD2 SIT2 SKK2 SLL2 STD2 TMM2 VEB2 VEF2 ZMK2 ZWD2 ZWL2 ZWN2 ZWR2";

/// A currency's minor units, by its code (upper case): an active one, or a withdrawn one.
pub fn units_of(code: &str) -> Option<u32> {
    static T: OnceLock<HashMap<&'static str, u32>> = OnceLock::new();
    T.get_or_init(|| {
        CURRENCIES.split_whitespace().chain(WITHDRAWN.split_whitespace()).map(|c| (&c[..3], c[3..].parse().unwrap())).collect()
    })
    .get(code)
    .copied()
}

struct Locale {
    name: &'static str,
    decimal: &'static str,
    group: &'static str,
    /// A number's marks, when not money's.
    number: (&'static str, &'static str),
    minus: &'static str,
    indian: bool,
    /// Indian grouping in other numbers (ml-IN groups numbers so, not money).
    number_indian: bool,
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
                    number_indian: flags & 4 != 0,
                    min2: flags & 2 != 0,
                    pats: [f[7], f[8], f[9], f[10]],
                    symbols: symbol_pairs(f.get(11).copied().unwrap_or("")),
                }
            })
            .collect()
    })
}

/// The decimals `money.format` writes for a currency: CLDR's, else its minor units.
fn display_digits(code: &str, units: u32) -> u32 {
    static T: OnceLock<HashMap<&'static str, u32>> = OnceLock::new();
    T.get_or_init(|| DISPLAY.split_whitespace().map(|c| (&c[..3], c[3..].parse().unwrap())).collect()).get(code).copied().unwrap_or(units)
}

/// Regions whose Spanish CLDR writes as Latin America's (es-419), and whose Portuguese as
/// Portugal's: its parent locales.
const ES_419: &[&str] = &["419", "AR", "BO", "BZ", "CL", "CO", "CR", "CU", "DO", "EC", "GT", "HN", "MX", "NI", "PA", "PE", "PR", "PY", "SV", "US", "UY", "VE"];
const PT_PT: &[&str] = &["AO", "CH", "CV", "GQ", "GW", "LU", "MO", "MZ", "PT", "ST", "TL"];

/// A locale's data: by its name (`en-US`, `en_us`, `sr-Latn-RS`); for Chinese by its script
/// (`zh-Hant` as zh-TW, `zh-Hans-SG` as zh-SG); else by its language and region, the script
/// left out; for Spanish of Latin America es-419, for Portuguese of Africa and Asia pt-PT, as
/// CLDR's parent locales are; else the first of its language (`de`: de-DE); else en-US.
/// Extensions (`-u-ca-gregory`, `-x-...`) are not read. es-CO was Spain's, zh-Hant-TW
/// mainland China's (FEEDBACK50.md, Q-627; branch feedback50-fixes). The second value: whether
/// the name was matched or taken for what it says, not only written as something else.
fn locale_found(name: &str) -> (&'static Locale, bool) {
    let all = locales();
    let name = name.replace('_', "-");
    let find = |n: &str| all.iter().find(|l| l.name.eq_ignore_ascii_case(n));
    if let Some(l) = find(&name) {
        return (l, true);
    }
    let subs: Vec<&str> = name.split('-').collect();
    let lang = subs[0].to_ascii_lowercase();
    // Up to an extension's one-letter key (`-u-`, `-x-`).
    let rest: Vec<&str> = subs[1..].iter().copied().take_while(|s| !(s.len() == 1 && s.bytes().all(|b| b.is_ascii_alphanumeric()))).collect();
    let alpha = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_alphabetic());
    let script = rest.first().filter(|s| alpha(s, 4)).map(|s| s.to_ascii_lowercase());
    let region = rest
        .get(script.is_some() as usize)
        .filter(|s| alpha(s, 2) || (s.len() == 3 && s.bytes().all(|b| b.is_ascii_digit())))
        .map(|s| s.to_ascii_uppercase());
    let (script, region) = (script.as_deref(), region.as_deref());
    let picked = match (lang.as_str(), script, region) {
        ("zh", Some("hant"), Some(r @ ("HK" | "MO"))) => Some(format!("zh-{}", r)),
        ("zh", Some("hant"), _) => Some("zh-TW".to_string()),
        ("zh", Some("hans"), Some("SG")) => Some("zh-SG".to_string()),
        ("zh", Some("hans"), _) => Some("zh-CN".to_string()),
        (_, _, Some(r)) if find(&format!("{}-{}", lang, r)).is_some() => Some(format!("{}-{}", lang, r)),
        ("es", _, Some(r)) if ES_419.contains(&r) => Some("es-419".to_string()),
        ("pt", _, Some(r)) if PT_PT.contains(&r) => Some("pt-PT".to_string()),
        _ => None,
    };
    if let Some(l) = picked.and_then(|n| find(&n)) {
        return (l, true);
    }
    match all.iter().find(|l| l.name.split('-').next().map_or(false, |x| x.eq_ignore_ascii_case(&lang))) {
        // A language alone (or with its own script: `sr-Latn`, `en-Latn`) names its first
        // locale; with a region not here, it is written as another country's.
        Some(l) => {
            let row_script = l.name.split('-').nth(1).filter(|s| s.len() == 4);
            let same_script = script.map_or(true, |s| row_script.map_or(s == "latn", |r| r.eq_ignore_ascii_case(s)));
            (l, region.is_none() && same_script)
        }
        None => (&all[0], false),
    }
}

/// A locale's data (`locale_found`), said in the log once for each name written as another
/// (at most 64 names).
fn locale_data(name: &str) -> &'static Locale {
    let (loc, found) = locale_found(name);
    if !found {
        static SAID: OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = OnceLock::new();
        let mut said = SAID.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if said.len() < 64 && !said.contains(name) {
            said.insert(name.to_string());
            eprintln!("quillo: warning: no money and number data for locale {:?}: written as {}", name, loc.name);
        }
    }
    loc
}

/// A currency sign (Unicode's Sc): a symbol whose edge is one meets the number; one whose edge
/// is anything else (a letter, `.` as in `Cg.`, a mark) is set apart with a no-break space, as
/// ICU does (FEEDBACK50.md, Q-627; branch feedback50-fixes).
fn is_currency_sign(c: char) -> bool {
    matches!(c, '$' | '\u{a2}'..='\u{a5}' | '\u{58f}' | '\u{60b}' | '\u{7fe}' | '\u{7ff}' | '\u{9f2}' | '\u{9f3}' | '\u{9fb}' | '\u{af1}' | '\u{bf9}'
        | '\u{e3f}' | '\u{17db}' | '\u{20a0}'..='\u{20c0}' | '\u{a838}' | '\u{fdfc}' | '\u{fe69}' | '\u{ff04}' | '\u{ffe0}' | '\u{ffe1}' | '\u{ffe5}' | '\u{ffe6}'
        | '\u{11fdd}'..='\u{11fe0}' | '\u{1e2ff}' | '\u{1ecb0}')
}

fn symbol(loc: &Locale, code: &str) -> String {
    static BASE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    loc.symbols
        .get(code)
        .or_else(|| BASE.get_or_init(|| symbol_pairs(SYMBOLS)).get(code))
        .map_or_else(|| code.to_string(), |s| s.to_string())
}

/// The digits of a decimal's magnitude, grouped, with these marks.
fn digits(loc: &Locale, (decimal, group): (&str, &str), indian: bool, d: Dec) -> String {
    let text = d.abs().to_string();
    let (whole, frac) = text.split_once('.').map_or((text.as_str(), ""), |(w, f)| (w, f));
    let mut out = String::new();
    let n = whole.len();
    let grouped = !(loc.min2 && n < 5);
    for (i, c) in whole.chars().enumerate() {
        let left = n - i;
        if i > 0 && grouped {
            let cut = if indian { left == 3 || (left > 3 && (left - 3) % 2 == 0) } else { left % 3 == 0 };
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
    let letters = side.map_or(false, |c| !is_currency_sign(c));
    let pat = loc.pats[if letters { 2 } else { 0 } + neg as usize];
    pat.replace('-', loc.minus).replace('\u{a4}', &sym).replace('#', &digits(loc, (loc.decimal, loc.group), loc.indian, d))
}

/// `format_number(x, locale, digits)`: a number as `locale` writes one.
pub fn format_number(d: Dec, places: Option<u32>, loc_name: &str) -> String {
    format_number_between(d, places.map(|p| (p, p)), Rounding::HalfUp, loc_name)
}

/// `format_number` with at least `min` and at most `max` digits after the point. A tie is
/// rounded away from zero, from the number's shortest decimal, as `Intl.NumberFormat` and
/// `round` do (FEEDBACK30.md, Q-369; branch feedback27-35-fixes), unless `mode` says otherwise:
/// half to even writes what `money` works out (FEEDBACK50.md, Q-626; branch feedback50-fixes).
pub fn format_number_between(d: Dec, places: Option<(u32, u32)>, mode: Rounding, loc_name: &str) -> String {
    let loc = locale_data(loc_name);
    // A number below 0 keeps its sign when it rounds to 0, as `Intl` and Python write -0.001 to two
    // digits: "-0.00" (FEEDBACK55.md, not counted; branch feedback55).
    let neg = d.is_negative();
    let d = match places {
        Some((min, max)) => {
            let mut r = d.round(max as i32, mode).unwrap_or(d);
            while r.scale() > min && r.mantissa() % 10 == 0 {
                r = Dec::new(r.mantissa() / 10, r.scale() - 1).unwrap_or(r);
            }
            r
        }
        None => d,
    };
    let sign = if neg { loc.minus } else { "" };
    format!("{}{}", sign, digits(loc, loc.number, loc.number_indian, d))
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
            Ok(units_of(&code).map(|u| (code, u)).ok_or_else(|| Value::err(Value::str(format!("unknown currency {}", Value::Str(s.clone()).repr())))))
        }
        other => fail(line, format!("`{}` expects a currency code (a str) as argument {}, got {}", f, i + 1, tn(other))),
    }
}

/// Argument `i` as money: a map with a known `currency` and an `amount` (a decimal, an int,
/// or numeric text, as JSON brings it), held to what `money.of` holds an amount to: the code
/// trimmed, no exponent in text, no more decimals than the currency has, and minor units that
/// fit an int. Every function sees one amount: `compare` and `is_zero` took $1.005 as it was
/// while `add` and `format` rounded it to $1.00, and `"1e3"` in a map was 1,000
/// (FEEDBACK50.md, Q-622 and Q-623; branch feedback50-fixes).
fn money(args: &[Value], i: usize, f: &str, line: u32) -> Result<Money, Ctrl> {
    read(args.get(i), i, f, line, true)
}

/// `money`, with `exact` false for `money.round`, which takes an amount made by hand with
/// more decimals than its currency has.
fn read(v: Option<&Value>, i: usize, f: &str, line: u32, exact: bool) -> Result<Money, Ctrl> {
    let bad = |why: String| fail(line, format!("`{}` expects money ({{ amount, currency }}) as argument {}, got {}{}", f, i + 1, tn(v), why));
    let Some(Value::Map(m)) = v else { return bad(String::new()) };
    let code = match m.get("currency") {
        Some(Value::Str(s)) => s.trim().to_ascii_uppercase(),
        _ => return bad(": no currency".into()),
    };
    let Some(units) = units_of(&code) else { return bad(": unknown currency".into()) };
    let amount = match m.get("amount") {
        Some(Value::Dec(d)) => *d,
        Some(Value::Int(n)) => Dec::from_i64(*n),
        Some(Value::Str(s)) => match Dec::parse(s.trim()).filter(|_| !s.contains(['e', 'E'])) {
            Some(d) => d,
            None => return bad(format!(": its amount {} is not an amount", Value::Str(s.clone()).repr())),
        },
        Some(Value::Float(_)) => return bad(": its amount is a float: make it with money.of(dec(x), currency)".into()),
        _ => return bad(": its amount must be a dec".into()),
    };
    if !exact {
        return Ok(Money { amount, code, units });
    }
    // An amount with no room for the currency's decimals stays as it is, and is too large, as
    // one whose minor units do not fit an int is.
    let fitted = amount.round(units as i32, Rounding::Down).unwrap_or(amount);
    if fitted != amount {
        return fail(line, format!("`{}`: {} has more decimals than {} has ({}): round it first with money.round(m, mode)", f, amount, code, units));
    }
    if minor_of(fitted, units).is_none() {
        return fail(line, format!("`{}`: {} {} is too large an amount", f, amount, code));
    }
    Ok(Money { amount: fitted, code, units })
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
    move |e| Ctrl::Error(Box::new(crate::QuilloError::new(e.message(), line)))
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
        // Text is an amount as people write one: "1e3" is not 1,000
        // (FEEDBACK14.md, Q-096; branch feedback14-fixes).
        Some(Value::Str(s)) => match Dec::parse(s.trim()).filter(|_| !s.contains(['e', 'E'])) {
            Some(d) => d,
            None => return Ok(Value::err(Value::str(format!("not an amount: {}", Value::Str(s.clone()).repr())))),
        },
        Some(Value::Float(_)) => return fail(line, "`money.of` takes a dec, an int or text, not a float: convert it with dec(x)"),
        other => return fail(line, format!("`money.of` expects an amount (a dec, int or str) as argument 1, got {}", tn(other))),
    };
    of_amount(amount, code, units, &args, 2, "money.of", line)
}

/// The rest of `money.of` and `money.parse`: the amount to the currency's units (rounded as
/// argument `m` says, else exact, or an error value), and minor units that fit an int.
fn of_amount(amount: Dec, code: String, units: u32, args: &[Value], m: usize, f: &str, line: u32) -> R {
    let rounded = match args.get(m) {
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
        _ => to_units(amount, units, mode(args, m, f, line)?, line)?,
    };
    // Money whose minor units do not fit an int is an error value here,
    // where a form's amount is checked, not a runtime error later in
    // `money.to_minor` (FEEDBACK14.md, Q-096; branch feedback14-fixes).
    if minor_of(rounded, units).is_none() {
        return Ok(Value::err(Value::str(format!("{} {} is too large an amount", rounded, code))));
    }
    Ok(Money { amount: rounded, code, units }.value())
}

/// `money.parse(text, currency, locale, mode)`: an amount as `locale` writes one, for a form:
/// its decimal and group marks (whole groups only), a minus, and the currency's symbol or
/// code before or after. `money.of` reads text as a program writes it, so a German visitor's
/// "1.000", a thousand, was one euro (FEEDBACK50.md, Q-624; branch feedback50-fixes).
fn parse(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    const F: &str = "money.parse";
    let text = match args.first() {
        Some(Value::Str(s)) => s.clone(),
        other => return fail(line, format!("`{}` expects text as argument 1, got {}", F, tn(other))),
    };
    let (code, units) = match currency(&args, 1, F, line)? {
        Ok(c) => c,
        Err(e) => return Ok(e),
    };
    let loc = locale_data(&locale_arg(it, &args, 2, F, line)?);
    match read_amount(&text, &code, loc) {
        Some(amount) => of_amount(amount, code, units, &args, 3, F, line),
        None => Ok(Value::err(Value::str(format!("not an amount: {}", Value::Str(text).repr())))),
    }
}

/// Spaces a locale writes between groups, or that people type for them.
fn spacey(c: char) -> bool {
    matches!(c, ' ' | '\u{a0}' | '\u{202f}')
}

/// Text as `loc` writes an amount of `code`, as a decimal (`money.parse`).
fn read_amount(text: &str, code: &str, loc: &Locale) -> Option<Dec> {
    let marks = |c: char| spacey(c) || matches!(c, '\u{200e}' | '\u{200f}' | '\u{61c}');
    let mut t = text.trim().trim_matches(marks).to_string();
    if t.is_empty() || t.chars().count() > 400 {
        return None;
    }
    // The currency, at either end: its code, or a symbol any locale writes for it (`¥` and
    // `￥`, `$` and `US$`), the longest first.
    let mut names: Vec<String> = locales().iter().map(|l| symbol(l, code)).chain([code.to_string()]).collect();
    names.sort_by_key(|n| (std::cmp::Reverse(n.chars().count()), n.clone()));
    names.dedup();
    // A minus before the currency (`-$5`) or after it (`$-5`, `€ -5`).
    let sign = |t: &mut String| -> bool {
        match [loc.minus, "-", "\u{2212}"].iter().find_map(|m| t.strip_prefix(m)) {
            Some(rest) => {
                *t = rest.trim_matches(marks).to_string();
                true
            }
            None => false,
        }
    };
    let mut neg = sign(&mut t);
    if let Some(n) = names.iter().find(|n| t.starts_with(n.as_str()) || t.ends_with(n.as_str())) {
        t = t.strip_prefix(n.as_str()).or_else(|| t.strip_suffix(n.as_str())).unwrap_or(&t).trim_matches(marks).to_string();
    }
    if !neg {
        neg = sign(&mut t);
    }
    // The whole part's groups and the fraction, by the locale's marks.
    let group_spaced = loc.group.chars().all(spacey);
    let group = |c: char| if group_spaced { spacey(c) } else { loc.group.contains(c) || (loc.group == "\u{2019}" && c == '\'') };
    let (whole, frac) = match t.split_once(loc.decimal) {
        Some((w, f)) => (w, Some(f)),
        None => (t.as_str(), None),
    };
    if frac.is_some_and(|f| f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let groups: Vec<&str> = whole.split(group).collect();
    if groups.iter().any(|g| g.is_empty() || !g.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    if groups.len() > 1 {
        // The first group of 1 to 3 digits, the others of 3; in Indian grouping also the last
        // of 3 and those before it of 2.
        let (last, rest) = groups.split_last().unwrap();
        let western = groups[0].len() <= 3 && groups[1..].iter().all(|g| g.len() == 3);
        let ok = western || (loc.indian && last.len() == 3 && rest[0].len() <= 2 && rest[1..].iter().all(|g| g.len() == 2));
        if !ok {
            return None;
        }
    }
    let digits = format!("{}{}{}", if neg { "-" } else { "" }, groups.concat(), frac.map_or(String::new(), |f| format!(".{}", f)));
    Dec::parse(&digits)
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
    // `money` has the amount at the currency's units, and its minor units fit an int.
    let m = money(&args, 0, "money.to_minor", line)?;
    Ok(Value::Int(m.amount.mantissa() as i64))
}

/// An amount with `units` decimals as an int of minor units, if it fits.
fn minor_of(d: Dec, units: u32) -> Option<i64> {
    i64::try_from(d.mantissa()).ok().filter(|_| d.scale() == units)
}

/// Money past what `money.of` takes, as `money.of` says it: an error value, where `money.add`,
/// `money.sub` and `money.mul` made amounts that only `money.to_minor` refused, with "integer overflow"
/// (FEEDBACK49.md, Q-605; branch main).
fn in_range(m: Money) -> Value {
    match minor_of(m.amount, m.units) {
        Some(_) => m.value(),
        None => Value::err(Value::str(format!("{} {} is too large an amount", m.amount, m.code))),
    }
}

/// What the `money` type makes of data, in a pattern or a record's field: a map of an
/// `amount` (a decimal, an int, numeric text, or a JSON number through its shortest text) and
/// a `currency`, held to `money.of`'s rules with no rounding mode, and made money; else what is
/// wrong, without the value sent (it may go to the log). `type Money = { amount: dec, currency: str }` checked the shape only, so $10.004 was
/// taken and dropped to $10.00, "1e3" was 1,000, and BTC was a 500 at the first `money` call
/// (FEEDBACK50.md, Q-621; branch feedback50-fixes).
pub fn from_data(v: &Value) -> Result<Value, String> {
    let Value::Map(m) = v else { return Err("must be money, { amount, currency }".into()) };
    let code = match m.get("currency") {
        Some(Value::Str(s)) => s.trim().to_ascii_uppercase(),
        Some(_) => return Err("has a currency that is not text".into()),
        None => return Err("has no currency".into()),
    };
    let Some(units) = units_of(&code) else { return Err("has a currency that is not an ISO 4217 code".into()) };
    let amount = match m.get("amount") {
        Some(Value::Dec(d)) => Some(*d),
        Some(Value::Int(n)) => Some(Dec::from_i64(*n)),
        Some(Value::Float(f)) => Dec::from_f64(*f),
        Some(Value::Str(s)) => Dec::parse(s.trim()).filter(|_| !s.contains(['e', 'E'])),
        Some(_) => None,
        None => return Err("has no amount".into()),
    };
    let Some(amount) = amount else { return Err("has an amount that is not a number".into()) };
    let fitted = amount.round(units as i32, Rounding::Down).unwrap_or(amount);
    if fitted != amount {
        return Err(format!("has more decimals than {} has ({})", code, units));
    }
    if minor_of(fitted, units).is_none() {
        return Err(format!("is too large an amount of {}", code));
    }
    Ok(Money { amount: fitted, code, units }.value())
}

/// Whether a value is money as every `money` function takes it: what an annotation's `money`
/// holds a value to.
pub fn is_money(v: &Value) -> bool {
    read(Some(v), 0, "", 0, true).is_ok()
}

/// Whether a value has money's shape: a map of exactly `amount` and `currency`.
fn shaped(v: &Value) -> bool {
    matches!(v, Value::Map(m) if m.len() == 2 && m.contains_key("amount") && m.contains_key("currency"))
}

/// `a + b` and `a - b` when both have money's shape: the amounts added, as `money.add` does,
/// and two currencies an error value. `+` merged the maps, so a cart's total was its last
/// line, and dollars plus euros were the euros (FEEDBACK50.md, Q-620; branch feedback50-fixes).
/// None for other values, which `+` merges and `-` refuses as before.
pub fn op(a: &Value, b: &Value, sub: bool, line: u32) -> Option<R> {
    if !(shaped(a) && shaped(b)) {
        return None;
    }
    let (sym, verb) = if sub { ("-", "subtract") } else { ("+", "add") };
    let both = read(Some(a), 0, sym, line, true).and_then(|x| Ok((x, read(Some(b), 1, sym, line, true)?)));
    Some(both.map(|(x, y)| {
        if x.code != y.code {
            return Value::err(Value::str(format!("cannot {} {} and {}: convert one currency to the other first", verb, x.code, y.code)));
        }
        sum(x, y, sub)
    }))
}

/// `a + b` or `a - b` of two amounts of one currency, as `money` reads them: at the currency's
/// units, with minor units that fit an int, so the answer is exact and fits a decimal.
fn sum(a: Money, b: Money, sub: bool) -> Value {
    let r = if sub { a.amount.sub(b.amount) } else { a.amount.add(b.amount) };
    in_range(Money { amount: r.expect("two ints' worth of minor units fit a decimal"), ..a })
}

fn add(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (a, b) = (money(&args, 0, "money.add", line)?, money(&args, 1, "money.add", line)?);
    same_currency(&a, &b, "add", line)?;
    Ok(sum(a, b, false))
}

fn sub(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (a, b) = (money(&args, 0, "money.sub", line)?, money(&args, 1, "money.sub", line)?);
    same_currency(&a, &b, "subtract", line)?;
    Ok(sum(a, b, true))
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
    Ok(in_range(Money { amount: to_units(product, m.units, mode, line)?, ..m }))
}

fn neg(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let m = money(&args, 0, "money.neg", line)?;
    // `-(-9223372036854775808)` cents is one past what fits: an error value, as from
    // `money.add` (found in review).
    Ok(in_range(Money { amount: m.amount.neg(), ..m }))
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
    let m = read(args.first(), 0, "money.round", line, false)?;
    let mode = mode(&args, 1, "money.round", line)?;
    Ok(in_range(Money { amount: to_units(m.amount, m.units, mode, line)?, ..m }))
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
/// left over go one each to the shares closest to their next unit (the
/// largest remainders, Hamilton's method; a tie to the first), so the
/// shares add up to the amount and none gets more than a unit past its part.
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
    let minor = m.amount.mantissa();
    let (neg, units) = (minor < 0, minor.unsigned_abs() as i128);
    let mut shares = Vec::with_capacity(whole.len());
    let mut rests = Vec::with_capacity(whole.len());
    for r in &whole {
        match units.checked_mul(*r) {
            Some(p) => {
                shares.push(p / total_ratio);
                rests.push(p % total_ratio);
            }
            None => return fail(line, format!("`{}`: the ratios are too large", F)),
        }
    }
    // The units left go to the largest remainders, not to the first shares: of $0.10 split
    // 1:99, the 1% share got $0.01, a tenth (FEEDBACK50.md, Q-631; branch feedback50-fixes).
    // A share with no ratio has no remainder, and fewer units are left than shares with one.
    let left = (units - shares.iter().sum::<i128>()) as usize;
    let mut order: Vec<usize> = (0..shares.len()).collect();
    order.sort_by(|a, b| rests[*b].cmp(&rests[*a]).then(a.cmp(b)));
    for k in order.into_iter().take(left) {
        shares[k] += 1;
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
    // Fewer decimals shown than the amount has (IRR's none): rounded as Intl rounds, a tie away
    // from zero (found in review of FEEDBACK50's fixes).
    let shown = display_digits(&m.code, m.units);
    let amount = m.amount.round(shown as i32, Rounding::HalfUp).unwrap_or(m.amount);
    Ok(Value::str(format_amount(amount, &m.code, shown, &loc)))
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
    // Digits after the point: exactly `n`, or `{ min, max }` as `Intl` takes them, so 10 and
    // 8.875 come from one call (FEEDBACK30.md; branch feedback27-35-fixes).
    let places = match args.get(2) {
        None | Some(Value::Nil) => None,
        Some(Value::Int(n)) => Some(((*n).clamp(0, 28) as u32, (*n).clamp(0, 28) as u32)),
        Some(Value::Map(m)) => {
            if let Some(k) = m.keys().find(|k| !matches!(&***k, "min" | "max")) {
                return fail(line, format!("`{}`'s digits are `{{ min, max }}`, not `{}`", F, k));
            }
            let get = |k: &str| match m.get(k) {
                None | Some(Value::Nil) => Ok(None),
                Some(Value::Int(n)) => Ok(Some((*n).clamp(0, 28) as u32)),
                Some(other) => Err(format!("`{}`'s {} is an int, got {}", F, k, tn(Some(other)))),
            };
            let (min, max) = match (get("min"), get("max")) {
                (Ok(a), Ok(b)) => (a, b),
                (Err(e), _) | (_, Err(e)) => return fail(line, e),
            };
            let min = min.unwrap_or(0);
            let max = max.unwrap_or(min.max(3));
            if min > max {
                return fail(line, format!("`{}`: min {} is more than max {}", F, min, max));
            }
            Some((min, max))
        }
        other => return fail(line, format!("`{}` expects an int or `{{ min, max }}` as argument 3, got {}", F, tn(other))),
    };
    // How a tie rounds: away from zero unless told (FEEDBACK50.md, Q-626; branch feedback50-fixes).
    let rounding = match args.get(3) {
        None | Some(Value::Nil) => Rounding::HalfUp,
        Some(_) => mode(&args, 3, F, line)?,
    };
    let text = format_number_between(d, places, rounding, &loc);
    // A float below 0 that its decimal holds as 0 (-1e-30), and -0.0, are written "-0", as `Intl`
    // writes them; a decimal has no -0 to keep.
    let minus = locale_data(&loc).minus;
    if matches!(args.first(), Some(Value::Float(f)) if f.is_sign_negative()) && !text.starts_with(minus) {
        return Ok(Value::str(format!("{}{}", minus, text)));
    }
    Ok(Value::str(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_read_whole() {
        assert_eq!(CURRENCIES.split_whitespace().count(), 165);
        assert_eq!((units_of("JPY"), units_of("USD"), units_of("BHD"), units_of("CLF"), units_of("XAU")), (Some(0), Some(2), Some(3), Some(4), None));
        assert!(locales().iter().all(|l| l.pats.iter().all(|p| p.contains('#') && p.contains('\u{a4}'))));
        let named = |n: &str| (locale_found(n).0.name, locale_found(n).1);
        assert_eq!(named("de_lu"), ("de-LU", true));
        assert_eq!(named("DE-at"), ("de-AT", true));
        assert_eq!(named("de"), ("de-DE", true));
        assert_eq!(named("de-XX"), ("de-DE", false));
        assert_eq!(named("sr-Latn-RS"), ("sr-Latn-RS", true));
        assert_eq!(named("az-Cyrl-AZ"), ("az-Latn-AZ", false));
        assert_eq!(named("zh-Hant-TW"), ("zh-TW", true));
        assert_eq!(named("zh-Hant"), ("zh-TW", true));
        assert_eq!(named("zh-Hant-HK"), ("zh-HK", true));
        assert_eq!(named("zh-Hant-MO"), ("zh-MO", true));
        assert_eq!(named("zh-Hans-SG"), ("zh-SG", true));
        assert_eq!(named("zh-Hans"), ("zh-CN", true));
        assert_eq!(named("es-CO"), ("es-CO", true));
        assert_eq!(named("es-HN"), ("es-419", true));
        assert_eq!(named("es"), ("es-ES", true));
        assert_eq!(named("pt-CV"), ("pt-PT", true));
        assert_eq!(named("pt"), ("pt-BR", true));
        assert_eq!(named("es-419"), ("es-419", true));
        assert_eq!(named("zh-Hant-CN"), ("zh-TW", true));
        assert_eq!(named("zh-Hant-SG"), ("zh-TW", true));
        assert_eq!(named("zh-Hans-TW"), ("zh-CN", true));
        assert_eq!(named("zh-TW"), ("zh-TW", true));
        assert_eq!(named("en-u-ca-gregory"), ("en-US", true));
        assert_eq!(named("fr-u-ca-gregory"), ("fr-FR", true));
        assert_eq!(named("es-x-ca"), ("es-ES", true));
        assert_eq!(named("de-CH-u-nu-latn"), ("de-CH", true));
        assert_eq!(named("es-GQ"), ("es-ES", false));
        assert_eq!(named("es-150"), ("es-ES", false));
        assert_eq!(named("pt-US"), ("pt-BR", false));
        assert_eq!(named("pt-GW"), ("pt-PT", true));
        assert_eq!(named("pt-AO"), ("pt-AO", true));
        assert_eq!(named("sr-Latn"), ("sr-Latn-RS", true));
        assert_eq!(named("sr-Cyrl"), ("sr-Latn-RS", false));
        assert_eq!(named("en-Latn"), ("en-US", true));
        assert_eq!(named("es-\u{e9}"), ("es-ES", true));
        assert_eq!(named("es-\u{e9}\u{e9}"), ("es-ES", true));
        assert_eq!(named("\u{212a}k-KZ"), ("en-US", false));
        assert_eq!(named("en--US"), ("en-US", true));
        assert_eq!(named("xx"), ("en-US", false));
        assert_eq!(named(""), ("en-US", false));
        assert_eq!(locale_data("xx-YY").name, "en-US");
        assert_eq!(locale_data("xx-YY").name, "en-US");
        assert_eq!(display_digits("IRR", 2), 0);
        assert_eq!(display_digits("USD", 2), 2);
        assert!(['$', '\u{a2}', '\u{20ac}', '\u{20b9}', '\u{ffe5}', '\u{17db}'].iter().all(|c| is_currency_sign(*c)));
        assert!(['.', 'F', '/', '\u{17b8}', ' '].iter().all(|c| !is_currency_sign(*c)));
        assert!(['\u{11fdd}', '\u{11fe0}', '\u{1e2ff}', '\u{1ecb0}'].iter().all(|c| is_currency_sign(*c)));
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
        assert_eq!(constant("MONEY_WITHDRAWN"), WITHDRAWN);
        assert_eq!(constant("MONEY_DISPLAY"), DISPLAY);
        assert_eq!(constant("MONEY_SYMBOLS"), SYMBOLS);
        assert_eq!(constant("MONEY_LOCALES"), LOCALES);
    }
}
