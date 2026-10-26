//! `date` (src/dates.rs): the calendar, time zones across daylight saving
//! changes, business days, day counts against published reference values,
//! and the same answers in the browser (src/client/runtime.js).

use quillo::Interp;
use std::process::Command;

const DATA: &str = "let cal = { holidays: [\"2026-12-25\", \"2026-12-28\", \"2027-01-01\"] }";

fn run(body: &str) -> Result<String, String> {
    let src = format!("{}\nfn __t() {{\n{}\n}}\n", DATA, body);
    let mut it = Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => Ok(v.repr()),
        Err(quillo::interp::Ctrl::Error(e)) => Err(e.msg),
        Err(_) => Err("returned".into()),
    }
}

fn eval(body: &str) -> String {
    run(body).unwrap_or_else(|e| panic!("{}: {}", body, e))
}

/// The value as a str (the text without its quotes).
fn s(body: &str) -> String {
    let r = eval(body);
    serde_json::from_str::<String>(&r).unwrap_or_else(|_| panic!("{} gave {}", body, r))
}

fn float(body: &str) -> f64 {
    let r = eval(body);
    r.parse().unwrap_or_else(|_| panic!("{} gave {}", body, r))
}

fn error(body: &str) -> String {
    match run(body) {
        Err(e) => e,
        Ok(v) => panic!("{} gave {}, not an error", body, v),
    }
}

fn check(src: &str) -> Vec<String> {
    quillo::check::check_source(src, "main.qwe", std::path::Path::new(".")).errors().map(|d| d.message.clone()).collect()
}

#[test]
fn month_ends_are_clamped() {
    for (e, want) in [
        ("date.add_months(\"2026-01-31\", 1)", "2026-02-28"),
        ("date.add_months(\"2024-01-31\", 1)", "2024-02-29"),
        ("date.add_months(\"2026-03-31\", 1)", "2026-04-30"),
        ("date.add_months(\"2026-03-31\", -1)", "2026-02-28"),
        ("date.add_months(\"2026-05-31\", -3)", "2026-02-28"),
        ("date.add_months(\"2026-10-27\", 15)", "2028-01-27"),
        ("date.add_months(\"2026-10-27\", -22)", "2024-12-27"),
        ("date.add_months(\"2026-02-28\", 1)", "2026-03-28"),
        ("date.end_of_month(\"2026-02-10\")", "2026-02-28"),
        ("date.end_of_month(\"2024-02-10\")", "2024-02-29"),
        ("date.end_of_month(\"2026-12-01\")", "2026-12-31"),
        ("date.start_of_month(\"2026-10-27\")", "2026-10-01"),
    ] {
        assert_eq!(s(e), want, "{}", e);
    }
    for (e, want) in [
        ("date.months_between(\"2026-01-31\", \"2026-02-28\")", "1"),
        ("date.months_between(\"2026-01-31\", \"2026-02-27\")", "0"),
        ("date.months_between(\"2026-01-15\", \"2027-03-14\")", "13"),
        ("date.months_between(\"2026-01-15\", \"2027-03-15\")", "14"),
        ("date.months_between(\"2026-03-31\", \"2026-01-31\")", "-2"),
        ("date.months_between(\"2026-10-27\", \"2026-10-27\")", "0"),
        ("date.days_between(\"2026-01-01\", \"2026-12-31\")", "364"),
        ("date.days_between(\"2026-10-27\", \"2026-10-01\")", "-26"),
        ("date.days_between(\"1970-01-01\", \"2026-10-27\")", "20753"),
    ] {
        assert_eq!(eval(e), want, "{}", e);
    }
}

#[test]
fn leap_years() {
    assert_eq!(eval("[date.is_leap_year(2024), date.is_leap_year(2026), date.is_leap_year(1900), date.is_leap_year(2000), date.is_leap_year(2100), date.is_leap_year(\"2028-06-01\")]"), "[true, false, false, true, false, true]");
    assert_eq!(s("date.add_years(\"2024-02-29\", 1)"), "2025-02-28");
    assert_eq!(s("date.add_years(\"2024-02-29\", 4)"), "2028-02-29");
    assert_eq!(s("date.add_years(\"2024-02-29\", -100)"), "1924-02-29");
    assert_eq!(s("date.add_years(\"2000-02-29\", 100)"), "2100-02-28");
    assert_eq!(s("date.add_days(\"2024-02-28\", 1)"), "2024-02-29");
    assert_eq!(s("date.add_days(\"2023-02-28\", 1)"), "2023-03-01");
    assert_eq!(s("date.add_days(\"2026-12-31\", 1)"), "2027-01-01");
    assert_eq!(s("date.add_days(\"2026-10-27\", -10000)"), "1999-06-11");
    assert_eq!(eval("[date.days_in_month(2024, 2), date.days_in_month(2100, 2), date.days_in_month(\"2026-04-15\")]"), "[29, 28, 30]");
    assert_eq!(eval("date.days_between(\"2024-01-01\", \"2025-01-01\")"), "366");
    assert_eq!(eval("date.days_between(\"2000-01-01\", \"2100-01-01\")"), "36525");
}

#[test]
fn parts_weekdays_and_construction() {
    assert_eq!(eval("date.parts(\"2026-10-27\")"), "{ year: 2026, month: 10, day: 27, weekday: 2 }");
    assert_eq!(
        eval("date.parts(\"2026-10-27T14:05:09+05:30\")"),
        "{ year: 2026, month: 10, day: 27, weekday: 2, hour: 14, minute: 5, second: 9, offset: \"+05:30\" }"
    );
    // 1 is Monday, 7 Sunday.
    assert_eq!(eval("map([\"2026-10-26\", \"2026-11-01\", \"1970-01-01\", \"2000-01-01\", \"0001-01-01\"], (d) => date.weekday(d))"), "[1, 7, 4, 6, 1]");
    assert_eq!(s("date.of(2026, 2, 3)"), "2026-02-03");
    assert_eq!(s("date.of(5, 12, 31)"), "0005-12-31");
    assert_eq!(eval("[date.valid(\"2026-10-27\"), date.valid(\"2026-10-27T14:30:00Z\"), date.valid(\"2026-10-27T14:30:00.125-04:00\"), date.valid(\"2026-02-29\"), date.valid(\"2026-10-27T14:30:00\"), date.valid(\"2026-10-27T24:00:00Z\"), date.valid(20261027), date.valid(null)]"), "[true, true, true, false, false, false, false, false]");
    // As text, dates sort and compare in time order.
    assert_eq!(eval("sort([\"2026-10-27\", \"2025-12-31\", \"2026-01-02\"])"), "[\"2025-12-31\", \"2026-01-02\", \"2026-10-27\"]");
    assert_eq!(eval("\"2026-10-27\" < \"2026-11-01\""), "true");
}

#[test]
fn time_zones_across_daylight_saving() {
    for (e, want) in [
        // Paris, spring 2026: at 01:00 UTC on the last Sunday of March, 02:00 becomes 03:00.
        ("date.to_zone(\"2026-03-29T00:59:59Z\", \"Europe/Paris\")", "2026-03-29T01:59:59+01:00"),
        ("date.to_zone(\"2026-03-29T01:00:00Z\", \"Europe/Paris\")", "2026-03-29T03:00:00+02:00"),
        // Autumn: at 01:00 UTC on the last Sunday of October, 03:00 becomes 02:00 again.
        ("date.to_zone(\"2026-10-25T00:30:00Z\", \"Europe/Paris\")", "2026-10-25T02:30:00+02:00"),
        ("date.to_zone(\"2026-10-25T01:30:00Z\", \"Europe/Paris\")", "2026-10-25T02:30:00+01:00"),
        // New York, spring: 02:00 EST on the second Sunday of March becomes 03:00 EDT.
        ("date.to_zone(\"2026-03-08T06:59:59Z\", \"America/New_York\")", "2026-03-08T01:59:59-05:00"),
        ("date.to_zone(\"2026-03-08T07:00:00Z\", \"America/New_York\")", "2026-03-08T03:00:00-04:00"),
        // Autumn: 02:00 EDT on the first Sunday of November becomes 01:00 EST.
        ("date.to_zone(\"2026-11-01T05:30:00Z\", \"America/New_York\")", "2026-11-01T01:30:00-04:00"),
        ("date.to_zone(\"2026-11-01T06:30:00Z\", \"America/New_York\")", "2026-11-01T01:30:00-05:00"),
        // From one offset to another, the fraction of a second kept.
        ("date.to_zone(\"2026-10-27T15:30:00.250+01:00\", \"Asia/Kolkata\")", "2026-10-27T20:00:00.250+05:30"),
        ("date.to_zone(\"2026-10-27T10:30:00-04:00\", \"UTC\")", "2026-10-27T14:30:00Z"),
        ("date.to_zone(\"2026-10-27T14:30:00Z\", \"+09:00\")", "2026-10-27T23:30:00+09:00"),
        ("date.to_zone(\"2026-10-27T14:30:00Z\")", "2026-10-27T14:30:00Z"),
        // A local time in a gap is that far after it; in an overlap, the earlier one.
        ("date.at(\"2026-03-29\", \"02:30\", \"Europe/Paris\")", "2026-03-29T03:30:00+02:00"),
        ("date.at(\"2026-10-25\", \"02:30\", \"Europe/Paris\")", "2026-10-25T02:30:00+02:00"),
        ("date.at(\"2026-03-08\", \"02:30\", \"America/New_York\")", "2026-03-08T03:30:00-04:00"),
        ("date.at(\"2026-11-01\", \"01:30\", \"America/New_York\")", "2026-11-01T01:30:00-04:00"),
        ("date.at(\"2026-07-01\", \"17:00:00\", \"America/New_York\")", "2026-07-01T17:00:00-04:00"),
        ("date.at(\"2026-12-01\", \"17:00\", \"America/New_York\")", "2026-12-01T17:00:00-05:00"),
        ("date.at(\"2026-10-27\")", "2026-10-27T00:00:00Z"),
        // The calendar date there.
        ("date.date_in(\"2026-10-27T23:30:00Z\", \"Europe/Paris\")", "2026-10-28"),
        ("date.date_in(\"2026-10-27T03:00:00Z\", \"America/New_York\")", "2026-10-26"),
        ("date.date_in(\"2026-10-27T03:00:00+05:00\")", "2026-10-26"),
        ("date.date_in(\"2026-12-31T20:00:00-05:00\", \"Asia/Tokyo\")", "2027-01-01"),
    ] {
        assert_eq!(s(e), want, "{}", e);
    }
}

#[test]
fn the_clock() {
    let today = s("date.today()");
    assert!(today.len() == 10 && quillo::dates::parse_date(&today).is_some(), "{}", today);
    let now = s("date.now()");
    assert!(now.len() == 20 && now.ends_with('Z') && now.starts_with(&today[..4]), "{}", now);
    let paris = s("date.now(\"Europe/Paris\")");
    assert!(paris.ends_with("+01:00") || paris.ends_with("+02:00"), "{}", paris);
    // Today in a zone is the date of now there (unless midnight passed between).
    let (a, b) = (s("date.today(\"Pacific/Auckland\")"), s("date.date_in(date.now(), \"Pacific/Auckland\")"));
    assert!(a == b || a < b, "{} {}", a, b);
}

#[test]
fn business_days_around_holidays_and_month_ends() {
    // cal: Christmas (Friday), Boxing Day's Monday, New Year's Day (Friday).
    for (e, want) in [
        ("date.adjust(\"2026-12-25\", \"following\", cal)", "2026-12-29"),
        ("date.adjust(\"2026-12-25\", \"preceding\", cal)", "2026-12-24"),
        ("date.adjust(\"2026-12-25\", \"modified_following\", cal)", "2026-12-29"),
        ("date.adjust(\"2026-12-24\", \"following\", cal)", "2026-12-24"),
        ("date.adjust(\"2026-12-25\", \"unadjusted\", cal)", "2026-12-25"),
        // Saturday the 31st: the following business day is in November, so back to Friday.
        ("date.adjust(\"2026-10-31\", \"following\")", "2026-11-02"),
        ("date.adjust(\"2026-10-31\", \"modified_following\")", "2026-10-30"),
        ("date.adjust(\"2026-10-31\")", "2026-11-02"),
        ("date.adjust(\"2027-01-30\", \"modified_following\")", "2027-01-29"),
        // Sunday the 1st: the preceding business day is in October, so on to Monday.
        ("date.adjust(\"2026-11-01\", \"preceding\")", "2026-10-30"),
        ("date.adjust(\"2026-11-01\", \"modified_preceding\")", "2026-11-02"),
        ("date.adjust(\"2027-01-01\", \"modified_preceding\", cal)", "2027-01-04"),
        ("date.adjust(\"2027-01-01\", \"preceding\", cal)", "2026-12-31"),
        ("date.add_business_days(\"2026-12-24\", 1, cal)", "2026-12-29"),
        ("date.add_business_days(\"2026-12-24\", 3, cal)", "2026-12-31"),
        ("date.add_business_days(\"2026-12-24\", 4, cal)", "2027-01-04"),
        ("date.add_business_days(\"2026-12-29\", -1, cal)", "2026-12-24"),
        ("date.add_business_days(\"2027-01-04\", -4, cal)", "2026-12-24"),
        // T+2 from a Thursday; from a Saturday.
        ("date.add_business_days(\"2026-10-29\", 2)", "2026-11-02"),
        ("date.add_business_days(\"2026-10-31\", 1)", "2026-11-02"),
        ("date.add_business_days(\"2026-10-31\", 0)", "2026-11-02"),
        ("date.add_business_days(\"2026-10-27\", 0)", "2026-10-27"),
        ("date.add_business_days(\"2026-11-01\", -1)", "2026-10-30"),
        ("date.add_business_days(\"2026-10-26\", 10)", "2026-11-09"),
        // A Friday and Saturday weekend.
        ("date.add_business_days(\"2026-10-29\", 1, { weekend: [5, 6] })", "2026-11-01"),
    ] {
        assert_eq!(s(e), want, "{}", e);
    }
    for (e, want) in [
        ("date.business_days_between(\"2026-12-21\", \"2027-01-04\", cal)", "7"),
        ("date.business_days_between(\"2027-01-04\", \"2026-12-21\", cal)", "-7"),
        ("date.business_days_between(\"2026-10-01\", \"2026-11-01\")", "22"),
        ("date.business_days_between(\"2026-10-27\", \"2026-10-27\")", "0"),
        ("date.business_days_between(\"2026-01-01\", \"2027-01-01\")", "261"),
        ("[date.is_business_day(\"2026-12-25\", cal), date.is_business_day(\"2026-12-24\", cal), date.is_business_day(\"2026-10-31\"), date.is_business_day(\"2026-10-30\", { weekend: [5, 6] }), date.is_business_day(\"2026-11-01\", { weekend: [5, 6] }), date.is_business_day(\"2026-10-31\", { weekend: [] })]", "[false, true, false, false, true, true]"),
    ] {
        assert_eq!(eval(e), want, "{}", e);
    }
}

/// Year fractions are compared exactly: these are the quotients the
/// definitions give.
#[test]
fn day_counts_against_reference_values() {
    // ISDA's examples for ACT/ACT ("EMU and market conventions", 1998).
    for (a, b, want) in [
        ("2003-11-01", "2004-05-01", 61.0 / 365.0 + 121.0 / 366.0),
        ("1999-02-01", "1999-07-01", 150.0 / 365.0),
        ("2002-08-15", "2003-07-15", 334.0 / 365.0),
        ("1999-07-30", "2000-01-30", 155.0 / 365.0 + 29.0 / 366.0),
        ("2000-01-30", "2000-06-30", 152.0 / 366.0),
        ("1999-11-30", "2000-04-30", 32.0 / 365.0 + 120.0 / 366.0),
    ] {
        let got = float(&format!("date.year_fraction(\"{}\", \"{}\", \"ACT/ACT\")", a, b));
        assert!((got - want).abs() < 1e-15, "ACT/ACT {} {}: {} not {}", a, b, got, want);
    }
    assert_eq!(float("date.year_fraction(\"2003-11-01\", \"2004-05-01\", \"ACT/ACT ISDA\")"), 0.49772438056740775);
    // Several years: the whole years between count one each.
    let got = float("date.year_fraction(\"2023-07-01\", \"2026-03-01\", \"Actual/Actual\")");
    assert!((got - (184.0 / 365.0 + 2.0 + 59.0 / 365.0)).abs() < 1e-15, "{}", got);
    // ACT/360 and ACT/365 Fixed: actual days over 360 or 365, leap year or not.
    assert_eq!(float("date.year_fraction(\"2026-01-15\", \"2026-07-15\", \"ACT/360\")"), 181.0 / 360.0);
    assert_eq!(float("date.year_fraction(\"2026-01-15\", \"2026-07-15\", \"ACT/365\")"), 181.0 / 365.0);
    assert_eq!(float("date.year_fraction(\"2024-01-01\", \"2025-01-01\", \"ACT/365F\")"), 366.0 / 365.0);
    assert_eq!(float("date.year_fraction(\"2024-01-01\", \"2025-01-01\", \"act/360\")"), 366.0 / 360.0);
    assert_eq!(eval("date.day_count(\"2024-01-01\", \"2025-01-01\", \"ACT/365 Fixed\")"), "366");
    // 30/360: the 31st and the end of February, by convention. Days counted
    // (bond basis ISDA 4.16(f), 30U/360 SIA, 30E/360 ISDA 4.16(g)).
    for (a, b, bond, us, euro) in [
        ("2006-08-20", "2007-02-20", 180, 180, 180),
        ("2007-02-15", "2007-05-31", 106, 106, 105),
        ("2007-01-31", "2007-02-28", 28, 28, 28),
        ("2007-02-28", "2007-03-31", 33, 30, 32),
        ("2007-02-28", "2008-02-29", 361, 360, 361),
        ("2008-02-29", "2008-03-31", 32, 30, 31),
        ("2006-08-31", "2007-02-28", 178, 178, 178),
        ("2007-03-31", "2007-08-31", 150, 150, 150),
        ("2007-03-30", "2007-08-31", 150, 150, 150),
        ("2007-03-29", "2007-08-31", 152, 152, 151),
        ("2008-02-29", "2009-02-28", 359, 360, 359),
    ] {
        for (conv, want) in [("30/360", bond), ("30U/360", us), ("30E/360", euro)] {
            let e = format!("[date.day_count(\"{}\", \"{}\", \"{}\"), date.year_fraction(\"{}\", \"{}\", \"{}\")]", a, b, conv, a, b, conv);
            let want_frac = want as f64 / 360.0;
            let got = eval(&e);
            let parts: Vec<&str> = got.trim_matches(['[', ']']).split(", ").collect();
            assert_eq!(parts[0], want.to_string(), "{}", e);
            assert_eq!(parts[1].parse::<f64>().unwrap(), want_frac, "{}", e);
        }
    }
    assert_eq!(float("date.year_fraction(\"2026-01-15\", \"2026-07-15\", \"30/360\")"), 0.5);
    assert_eq!(eval("date.day_count(\"2026-01-31\", \"2026-07-31\", \"Bond basis\")"), "180");
    // Backwards: the same, negative.
    assert_eq!(float("date.year_fraction(\"2004-05-01\", \"2003-11-01\", \"ACT/ACT\")"), -0.49772438056740775);
    assert_eq!(eval("date.day_count(\"2007-03-31\", \"2007-02-28\", \"30U/360\")"), "-30");
}

#[test]
fn easter() {
    for (y, want) in [
        (1818, "1818-03-22"),
        (1943, "1943-04-25"),
        (2000, "2000-04-23"),
        (2008, "2008-03-23"),
        (2011, "2011-04-24"),
        (2019, "2019-04-21"),
        (2024, "2024-03-31"),
        (2025, "2025-04-20"),
        (2026, "2026-04-05"),
        (2027, "2027-03-28"),
        (2038, "2038-04-25"),
        (2285, "2285-03-22"),
    ] {
        assert_eq!(s(&format!("date.easter({})", y)), want, "{}", y);
    }
    // Holidays that follow from it.
    assert_eq!(s("date.add_days(date.easter(2026), -2)"), "2026-04-03");
    assert_eq!(s("date.add_days(date.easter(2026), 1)"), "2026-04-06");
    assert_eq!(s("date.add_days(date.easter(2026), 50)"), "2026-05-25");
}

#[test]
fn formatting() {
    for (e, want) in [
        ("date.format(\"2026-10-27\", \"DD/MM/YYYY\")", "27/10/2026"),
        ("date.format(\"2026-03-05\", \"D.M.YY\")", "5.3.26"),
        ("date.format(\"2026-10-27\", \"dddd, MMMM D, YYYY\")", "Tuesday, October 27, 2026"),
        ("date.format(\"2026-10-27\", \"ddd D MMM\")", "Tue 27 Oct"),
        ("date.format(\"2026-10-27\", \"[Due] D MMM\")", "Due 27 Oct"),
        ("date.format(\"2026-10-27T09:05:03+01:00\", \"YYYY-MM-DD HH:mm:ss Z\")", "2026-10-27 09:05:03 +01:00"),
        ("date.format(\"2026-10-27T00:30:00Z\", \"h:mm A\")", "12:30 AM"),
        ("date.format(\"2026-10-27T12:30:00Z\", \"h:mm A\")", "12:30 PM"),
        ("date.format(\"2026-10-27T21:30:00Z\", \"hh:mm A [UTC]\")", "09:30 PM UTC"),
        ("date.format(\"2026-10-27\", { locale: \"en-US\", style: \"short\" })", "10/27/26"),
        ("date.format(\"2026-10-27\", { locale: \"en\" })", "Oct 27, 2026"),
        ("date.format(\"2026-10-27\", { locale: \"en-GB\", style: \"long\" })", "27 October 2026"),
        ("date.format(\"2026-10-27\", { locale: \"de-DE\", style: \"short\" })", "27.10.26"),
        ("date.format(\"2026-10-27\", { locale: \"de-DE\", style: \"long\" })", "27. Oktober 2026"),
        ("date.format(\"2026-03-27\", { locale: \"de_AT\", style: \"full\" })", "Freitag, 27. März 2026"),
        ("date.format(\"2026-10-27\", { locale: \"fr-FR\", style: \"medium\" })", "27 oct. 2026"),
        ("date.format(\"2026-08-01\", { locale: \"fr-CA\", style: \"full\" })", "samedi 1 août 2026"),
        ("date.format(\"2026-10-27\", { locale: \"es-ES\", style: \"long\" })", "27 de octubre de 2026"),
        ("date.format(\"2026-10-27\", { locale: \"it-IT\", style: \"full\" })", "martedì 27 ottobre 2026"),
        ("date.format(\"2026-10-27\", { locale: \"nl-NL\", style: \"short\" })", "27-10-2026"),
        ("date.format(\"2026-10-27\", { locale: \"pt-BR\", style: \"full\" })", "terça-feira, 27 de outubro de 2026"),
        ("date.format(\"2026-10-27\", { locale: \"de\", pattern: \"dddd, D. MMM\" })", "Dienstag, 27. Okt."),
        // Unknown: English.
        ("date.format(\"2026-10-27\", { locale: \"sv-SE\", style: \"long\" })", "October 27, 2026"),
        // A date-time in a zone: move it there first.
        ("date.format(date.to_zone(\"2026-10-27T14:30:00Z\", \"Europe/Paris\"), \"DD/MM/YYYY HH:mm\")", "27/10/2026 15:30"),
    ] {
        assert_eq!(s(e), want, "{}", e);
    }
}

#[test]
fn parsing() {
    for (e, want) in [
        ("date.parse(\"2026-10-27\")", "2026-10-27"),
        ("date.parse(\" 2026-10-27 \")", "2026-10-27"),
        ("date.parse(\"2026-10-27T14:30:00Z\")", "2026-10-27T14:30:00Z"),
        ("date.parse(\"2026-10-27 14:30:00+01\")", "2026-10-27T14:30:00+01:00"),
        ("date.parse(\"2026-10-27t14:30+0530\")", "2026-10-27T14:30:00+05:30"),
        ("date.parse(\"2026-10-27T14:30:00.5+00:00\")", "2026-10-27T14:30:00.5Z"),
        ("date.parse(\"2026-10-27T14:30:00,25z\")", "2026-10-27T14:30:00.25Z"),
        ("date.parse(\"27/10/2026\", \"DD/MM/YYYY\")", "2026-10-27"),
        ("date.parse(\"5.3.26\", \"D.M.YY\")", "2026-03-05"),
        ("date.parse(\"261027\", \"YYMMDD\")", "2026-10-27"),
        ("date.parse(\"Oct 27, 2026\", \"MMM D, YYYY\")", "2026-10-27"),
        ("date.parse(\"tuesday 27 OCTOBER 2026\", \"dddd D MMMM YYYY\")", "2026-10-27"),
        ("date.parse(\"10/27/2026 2:05 PM\", \"MM/DD/YYYY h:mm A\")", "2026-10-27T14:05:00Z"),
        ("date.parse(\"10/27/2026 12:05 am\", \"MM/DD/YYYY h:mm A\")", "2026-10-27T00:05:00Z"),
        ("date.parse(\"2026-10-27 14:30 +0100\", \"YYYY-MM-DD HH:mm Z\")", "2026-10-27T14:30:00+01:00"),
        ("date.parse(\"Due 27/10/2026\", \"[Due] DD/MM/YYYY\")", "2026-10-27"),
    ] {
        assert_eq!(s(e), want, "{}", e);
    }
    for e in [
        "date.parse(\"2026-02-30\")",
        "date.parse(\"2026-10-27T14:30:00\")",
        "date.parse(\"27/10/2026\")",
        "date.parse(\"yesterday\")",
        "date.parse(\"31/02/2026\", \"DD/MM/YYYY\")",
        "date.parse(\"27/10/2026 extra\", \"DD/MM/YYYY\")",
        "date.parse(\"10/27/2026 13:05 PM\", \"MM/DD/YYYY h:mm A\")",
        "date.parse(\"10:30\", \"HH:mm\")",
    ] {
        assert!(eval(e).starts_with("err("), "{} gave {}", e, eval(e));
    }
    assert_eq!(eval("date.parse(\"2026-02-30\")"), "err(\"not a date or a date-time: \\\"2026-02-30\\\"\")");
    assert_eq!(eval("date.parse(\"31/02/2026\", \"DD/MM/YYYY\")"), "err(\"\\\"31/02/2026\\\" is not a date in the form \\\"DD/MM/YYYY\\\"\")");
    assert_eq!(s("date.parse(\"x\") ?? \"none\""), "none");
}

#[test]
fn malformed_input_is_an_error() {
    for (e, want) in [
        ("date.add_days(\"2026-02-30\", 1)", "`date.add_days` expects a date (YYYY-MM-DD) as argument 1, got \"2026-02-30\""),
        ("date.add_days(\"2026-1-5\", 1)", "`date.add_days` expects a date (YYYY-MM-DD) as argument 1, got \"2026-1-5\""),
        ("date.add_days(20261027, 1)", "`date.add_days` expects a date (YYYY-MM-DD) as argument 1, got int"),
        ("date.add_days(\"2026-10-27\", \"1\")", "`date.add_days` expects an int as argument 2, got \"1\""),
        ("date.add_days(\"2026-10-27\")", "`date.add_days` expects an int as argument 2, got nothing"),
        ("date.add_days(\"2026-10-27T14:30:00Z\", 1)", "`date.add_days` expects a date (YYYY-MM-DD) as argument 1, got the date-time \"2026-10-27T14:30:00Z\" (`date.date_in` gives its date)"),
        ("date.days_between(\"2026-10-27\", \"27/10/2026\")", "`date.days_between` expects a date (YYYY-MM-DD) as argument 2, got \"27/10/2026\""),
        ("date.to_zone(\"2026-10-27T14:30:00\", \"Europe/Paris\")", "`date.to_zone` expects a date-time (like 2026-10-27T14:30:00Z) as argument 1, got \"2026-10-27T14:30:00\""),
        ("date.to_zone(\"2026-10-27T14:30:00Z\", \"Mars/Olympus_Mons\")", "`date.to_zone`: unknown time zone \"Mars/Olympus_Mons\""),
        ("date.today(1)", "`date.today` expects a time zone (like \"Europe/Paris\") as argument 1, got int"),
        ("date.at(\"2026-10-27\", \"25:00\", \"UTC\")", "`date.at` expects a time (HH:MM or HH:MM:SS) as argument 2, got \"25:00\""),
        ("date.of(2026, 2, 29)", "`date.of`: there is no date 2026-2-29"),
        ("date.of(10000, 1, 1)", "`date.of`: there is no date 10000-1-1"),
        ("date.add_years(\"9999-06-01\", 1)", "`date.add_years`: the date is out of range (years 1 to 9999)"),
        ("date.add_days(\"0001-01-01\", -1)", "`date.add_days`: the date is out of range (years 1 to 9999)"),
        ("date.add_months(\"2026-10-27\", 99999999)", "`date.add_months`: the date is out of range (years 1 to 9999)"),
        ("date.easter(1500)", "`date.easter` is for Gregorian years (1583 to 9999), got 1500"),
        ("date.year_fraction(\"2026-01-01\", \"2026-07-01\", \"ACT/999\")", "`date.year_fraction`: unknown day count convention \"ACT/999\" (ACT/360, ACT/365, ACT/ACT, 30/360, 30U/360 or 30E/360)"),
        ("date.day_count(\"2026-01-01\", \"2026-07-01\")", "`date.day_count` expects a str as argument 3, got nothing"),
        ("date.adjust(\"2026-10-31\", \"nearest\")", "`date.adjust`: unknown convention \"nearest\" (following, modified_following, preceding, modified_preceding or unadjusted)"),
        ("date.adjust(\"2026-10-31\", \"following\", { holiday: [\"2026-12-25\"] })", "`date.adjust`: a calendar has `weekend` and `holidays`, not `holiday`"),
        ("date.is_business_day(\"2026-10-31\", { weekend: [0] })", "`date.is_business_day`: a calendar's weekend is days 1 (Monday) to 7 (Sunday), got int"),
        ("date.is_business_day(\"2026-10-31\", { weekend: [1, 2, 3, 4, 5, 6, 7] })", "`date.is_business_day`: a calendar's weekend cannot be the whole week"),
        ("date.is_business_day(\"2026-10-31\", { holidays: [\"25/12/2026\"] })", "`date.is_business_day`: a calendar's holidays are dates (YYYY-MM-DD), got \"25/12/2026\""),
        ("date.is_business_day(\"2026-10-31\", { holidays: \"2026-12-25\" })", "`date.is_business_day`: a calendar's holidays is a list, got \"2026-12-25\""),
        ("date.is_business_day(\"2026-10-31\", [6, 7])", "`date.is_business_day` expects a calendar ({ weekend, holidays }) as argument 2, got list"),
        ("date.format(\"2026-10-27\", \"HH:mm\")", "`date.format`: the pattern has a time (HH), but \"2026-10-27\" is a date"),
        ("date.format(\"2026-10-27\", { style: \"huge\" })", "`date.format`: style is short, medium, long or full, got \"huge\""),
        ("date.format(\"2026-10-27\", { lang: \"de\" })", "`date.format`: the options are locale, style and pattern, not `lang`"),
        ("date.format(\"2026-10-27\")", "`date.format` expects a pattern (like \"DD/MM/YYYY\") or options as argument 2, got nothing"),
        ("date.parts(\"soon\")", "`date.parts` expects a date or a date-time as argument 1, got \"soon\""),
        ("date.days_in_month(2026, 13)", "`date.days_in_month`: there is no month 13"),
    ] {
        assert_eq!(error(e), want, "{}", e);
    }
}

#[test]
fn checked_like_other_builtins() {
    assert!(check("let d = date.add_days(date.today(), 30)\nfn due(start: str) -> str = date.adjust(date.add_months(start, 1), \"modified_following\")\nprint(due(d), date.year_fraction(d, \"2027-01-01\", \"ACT/360\") * 0.05)\n").is_empty());
    let e = check("print(date.add_days(\"2026-10-27\", \"1\"))\nprint(date.nope(1))\nprint(date.easter())\n");
    assert!(e.iter().any(|m| m.contains("date.add_days")), "{:?}", e);
    assert!(e.iter().any(|m| m.contains("`date` has no function `nope`")), "{:?}", e);
    assert!(e.iter().any(|m| m.contains("date.easter")), "{:?}", e);
    // The browser has it too.
    assert!(check("component Due(d: str) = <p>{date.format(date.add_business_days(d, 2), \"D MMM\")}</p>\nroute GET / = <Due d=\"2026-10-27\" />\n").iter().all(|m| !m.contains("not available in the browser")));
    // Reading the clock is the `time` effect.
    let e = check("fn stamp() with db = date.now()\n");
    assert!(e.iter().any(|m| m.contains("time")), "{:?}", e);
    assert!(check("fn stamp() with time = date.now()\nfn later(d: str) with db = date.add_days(d, 1)\n").is_empty());
}

#[test]
fn a_program_s_own_date_shadows_the_module() {
    for src in [
        "let date = \"2026-10-27\"\nprint(date, len(date))\n",
        "let date = { parse: (s) => s }\nprint(date.parse(\"x\"))\n",
        "fn f(date: str) -> str = date\nfn g(date) = date.year\nprint(f(\"x\"), g({ year: 1 }))\n",
        "component Row(date: str) = <td>{date}</td>\nroute GET / = <Row date=\"2026-10-27\" />\n",
        "let rows = map([1], (date) => date + 1)\nprint(rows)\n",
    ] {
        let e = check(src);
        assert!(e.is_empty(), "{}: {:?}", src, e);
    }
    assert_eq!(eval("let date = { add_days: (d, n) => n }\ndate.add_days(\"2026-10-27\", 3)"), "3");
    assert_eq!(eval("let date = \"2026-10-27\"\ndate"), "\"2026-10-27\"");
}

#[test]
fn scripts_have_dates_but_not_the_clock() {
    assert_eq!(s("run_script(\"date.add_business_days(d, 2)\", { d: \"2026-10-30\" })"), "2026-11-03");
    let r = eval("run_script(\"date.today()\")");
    assert!(r.starts_with("err("), "{}", r);
    let r = eval("run_script(\"date.now()\")");
    assert!(r.starts_with("err("), "{}", r);
}

// ---- the browser ------------------------------------------------------------------------------

fn node_available() -> bool {
    Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// The server's and the browser's answers. A float that is whole shows as
/// 2.0 on the server and 2 in the browser, which cannot tell them apart.
fn both(body: &str) -> (String, String) {
    let src = format!("{}\nfn __t() {{\n{}\n}}\n", DATA, body);
    let mut it = Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap();
    let server = match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(_) => "returned".into(),
    };
    let js = quillo::client::test_script(&it, "__t").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let out = Command::new("node").arg("-e").arg(&js).output().expect("node");
    assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
    let browser = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (whole_floats(&server), browser)
}

fn whole_floats(s: &str) -> String {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'.' && b.get(i + 1) == Some(&b'0') && i > 0 && b[i - 1].is_ascii_digit() && !b.get(i + 2).map_or(false, |c| c.is_ascii_digit()) {
            i += 2;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap()
}

#[test]
fn the_browser_gives_the_same_answers() {
    if !node_available() {
        return;
    }
    let cases = [
        "[date.add_months(\"2026-01-31\", 1), date.add_months(\"2024-01-31\", 1), date.add_months(\"2026-05-31\", -3), date.add_years(\"2024-02-29\", 1), date.add_years(\"2000-02-29\", 100)]",
        "[date.end_of_month(\"2024-02-10\"), date.start_of_month(\"2026-10-27\"), date.months_between(\"2026-01-31\", \"2026-02-28\"), date.months_between(\"2026-03-31\", \"2026-01-31\"), date.months_between(\"2026-01-15\", \"2027-03-14\")]",
        "[date.add_days(\"2024-02-28\", 1), date.add_days(\"2026-10-27\", -10000), date.add_days(\"0001-01-01\", 3652058), date.days_between(\"1970-01-01\", \"2026-10-27\"), date.days_between(\"2000-01-01\", \"2100-01-01\")]",
        "[date.is_leap_year(1900), date.is_leap_year(2000), date.is_leap_year(\"2028-06-01\"), date.days_in_month(2024, 2), date.days_in_month(\"2026-04-15\")]",
        "[date.parts(\"2026-10-27\"), date.parts(\"2026-10-27T14:05:09+05:30\"), date.weekday(\"0001-01-01\"), date.weekday(\"9999-12-31\"), date.of(5, 12, 31)]",
        "[date.valid(\"2026-10-27\"), date.valid(\"2026-10-27T14:30:00.125-04:00\"), date.valid(\"2026-02-29\"), date.valid(\"2026-10-27T14:30:00\"), date.valid(20261027), date.valid(\"2026-10-27T14:30:00+00:09:21\")]",
        "[date.to_zone(\"2026-03-29T00:59:59Z\", \"Europe/Paris\"), date.to_zone(\"2026-03-29T01:00:00Z\", \"Europe/Paris\"), date.to_zone(\"2026-10-25T00:30:00Z\", \"Europe/Paris\"), date.to_zone(\"2026-10-25T01:30:00Z\", \"Europe/Paris\")]",
        "[date.to_zone(\"2026-03-08T06:59:59Z\", \"America/New_York\"), date.to_zone(\"2026-03-08T07:00:00Z\", \"America/New_York\"), date.to_zone(\"2026-11-01T05:30:00Z\", \"America/New_York\"), date.to_zone(\"2026-11-01T06:30:00Z\", \"America/New_York\")]",
        "[date.to_zone(\"2026-10-27T15:30:00.250+01:00\", \"Asia/Kolkata\"), date.to_zone(\"2026-10-27T10:30:00-04:00\", \"UTC\"), date.to_zone(\"2026-10-27T14:30:00Z\", \"+09:00\"), date.to_zone(\"2026-10-27T14:30:00Z\", \"Australia/Lord_Howe\"), date.to_zone(\"1990-06-01T12:00:00Z\", \"Europe/London\")]",
        "[date.at(\"2026-03-29\", \"02:30\", \"Europe/Paris\"), date.at(\"2026-10-25\", \"02:30\", \"Europe/Paris\"), date.at(\"2026-03-08\", \"02:30\", \"America/New_York\"), date.at(\"2026-11-01\", \"01:30\", \"America/New_York\"), date.at(\"2026-10-27\")]",
        "[date.date_in(\"2026-10-27T23:30:00Z\", \"Europe/Paris\"), date.date_in(\"2026-10-27T03:00:00Z\", \"America/New_York\"), date.date_in(\"2026-12-31T20:00:00-05:00\", \"Asia/Tokyo\"), date.date_in(\"2026-10-27T03:00:00+05:00\")]",
        "[date.adjust(\"2026-12-25\", \"following\", cal), date.adjust(\"2026-12-25\", \"preceding\", cal), date.adjust(\"2026-10-31\", \"modified_following\"), date.adjust(\"2026-11-01\", \"modified_preceding\"), date.adjust(\"2027-01-01\", \"modified_preceding\", cal), date.adjust(\"2026-10-31\")]",
        "[date.add_business_days(\"2026-12-24\", 4, cal), date.add_business_days(\"2027-01-04\", -4, cal), date.add_business_days(\"2026-10-31\", 0), date.add_business_days(\"2026-10-29\", 1, { weekend: [5, 6] }), date.add_business_days(\"2026-10-26\", 10)]",
        "[date.business_days_between(\"2026-12-21\", \"2027-01-04\", cal), date.business_days_between(\"2027-01-04\", \"2026-12-21\", cal), date.business_days_between(\"2026-01-01\", \"2027-01-01\"), date.is_business_day(\"2026-12-25\", cal), date.is_business_day(\"2026-10-30\", { weekend: [5, 6] })]",
        "[date.year_fraction(\"2003-11-01\", \"2004-05-01\", \"ACT/ACT\"), date.year_fraction(\"1999-07-30\", \"2000-01-30\", \"ACT/ACT ISDA\"), date.year_fraction(\"2023-07-01\", \"2026-03-01\", \"Actual/Actual\"), date.year_fraction(\"2004-05-01\", \"2003-11-01\", \"ACT/ACT\")]",
        "[date.year_fraction(\"2026-01-15\", \"2026-07-15\", \"ACT/360\"), date.year_fraction(\"2026-01-15\", \"2026-07-15\", \"ACT/365\"), date.year_fraction(\"2024-01-01\", \"2025-01-01\", \"ACT/365F\"), date.day_count(\"2024-01-01\", \"2025-01-01\", \"ACT/365 Fixed\")]",
        "map([[\"2007-02-28\", \"2007-03-31\"], [\"2007-02-28\", \"2008-02-29\"], [\"2008-02-29\", \"2008-03-31\"], [\"2007-03-29\", \"2007-08-31\"], [\"2007-03-31\", \"2007-02-28\"]], (p) => map([\"30/360\", \"30U/360\", \"30E/360\"], (c) => [date.day_count(p[0], p[1], c), date.year_fraction(p[0], p[1], c)]))",
        "map([1583, 1818, 2000, 2008, 2019, 2024, 2025, 2026, 2038, 2285, 9999], (y) => date.easter(y))",
        "[date.format(\"2026-10-27\", \"dddd, MMMM D, YYYY\"), date.format(\"2026-03-05\", \"D.M.YY ddd MMM\"), date.format(\"2026-10-27T00:30:00Z\", \"h:mm A\"), date.format(\"2026-10-27T21:30:00-03:30\", \"hh:mm A Z [UTC]\"), date.format(\"2026-10-27\", \"[Due] D MMM\")]",
        "map([\"en\", \"en-US\", \"en-GB\", \"de-DE\", \"de_AT\", \"fr-FR\", \"es-ES\", \"it-IT\", \"nl-NL\", \"pt-BR\", \"sv-SE\"], (l) => map([\"short\", \"medium\", \"long\", \"full\"], (s) => date.format(\"2026-03-01\", { locale: l, style: s })))",
        "map([\"de\", \"fr\", \"pt\"], (l) => date.format(\"2026-12-27\", { locale: l, pattern: \"ddd D MMM YYYY\" }))",
        "[date.parse(\"2026-10-27\"), date.parse(\"2026-10-27 14:30:00+01\"), date.parse(\"2026-10-27t14:30+0530\"), date.parse(\"2026-10-27T14:30:00.5+00:00\"), date.parse(\"2026-10-27T14:30:00,25z\"), date.parse(\"2026-02-30\"), date.parse(\"2026-10-27T14:30\")]",
        "[date.parse(\"27/10/2026\", \"DD/MM/YYYY\"), date.parse(\"261027\", \"YYMMDD\"), date.parse(\"tuesday 27 OCTOBER 2026\", \"dddd D MMMM YYYY\"), date.parse(\"10/27/2026 12:05 am\", \"MM/DD/YYYY h:mm A\"), date.parse(\"2026-10-27 14:30 +0100\", \"YYYY-MM-DD HH:mm Z\"), date.parse(\"31/02/2026\", \"DD/MM/YYYY\"), date.parse(\"x 27/10/2026\", \"DD/MM/YYYY\")]",
        "date.add_days(\"2026-02-30\", 1)",
        "date.add_days(\"2026-10-27T14:30:00Z\", 1)",
        "date.days_between(\"2026-10-27\", 5)",
        "date.to_zone(\"2026-10-27T14:30:00Z\", \"Mars/Olympus_Mons\")",
        "date.to_zone(\"2026-10-27T14:30:00\", \"UTC\")",
        "date.add_years(\"9999-06-01\", 1)",
        "date.year_fraction(\"2026-01-01\", \"2026-07-01\", \"ACT/999\")",
        "date.adjust(\"2026-10-31\", \"nearest\")",
        "date.adjust(\"2026-10-31\", \"following\", { holiday: [] })",
        "date.is_business_day(\"2026-10-31\", { weekend: [1, 2, 3, 4, 5, 6, 7] })",
        "date.is_business_day(\"2026-10-31\", { holidays: [\"25/12/2026\"] })",
        "date.format(\"2026-10-27\", \"HH:mm\")",
        "date.format(\"2026-10-27\", { style: \"huge\" })",
        "date.of(2026, 2, 29)",
        "date.easter(1500)",
        "date.at(\"2026-10-27\", \"25:00\")",
    ];
    for e in cases {
        let (s, b) = both(e);
        assert_eq!(s, b, "{}", e);
    }
    // The clock: the same shape (and today, unless midnight passed between).
    let (s, b) = both("[len(date.today()), len(date.now()), len(date.now(\"Asia/Kolkata\")), date.today(\"Pacific/Kiritimati\") > date.today(\"Pacific/Pago_Pago\")]");
    assert_eq!(s, b);
    assert_eq!(s, "[10, 20, 25, true]");
    let (s, b) = both("date.today(\"Europe/Paris\")");
    assert!(s == b || s < b, "{} {}", s, b);
}
