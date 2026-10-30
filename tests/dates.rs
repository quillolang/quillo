//! The `date` module: the calendar, time zones, formatting and reading text,
//! and what each function says when it is given the wrong thing.

use quillo::eval_str;

fn ev(src: &str) -> String {
    match eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

fn ev_err(src: &str) -> String {
    match eval_str(src) {
        Ok(v) => panic!("expected an error, got {}\n--- source ---\n{}", v.repr(), src),
        Err(e) => e.to_string(),
    }
}

/// `src`'s value as the text a program would see, unquoted.
fn text(src: &str) -> String {
    ev(src).trim_matches('"').to_string()
}

#[track_caller]
fn fails(src: &str, wanted: &str) {
    let e = ev_err(src);
    assert!(e.contains(wanted), "{}\n  wanted: {}\n  got: {}", src, wanted, e);
}

#[test]
fn a_date_is_made_from_its_parts_and_checked() {
    assert_eq!(text("date.of(2026, 10, 27)"), "2026-10-27");
    assert_eq!(text("date.of(2024, 2, 29)"), "2024-02-29");
    assert_eq!(text("date.of(1, 1, 1)"), "0001-01-01");
    // A whole float is an int: what `floor` and division give.
    assert_eq!(text("date.of(2024.0, 2.0, 29.0)"), "2024-02-29");
    fails("date.of(2026, 2, 29)", "`date.of`: there is no date 2026-2-29");
    fails("date.of(2026, 13, 1)", "there is no date 2026-13-1");
    fails("date.of(2026, 0, 1)", "there is no date 2026-0-1");
    fails("date.of(2026, 4, 31)", "there is no date 2026-4-31");
    fails("date.of(0, 1, 1)", "there is no date 0-1-1");
    fails("date.of(10000, 1, 1)", "there is no date 10000-1-1");
    fails("date.of(\"x\", 1, 1)", "`date.of` expects an int as argument 1, got \"x\"");
    fails("date.of(2026, 1.5, 1)", "`date.of` expects an int as argument 2, got float");
    fails("date.of(2026, 1)", "`date.of` expects an int as argument 3, got nothing");
}

#[test]
fn valid_says_whether_text_is_a_date_or_a_date_time() {
    for good in [
        "2026-10-27",
        "2024-02-29",
        "0001-01-01",
        "9999-12-31",
        "2026-10-27T14:30:00Z",
        "2026-10-27T14:30:00+01:00",
        "2026-10-27T14:30:00-04:00",
        "2026-10-27T14:30:00.250Z",
        "2026-10-27T14:30:00.123456789+01:00",
        "1900-01-01T00:00:00+00:09:21",
    ] {
        assert_eq!(ev(&format!("date.valid({:?})", good)), "true", "{}", good);
    }
    for bad in [
        "",
        "soon",
        "2026-02-29",
        "2026-13-01",
        "2026-00-10",
        "2026-04-31",
        "0000-01-01",
        "2026/10/27",
        "2026-1-27",
        "27-10-2026",
        "+026-10-27",
        "abcd-10-27",
        "2026-10-27 14:30:00Z",
        "2026-10-27t14:30:00Z",
        "2026-10-27T14:30:00",
        "2026-10-27T14:30Z",
        "2026-10-27T24:00:00Z",
        "2026-10-27T14:60:00Z",
        "2026-10-27T14:30:60Z",
        "2026-10-27T14-30-00Z",
        "2026-02-30T14:30:00Z",
        "2026-10-27T14:30:00.Z",
        "2026-10-27T14:30:00.1234567890Z",
        "2026-10-27T14:30:00+24:00",
        "2026-10-27T14:30:00+01:60",
        "2026-10-27T14:30:00+01:00:60",
        "2026-10-27T14:30:00+01:00x",
        "2026-10-27T14:30:00+0100",
        "2026-10-27T14:30:00+01-00",
        "2026-10-27T14:30:00+01:00-00",
        "2026-10-27T14:30:00z",
        "2026-10-27T14:30:00é",
    ] {
        assert_eq!(ev(&format!("date.valid({:?})", bad)), "false", "{}", bad);
    }
    assert_eq!(ev("date.valid(20261027)"), "false");
    assert_eq!(ev("date.valid(null)"), "false");
    assert_eq!(ev("date.valid()"), "false");
}

#[test]
fn valid_can_ask_for_one_kind() {
    let (d, t) = ("\"2026-10-27\"", "\"2026-10-27T14:30:00Z\"");
    assert_eq!(ev(&format!("[date.valid({d}, \"date\"), date.valid({t}, \"date\")]")), "[true, false]");
    assert_eq!(ev(&format!("[date.valid({d}, \"date-time\"), date.valid({t}, \"date-time\")]")), "[false, true]");
    assert_eq!(ev(&format!("[date.valid({d}, null), date.valid({t}, null)]")), "[true, true]");
    fails(&format!("date.valid({d}, \"time\")"), "`date.valid` takes \"date\" or \"date-time\" as argument 2, got \"time\"");
    fails(&format!("date.valid({d}, 3)"), "as argument 2, got int");
}

#[test]
fn parts_give_the_calendar_and_a_date_time_its_clock() {
    assert_eq!(ev("date.parts(\"2026-10-27\")"), "{ year: 2026, month: 10, day: 27, weekday: 2 }");
    assert_eq!(
        ev("date.parts(\"2026-10-27T14:05:09+05:30\")"),
        "{ year: 2026, month: 10, day: 27, weekday: 2, hour: 14, minute: 5, second: 9, offset: \"+05:30\" }"
    );
    assert_eq!(ev("date.parts(\"2026-01-02T00:00:00Z\").offset"), "\"Z\"");
    assert_eq!(ev("date.parts(\"1969-12-31T23:59:59-00:30\").second"), "59");
    fails("date.parts(\"x\")", "`date.parts` expects a date or a date-time as argument 1, got \"x\"");
    fails("date.parts(3)", "`date.parts` expects a date or a date-time as argument 1, got int");
    fails("date.parts()", "as argument 1, got nothing");
}

#[test]
fn weekdays_count_from_monday() {
    assert_eq!(ev("date.weekday(\"2026-10-26\")"), "1");
    assert_eq!(ev("date.weekday(\"2026-11-01\")"), "7");
    assert_eq!(ev("date.weekday(\"1970-01-01\")"), "4");
    assert_eq!(ev("date.weekday(\"1969-12-31\")"), "3");
    assert_eq!(ev("date.weekday(\"0001-01-01\")"), "1");
    // A date-time's weekday is that of the date as written.
    assert_eq!(ev("date.weekday(\"2026-10-27T23:00:00-05:00\")"), "2");
    fails("date.weekday(\"Tuesday\")", "`date.weekday` expects a date or a date-time");
}

#[test]
fn leap_years_follow_the_gregorian_rule() {
    assert_eq!(ev("[2024, 2026, 1900, 2000, 2024.0].map(y => date.is_leap_year(y))"), "[true, false, false, true, true]");
    assert_eq!(ev("date.is_leap_year(\"2024-02-01\")"), "true");
    assert_eq!(ev("date.is_leap_year(\"2023-12-31T00:00:00Z\")"), "false");
    fails("date.is_leap_year(\"x\")", "`date.is_leap_year` expects a date or a date-time as argument 1, got \"x\"");
    fails("date.is_leap_year(null)", "`date.is_leap_year` expects an int as argument 1, got null");
}

#[test]
fn days_in_month_takes_a_date_or_a_year_and_month() {
    assert_eq!(ev("date.days_in_month(\"2024-02-10\")"), "29");
    assert_eq!(ev("date.days_in_month(\"2026-02-10\")"), "28");
    assert_eq!(ev("date.days_in_month(\"2026-11-05T12:00:00Z\")"), "30");
    assert_eq!(ev("[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12].map(m => date.days_in_month(2026, m))"), "[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]");
    fails("date.days_in_month(2026, 13)", "`date.days_in_month`: there is no month 13");
    fails("date.days_in_month(2026, 0)", "there is no month 0");
    fails("date.days_in_month(2026)", "`date.days_in_month` expects an int as argument 2, got nothing");
    fails("date.days_in_month(\"soon\")", "expects a date or a date-time as argument 1, got \"soon\"");
}

#[test]
fn add_days_crosses_months_and_years() {
    assert_eq!(text("date.add_days(\"2026-12-31\", 1)"), "2027-01-01");
    assert_eq!(text("date.add_days(\"2024-02-28\", 1)"), "2024-02-29");
    assert_eq!(text("date.add_days(\"2026-03-01\", -1)"), "2026-02-28");
    assert_eq!(text("date.add_days(\"1970-01-01\", -1)"), "1969-12-31");
    assert_eq!(text("date.add_days(\"2026-10-27\", 0)"), "2026-10-27");
    assert_eq!(text("date.add_days(\"2026-10-27\", 7.0)"), "2026-11-03");
    fails("date.add_days(\"9999-12-31\", 1)", "`date.add_days`: the date is out of range (years 1 to 9999)");
    fails("date.add_days(\"0001-01-01\", -1)", "out of range");
    fails("date.add_days(\"2026-01-01\", 5000000)", "out of range");
    fails("date.add_days(\"2026-01-01\", -5000000)", "out of range");
    fails("date.add_days(\"2026-01-01\", 1.5)", "`date.add_days` expects an int as argument 2, got float");
    fails("date.add_days(\"2026-01-01\", 1.0e16)", "expects an int as argument 2, got float");
    fails("date.add_days(\"2026-01-01\", \"1\")", "expects an int as argument 2, got \"1\"");
}

#[test]
fn the_functions_on_dates_refuse_anything_else() {
    fails(
        "date.add_days(\"2026-10-27T14:30:00Z\", 1)",
        "`date.add_days` expects a date (YYYY-MM-DD) as argument 1, got the date-time \"2026-10-27T14:30:00Z\" (`date.date_in` gives its date)",
    );
    fails("date.add_days(\"27/10/2026\", 1)", "`date.add_days` expects a date (YYYY-MM-DD) as argument 1, got \"27/10/2026\"");
    fails("date.add_months(20261027, 1)", "`date.add_months` expects a date (YYYY-MM-DD) as argument 1, got int");
    fails("date.add_years(null, 1)", "`date.add_years` expects a date (YYYY-MM-DD) as argument 1, got null");
    fails("date.start_of_month()", "`date.start_of_month` expects a date (YYYY-MM-DD) as argument 1, got nothing");
    fails("date.end_of_month(\"x\")", "`date.end_of_month` expects a date");
    fails("date.days_between(\"2026-10-27\", \"x\")", "`date.days_between` expects a date (YYYY-MM-DD) as argument 2, got \"x\"");
    fails("date.months_between(\"x\", \"2026-10-27\")", "`date.months_between` expects a date (YYYY-MM-DD) as argument 1, got \"x\"");
}

#[test]
fn add_months_clamps_to_the_end_of_the_month() {
    assert_eq!(text("date.add_months(\"2026-01-31\", 1)"), "2026-02-28");
    assert_eq!(text("date.add_months(\"2024-01-31\", 1)"), "2024-02-29");
    assert_eq!(text("date.add_months(\"2026-12-15\", 1)"), "2027-01-15");
    assert_eq!(text("date.add_months(\"2026-01-15\", -1)"), "2025-12-15");
    assert_eq!(text("date.add_months(\"2026-03-31\", -1)"), "2026-02-28");
    assert_eq!(text("date.add_months(\"2026-01-31\", 13)"), "2027-02-28");
    assert_eq!(text("date.add_months(\"2026-05-31\", 1)"), "2026-06-30");
    fails("date.add_months(\"9999-12-01\", 1)", "`date.add_months`: the date is out of range");
    fails("date.add_months(\"0001-01-01\", -1)", "out of range");
    fails("date.add_months(\"2026-01-01\", 120001)", "out of range");
    fails("date.add_months(\"2026-01-01\", -120001)", "out of range");
}

#[test]
fn add_years_keeps_the_day_where_it_can() {
    assert_eq!(text("date.add_years(\"2024-02-29\", 1)"), "2025-02-28");
    assert_eq!(text("date.add_years(\"2024-02-29\", 4)"), "2028-02-29");
    assert_eq!(text("date.add_years(\"2026-10-27\", -18)"), "2008-10-27");
    fails("date.add_years(\"9000-01-01\", 1000)", "`date.add_years`: the date is out of range");
    fails("date.add_years(\"2026-01-01\", 10001)", "out of range");
    fails("date.add_years(\"2026-01-01\", -10001)", "out of range");
}

#[test]
fn days_between_is_b_minus_a() {
    assert_eq!(ev("date.days_between(\"2026-11-05\", \"2026-11-14\")"), "9");
    assert_eq!(ev("date.days_between(\"2026-11-14\", \"2026-11-05\")"), "-9");
    assert_eq!(ev("date.days_between(\"2024-01-01\", \"2025-01-01\")"), "366");
    assert_eq!(ev("date.days_between(\"0001-01-01\", \"9999-12-31\")"), "3652058");
}

#[test]
fn months_between_counts_whole_months() {
    assert_eq!(ev("date.months_between(\"2026-01-31\", \"2026-02-28\")"), "1");
    assert_eq!(ev("date.months_between(\"2026-01-31\", \"2026-02-27\")"), "0");
    assert_eq!(ev("date.months_between(\"2026-01-15\", \"2026-03-14\")"), "1");
    assert_eq!(ev("date.months_between(\"2026-01-15\", \"2026-03-15\")"), "2");
    assert_eq!(ev("date.months_between(\"2026-10-01\", \"2026-10-31\")"), "0");
    assert_eq!(ev("date.months_between(\"2024-02-29\", \"2025-02-28\")"), "12");
    assert_eq!(ev("date.months_between(\"2026-03-15\", \"2026-01-15\")"), "-2");
    assert_eq!(ev("date.months_between(\"2026-02-28\", \"2026-01-31\")"), "-1");
}

#[test]
fn start_and_end_of_a_month() {
    assert_eq!(text("date.start_of_month(\"2026-10-27\")"), "2026-10-01");
    assert_eq!(text("date.end_of_month(\"2026-10-27\")"), "2026-10-31");
    assert_eq!(text("date.end_of_month(\"2024-02-01\")"), "2024-02-29");
    assert_eq!(text("date.end_of_month(\"2026-02-01\")"), "2026-02-28");
    assert_eq!(text("date.end_of_month(\"2026-06-30\")"), "2026-06-30");
}

#[test]
fn to_zone_is_the_same_instant_at_the_zones_offset() {
    assert_eq!(text("date.to_zone(\"2026-10-27T14:30:00Z\", \"America/New_York\")"), "2026-10-27T10:30:00-04:00");
    assert_eq!(text("date.to_zone(\"2026-10-27T14:30:00Z\", \"Europe/Paris\")"), "2026-10-27T15:30:00+01:00");
    assert_eq!(text("date.to_zone(\"2026-07-01T12:00:00Z\", \"Europe/Paris\")"), "2026-07-01T14:00:00+02:00");
    assert_eq!(text("date.to_zone(\"2026-10-27T14:30:00+01:00\", \"+05:30\")"), "2026-10-27T19:00:00+05:30");
    assert_eq!(text("date.to_zone(\"2026-10-27T23:30:00-05:00\", \"-05:00\")"), "2026-10-27T23:30:00-05:00");
    // Across midnight, the date moves with it.
    assert_eq!(text("date.to_zone(\"2026-12-31T23:30:00Z\", \"Asia/Tokyo\")"), "2027-01-01T08:30:00+09:00");
    // UTC under its names, and when no zone is given.
    for zone in ["\"UTC\"", "\"utc\"", "\"Z\"", "\"+00:00\"", "null"] {
        assert_eq!(text(&format!("date.to_zone(\"2026-10-27T15:30:00+01:00\", {})", zone)), "2026-10-27T14:30:00Z", "{}", zone);
    }
    assert_eq!(text("date.to_zone(\"2026-10-27T15:30:00+01:00\")"), "2026-10-27T14:30:00Z");
    // The fraction is kept as it was written, and an offset with seconds written whole.
    assert_eq!(text("date.to_zone(\"2026-10-27T14:30:00.250Z\", \"Europe/Paris\")"), "2026-10-27T15:30:00.250+01:00");
    assert_eq!(text("date.to_zone(\"2026-01-01T00:00:00Z\", \"+00:09:21\")"), "2026-01-01T00:09:21+00:09:21");
    assert_eq!(text("date.to_zone(\"2026-01-01T00:00:00Z\", \"-00:09:21\")"), "2025-12-31T23:50:39-00:09:21");
}

#[test]
fn to_zone_says_what_is_wrong() {
    let t = "\"2026-10-27T14:30:00Z\"";
    fails("date.to_zone(\"2026-10-27\", \"UTC\")", "`date.to_zone` expects a date-time (like 2026-10-27T14:30:00Z) as argument 1, got \"2026-10-27\"");
    fails("date.to_zone(5, \"UTC\")", "`date.to_zone` expects a date-time (like 2026-10-27T14:30:00Z) as argument 1, got int");
    fails(&format!("date.to_zone({t}, \"Mars/Olympus\")"), "`date.to_zone`: unknown time zone \"Mars/Olympus\"");
    fails(&format!("date.to_zone({t}, \"+99:00\")"), "unknown time zone \"+99:00\"");
    fails(&format!("date.to_zone({t}, \"../../etc/passwd\")"), "unknown time zone \"../../etc/passwd\"");
    fails(&format!("date.to_zone({t}, 5)"), "`date.to_zone` expects a time zone (like \"Europe/Paris\") as argument 2, got int");
    fails("date.to_zone(\"9999-12-31T23:00:00Z\", \"+02:00\")", "`date.to_zone`: the date is out of range");
    fails("date.to_zone(\"0001-01-01T00:00:00Z\", \"-01:00\")", "out of range");
}

#[test]
fn date_in_is_the_calendar_date_in_a_zone() {
    assert_eq!(text("date.date_in(\"2026-10-27T23:30:00Z\", \"Asia/Tokyo\")"), "2026-10-28");
    assert_eq!(text("date.date_in(\"2026-10-27T01:00:00Z\", \"America/New_York\")"), "2026-10-26");
    assert_eq!(text("date.date_in(\"2026-10-27T23:30:00-05:00\")"), "2026-10-28");
    assert_eq!(text("date.date_in(\"2026-10-27T23:30:00-05:00\", \"-05:00\")"), "2026-10-27");
    fails("date.date_in(\"2026-10-27\", \"UTC\")", "`date.date_in` expects a date-time");
    fails("date.date_in(\"2026-10-27T00:00:00Z\", \"Nowhere\")", "`date.date_in`: unknown time zone \"Nowhere\"");
    fails("date.date_in(\"9999-12-31T23:30:00Z\", \"+02:00\")", "`date.date_in`: the date is out of range");
}

#[test]
fn at_names_the_instant_of_a_local_time() {
    assert_eq!(text("date.at(\"2026-10-27\", \"17:00\", \"America/New_York\")"), "2026-10-27T17:00:00-04:00");
    assert_eq!(text("date.at(\"2026-11-05\", \"15:00\", \"Europe/Lisbon\")"), "2026-11-05T15:00:00Z");
    assert_eq!(text("date.at(\"2026-10-27\", \"09:15:30\", \"+05:30\")"), "2026-10-27T09:15:30+05:30");
    assert_eq!(text("date.at(\"2026-10-27\", \"09:15\")"), "2026-10-27T09:15:00Z");
    assert_eq!(text("date.at(\"2026-10-27\")"), "2026-10-27T00:00:00Z");
    assert_eq!(text("date.at(\"2026-10-27\", null, \"+01:00\")"), "2026-10-27T00:00:00+01:00");
}

#[test]
fn at_settles_the_hours_clocks_skip_and_repeat() {
    // Paris went to summer time at 02:00 on 2026-03-29 and back at 03:00 on 2026-10-25.
    assert_eq!(text("date.at(\"2026-03-29\", \"01:00\", \"Europe/Paris\")"), "2026-03-29T01:00:00+01:00");
    assert_eq!(text("date.at(\"2026-03-29\", \"02:30\", \"Europe/Paris\")"), "2026-03-29T03:30:00+02:00");
    assert_eq!(text("date.at(\"2026-03-29\", \"12:00\", \"Europe/Paris\")"), "2026-03-29T12:00:00+02:00");
    assert_eq!(text("date.at(\"2026-10-25\", \"01:00\", \"Europe/Paris\")"), "2026-10-25T01:00:00+02:00");
    assert_eq!(text("date.at(\"2026-10-25\", \"02:30\", \"Europe/Paris\")"), "2026-10-25T02:30:00+02:00");
    assert_eq!(text("date.at(\"2026-10-25\", \"03:30\", \"Europe/Paris\")"), "2026-10-25T03:30:00+01:00");
}

#[test]
fn at_says_what_is_wrong() {
    let wrong = "`date.at` expects a time (HH:MM or HH:MM:SS) as argument 2, got";
    fails("date.at(\"2026-10-27\", \"25:00\")", &format!("{} \"25:00\"", wrong));
    fails("date.at(\"2026-10-27\", \"9:15\")", &format!("{} \"9:15\"", wrong));
    fails("date.at(\"2026-10-27\", \"09:15:\")", wrong);
    fails("date.at(\"2026-10-27\", 915)", &format!("{} int", wrong));
    fails("date.at(\"2026-10-27T00:00:00Z\", \"09:15\")", "`date.at` expects a date (YYYY-MM-DD) as argument 1, got the date-time");
    fails("date.at(\"2026-10-27\", \"09:15\", \"Nowhere\")", "`date.at`: unknown time zone");
    fails("date.at(\"2026-10-27\", \"09:15\", 3)", "`date.at` expects a time zone");
}

/// `offset_at` takes an offset of 0 for what jiff cannot hold, which ends
/// about two days before the calendar does: the zone is silently dropped.
#[test]
#[ignore = "finding: in the last two days of 9999 a named zone's offset is taken as 0, so the time is written as UTC"]
fn the_calendars_last_day_keeps_a_zones_offset() {
    assert_eq!(text("date.to_zone(\"9999-12-31T12:00:00Z\", \"America/New_York\")"), "9999-12-31T07:00:00-05:00");
    assert_eq!(text("date.at(\"9999-12-31\", \"23:00\", \"America/New_York\")"), "9999-12-31T23:00:00-05:00");
}

#[test]
fn the_calendars_last_day_is_in_range_at_a_fixed_offset() {
    assert_eq!(text("date.at(\"9999-12-31\", \"23:00\", \"-05:00\")"), "9999-12-31T23:00:00-05:00");
    assert_eq!(text("date.to_zone(\"9999-12-31T12:00:00Z\", \"-05:00\")"), "9999-12-31T07:00:00-05:00");
    assert_eq!(text("date.date_in(\"9999-12-31T12:00:00Z\", \"Asia/Tokyo\")"), "9999-12-31");
}

/// `n.abs()` overflows on the smallest int: a panic where overflow is checked
/// (`cargo test`'s build); `unsigned_abs` would not.
#[test]
#[ignore = "finding: date.add_days and date.add_months panic on the smallest int in a build with overflow checks"]
fn the_smallest_int_is_out_of_range() {
    fails("date.add_days(\"2026-01-01\", -9223372036854775807 - 1)", "`date.add_days`: the date is out of range");
    fails("date.add_months(\"2026-01-01\", -9223372036854775807 - 1)", "`date.add_months`: the date is out of range");
}

#[test]
fn ms_is_what_now_gives() {
    assert_eq!(ev("date.ms(\"1970-01-01\")"), "0");
    assert_eq!(ev("date.ms(\"1970-01-02\")"), "86400000");
    assert_eq!(ev("date.ms(\"1969-12-31\")"), "-86400000");
    assert_eq!(ev("date.ms(\"1970-01-01T00:00:01Z\")"), "1000");
    assert_eq!(ev("date.ms(\"1970-01-01T01:00:00+01:00\")"), "0");
    assert_eq!(ev("date.ms(\"1969-12-31T23:59:59Z\")"), "-1000");
    // Fractions, to the millisecond and no further.
    assert_eq!(ev("date.ms(\"1970-01-01T00:00:01.250Z\")"), "1250");
    assert_eq!(ev("date.ms(\"1970-01-01T00:00:01.5Z\")"), "1500");
    assert_eq!(ev("date.ms(\"1970-01-01T00:00:01.123456Z\")"), "1123");
    assert_eq!(ev("date.ms(\"1969-12-31T23:59:59.500Z\")"), "-500");
    fails("date.ms(\"x\")", "`date.ms` expects a date or a date-time as argument 1, got \"x\"");
    let close = ev("let gap = date.ms(date.now()) - now()\n gap > -5000 && gap < 5000");
    assert_eq!(close, "true");
}

#[test]
fn from_ms_goes_back_to_a_date_time() {
    assert_eq!(text("date.from_ms(0)"), "1970-01-01T00:00:00Z");
    assert_eq!(text("date.from_ms(1999)"), "1970-01-01T00:00:01Z");
    assert_eq!(text("date.from_ms(-1)"), "1969-12-31T23:59:59Z");
    assert_eq!(text("date.from_ms(0, \"+01:00\")"), "1970-01-01T01:00:00+01:00");
    assert_eq!(text("date.from_ms(date.ms(\"2026-07-01T12:00:00Z\"), \"Europe/Lisbon\")"), "2026-07-01T13:00:00+01:00");
    fails("date.from_ms(1.5)", "`date.from_ms` expects an int as argument 1, got float");
    fails("date.from_ms(0, \"Nowhere\")", "`date.from_ms`: unknown time zone");
    fails("date.from_ms(99999999999999999)", "`date.from_ms`: the date is out of range");
    fails("date.from_ms(-99999999999999999)", "out of range");
    fails("date.from_ms(99999999999999999, \"Europe/Paris\")", "out of range");
}

#[test]
fn the_clock_reads_todays_date_in_a_zone() {
    assert_eq!(ev("date.valid(date.today(), \"date\")"), "true");
    assert_eq!(ev("date.valid(date.now(), \"date-time\")"), "true");
    assert_eq!(ev("date.now()").ends_with("Z\""), true);
    assert_eq!(ev("date.parts(date.now(\"+05:30\")).offset"), "\"+05:30\"");
    assert_eq!(ev("date.valid(date.now(\"Asia/Tokyo\"), \"date-time\")"), "true");
    // The two ends of the world's clocks are 26 hours apart.
    let apart = ev("date.days_between(date.today(\"-12:00\"), date.today(\"+14:00\"))");
    assert!(apart == "1" || apart == "2", "{}", apart);
    assert_eq!(ev("date.days_between(date.today(), date.date_in(date.now())) < 2"), "true");
    fails("date.today(\"Nowhere\")", "`date.today`: unknown time zone");
    fails("date.now(5)", "`date.now` expects a time zone");
}

#[test]
fn format_writes_a_pattern() {
    assert_eq!(text("date.format(\"2026-10-27\", \"DD/MM/YYYY\")"), "27/10/2026");
    assert_eq!(text("date.format(\"2026-03-05\", \"D M YY\")"), "5 3 26");
    assert_eq!(text("date.format(\"2026-03-05\", \"DD MM\")"), "05 03");
    assert_eq!(text("date.format(\"2026-10-27\", \"MMMM MMM\")"), "October Oct");
    assert_eq!(text("date.format(\"2026-10-27\", \"dddd ddd\")"), "Tuesday Tue");
    assert_eq!(text("date.format(\"0987-01-02\", \"YYYY\")"), "0987");
    assert_eq!(text("date.format(\"2005-01-02\", \"YY\")"), "05");
    // Anything else, and text in brackets, stays as it is.
    assert_eq!(text("date.format(\"2026-10-27\", \"[Due] YYYY! é\")"), "Due 2026! é");
    assert_eq!(text("date.format(\"2026-10-27\", \"[YYYY\")"), "YYYY");
    assert_eq!(text("date.format(\"2026-10-27\", \"[]YY\")"), "26");
    // A date-time's date is the one written, whatever its offset.
    assert_eq!(text("date.format(\"2026-10-27T23:00:00-05:00\", \"YYYY-MM-DD\")"), "2026-10-27");
}

#[test]
fn format_writes_the_time_of_a_date_time() {
    assert_eq!(text("date.format(\"2026-10-27T14:05:09+01:00\", \"YYYY-MM-DD HH:mm:ss Z\")"), "2026-10-27 14:05:09 +01:00");
    assert_eq!(text("date.format(\"2026-10-27T14:05:09Z\", \"H h hh A\")"), "14 2 02 PM");
    assert_eq!(text("date.format(\"2026-10-27T00:05:09Z\", \"H h hh A Z\")"), "0 12 12 AM Z");
    assert_eq!(text("date.format(\"2026-10-27T12:00:00Z\", \"H h hh A\")"), "12 12 12 PM");
    assert_eq!(text("date.format(\"2026-10-27T09:00:00Z\", \"H h hh A\")"), "9 9 09 AM");
    fails("date.format(\"2026-10-27\", \"HH:mm\")", "`date.format`: the pattern has a time (HH), but \"2026-10-27\" is a date");
    fails("date.format(\"2026-10-27\", \"YYYY Z\")", "the pattern has a time (Z), but");
    fails("date.format(\"2026-10-27\", \"h A\")", "the pattern has a time (h), but");
    fails("date.format(\"2026-10-27\", \"hh\")", "the pattern has a time (hh), but");
    fails("date.format(\"2026-10-27\", \"H\")", "the pattern has a time (H), but");
    fails("date.format(\"2026-10-27\", \"mm\")", "the pattern has a time (mm), but");
    fails("date.format(\"2026-10-27\", \"ss\")", "the pattern has a time (ss), but");
    fails("date.format(\"2026-10-27\", \"A\")", "the pattern has a time (A), but");
    assert_eq!(text("date.format(\"2026-10-27\", \"[A]\")"), "A");
}

#[test]
fn every_locale_has_its_four_styles() {
    let table = [
        ("en", ["10/27/26", "Oct 27, 2026", "October 27, 2026", "Tuesday, October 27, 2026"]),
        ("en-GB", ["27/10/2026", "27 Oct 2026", "27 October 2026", "Tuesday, 27 October 2026"]),
        ("de", ["27.10.26", "27.10.2026", "27. Oktober 2026", "Dienstag, 27. Oktober 2026"]),
        ("fr", ["27/10/2026", "27 oct. 2026", "27 octobre 2026", "mardi 27 octobre 2026"]),
        ("es", ["27/10/26", "27 oct 2026", "27 de octubre de 2026", "martes, 27 de octubre de 2026"]),
        ("it", ["27/10/26", "27 ott 2026", "27 ottobre 2026", "martedì 27 ottobre 2026"]),
        ("nl", ["27-10-2026", "27 okt 2026", "27 oktober 2026", "dinsdag 27 oktober 2026"]),
        ("pt", ["27/10/2026", "27 de out. de 2026", "27 de outubro de 2026", "terça-feira, 27 de outubro de 2026"]),
    ];
    for (locale, styles) in table {
        for (style, wanted) in ["short", "medium", "long", "full"].iter().zip(styles) {
            let got = text(&format!("date.format(\"2026-10-27\", {{ locale: {:?}, style: {:?} }})", locale, style));
            assert_eq!(got, wanted, "{} {}", locale, style);
        }
    }
}

#[test]
fn a_locale_is_found_by_its_tag_or_its_language() {
    let long = |l: &str| text(&format!("date.format(\"2026-10-27\", {{ locale: {:?}, style: \"long\" }})", l));
    assert_eq!(long("de-DE"), "27. Oktober 2026");
    assert_eq!(long("de-AT"), "27. Oktober 2026");
    assert_eq!(long("DE_de"), "27. Oktober 2026");
    assert_eq!(long(" fr "), "27 octobre 2026");
    assert_eq!(long("pt-BR"), "27 de outubro de 2026");
    assert_eq!(long("en_AU"), "27 October 2026");
    assert_eq!(long("en-CA"), "October 27, 2026");
    assert_eq!(long("ja"), "October 27, 2026");
    assert_eq!(long(""), "October 27, 2026");
}

#[test]
fn format_options_default_and_combine() {
    assert_eq!(text("date.format(\"2026-10-27\", {})"), "Oct 27, 2026");
    assert_eq!(text("date.format(\"2026-10-27\", { locale: null, style: null, pattern: null })"), "Oct 27, 2026");
    assert_eq!(text("date.format(\"2026-10-27\", { locale: \"de\" })"), "27.10.2026");
    assert_eq!(text("date.format(\"2026-10-27\", { pattern: \"YYYY\" })"), "2026");
    assert_eq!(text("date.format(\"2026-10-27\", { locale: \"de\", pattern: \"dddd, MMMM (ddd, MMM)\" })"), "Dienstag, Oktober (Di., Okt.)");
    // A pattern wins over a style.
    assert_eq!(text("date.format(\"2026-10-27\", { pattern: \"YYYY\", style: \"long\" })"), "2026");
    assert_eq!(text("date.format(\"2026-09-03\", { locale: \"en-GB\" })"), "3 Sept 2026");
    fails("date.format(\"2026-10-27\", { locale: 5 })", "`date.format`: locale is a str, like \"de-DE\", got int");
    fails("date.format(\"2026-10-27\", { lang: \"de\" })", "`date.format`: the options are locale, style and pattern, not `lang`");
    fails("date.format(\"2026-10-27\", { style: \"huge\" })", "`date.format`: style is short, medium, long or full, got \"huge\"");
    fails("date.format(\"2026-10-27\", { style: 5 })", "`date.format`: pattern and style are strs, got int");
    fails("date.format(\"2026-10-27\", { pattern: 5 })", "`date.format`: pattern and style are strs, got int");
    fails("date.format(\"2026-10-27\", 5)", "`date.format` expects a pattern (like \"DD/MM/YYYY\") or options as argument 2, got int");
    fails("date.format(\"2026-10-27\")", "or options as argument 2, got nothing");
    fails("date.format(\"x\", \"YYYY\")", "`date.format` expects a date or a date-time as argument 1, got \"x\"");
}

#[test]
fn parse_reads_iso_text_and_writes_it_the_modules_way() {
    assert_eq!(text("date.parse(\"2026-10-27\")"), "2026-10-27");
    assert_eq!(text("date.parse(\"  2026-10-27\\n\")"), "2026-10-27");
    assert_eq!(text("date.parse(\"2026-10-27\", null)"), "2026-10-27");
    assert_eq!(text("date.parse(\"2026-10-27T14:30:00Z\")"), "2026-10-27T14:30:00Z");
    assert_eq!(text("date.parse(\"2026-10-27t14:30:05z\")"), "2026-10-27T14:30:05Z");
    assert_eq!(text("date.parse(\"2026-10-27 14:30+01\")"), "2026-10-27T14:30:00+01:00");
    assert_eq!(text("date.parse(\"2026-10-27 14:30:15-0430\")"), "2026-10-27T14:30:15-04:30");
    assert_eq!(text("date.parse(\"2026-10-27T14:30:00-04:00\")"), "2026-10-27T14:30:00-04:00");
    assert_eq!(text("date.parse(\"2026-10-27T14:30:00+00:00\")"), "2026-10-27T14:30:00Z");
    assert_eq!(text("date.parse(\"2026-10-27T14:30:00.250+01:00\")"), "2026-10-27T14:30:00.250+01:00");
    assert_eq!(text("date.parse(\"2026-10-27T14:30:00,5Z\")"), "2026-10-27T14:30:00.5Z");
    assert_eq!(text("date.parse(\"2026-10-27T14:30.5Z\")"), "2026-10-27T14:30:00.5Z");
}

#[test]
fn parse_gives_an_error_value_for_what_is_not_a_date() {
    assert_eq!(ev("date.parse(\"soon\")"), "err(\"not a date or a date-time: \\\"soon\\\"\")");
    for bad in [
        "2026-02-30",
        "2026-02-30T14:30:00Z",
        "2026-10-27T14:30",
        "2026-10-27T14:30:",
        "2026-10-27T14:30:0Z",
        "2026-10-27T14:30:61Z",
        "2026-10-27T14:61:00Z",
        "2026-10-27T14-30Z",
        "2026-10-27_14:30:00Z",
        "2026-10-27T14:30:00.Z",
        "2026-10-27T14:30:00.1234567890Z",
        "2026-10-27T14:30:00x",
        "2026-10-27T14:30:00+1",
        "2026-10-27T14:30:00+0160",
        "2026-10-27T14:30:00+99",
        "2026-10-27T14:30:00é",
    ] {
        assert_eq!(ev(&format!("is_err(date.parse({:?}))", bad)), "true", "{}", bad);
    }
    // A date-time's year is that of its local date, whatever the offset.
    assert_eq!(text("date.parse(\"9999-12-31T23:00:00-05:00\")"), "9999-12-31T23:00:00-05:00");
    assert_eq!(text("date.parse(\"0001-01-01T00:00:00+01:00\")"), "0001-01-01T00:00:00+01:00");
    fails("date.parse(5)", "`date.parse` expects a str as argument 1, got int");
    fails("date.parse(\"2026-10-27\", 5)", "`date.parse` expects a pattern (like \"DD/MM/YYYY\") as argument 2, got int");
}

#[test]
fn parse_reads_by_a_pattern() {
    assert_eq!(text("date.parse(\"27/10/2026\", \"DD/MM/YYYY\")"), "2026-10-27");
    assert_eq!(text("date.parse(\"5/3/26\", \"D/M/YY\")"), "2026-03-05");
    assert_eq!(text("date.parse(\"15/12/99\", \"D/M/YY\")"), "2099-12-15");
    assert_eq!(text("date.parse(\"October 27, 2026\", \"MMMM D, YYYY\")"), "2026-10-27");
    assert_eq!(text("date.parse(\"oct 27 2026\", \"MMM D YYYY\")"), "2026-10-27");
    assert_eq!(text("date.parse(\"Tuesday 27 Oct 2026\", \"dddd D MMM YYYY\")"), "2026-10-27");
    assert_eq!(text("date.parse(\"TUE 27 Oct 2026\", \"ddd D MMM YYYY\")"), "2026-10-27");
    assert_eq!(text("date.parse(\"Due 2026-10-27\", \"[Due] YYYY-MM-DD\")"), "2026-10-27");
    assert_eq!(text("date.parse(\"  27/10/2026 \", \"DD/MM/YYYY\")"), "2026-10-27");
}

#[test]
fn parse_reads_a_time_by_a_pattern() {
    assert_eq!(text("date.parse(\"27/10/2026 14:30\", \"DD/MM/YYYY HH:mm\")"), "2026-10-27T14:30:00Z");
    assert_eq!(text("date.parse(\"27/10/2026 9:05:07\", \"DD/MM/YYYY H:mm:ss\")"), "2026-10-27T09:05:07Z");
    assert_eq!(text("date.parse(\"2026-10-27 2:30 PM\", \"YYYY-MM-DD h:mm A\")"), "2026-10-27T14:30:00Z");
    assert_eq!(text("date.parse(\"2026-10-27 02:30 am\", \"YYYY-MM-DD hh:mm A\")"), "2026-10-27T02:30:00Z");
    assert_eq!(text("date.parse(\"2026-10-27 12:30 AM\", \"YYYY-MM-DD h:mm A\")"), "2026-10-27T00:30:00Z");
    assert_eq!(text("date.parse(\"2026-10-27 12:30 PM\", \"YYYY-MM-DD h:mm A\")"), "2026-10-27T12:30:00Z");
    assert_eq!(text("date.parse(\"2026-10-27 12:30\", \"YYYY-MM-DD hh:mm\")"), "2026-10-27T12:30:00Z");
    assert_eq!(text("date.parse(\"2026-10-27 13:30 PM\", \"YYYY-MM-DD HH:mm A\")"), "2026-10-27T13:30:00Z");
    // A time with no hour is at midnight's hour.
    assert_eq!(text("date.parse(\"2026-10-27 30\", \"YYYY-MM-DD mm\")"), "2026-10-27T00:30:00Z");
    assert_eq!(text("date.parse(\"2026-10-27 14:30 +01:00\", \"YYYY-MM-DD HH:mm Z\")"), "2026-10-27T14:30:00+01:00");
    assert_eq!(text("date.parse(\"2026-10-27 14:30 -0430\", \"YYYY-MM-DD HH:mm Z\")"), "2026-10-27T14:30:00-04:30");
    assert_eq!(text("date.parse(\"2026-10-27 14:30 Z\", \"YYYY-MM-DD HH:mm Z\")"), "2026-10-27T14:30:00Z");
    assert_eq!(text("date.parse(\"2026-10-27 14:30 z\", \"YYYY-MM-DD HH:mm Z\")"), "2026-10-27T14:30:00Z");
}

#[test]
fn parse_by_a_pattern_gives_an_error_value_when_the_text_does_not_fit() {
    assert_eq!(ev("date.parse(\"x\", \"YYYY\")"), "err(\"\\\"x\\\" is not a date in the form \\\"YYYY\\\"\")");
    for (s, p) in [
        ("2026-02-30", "YYYY-MM-DD"),
        ("2026-10-27x", "YYYY-MM-DD"),
        ("2026/10/27", "YYYY-MM-DD"),
        ("2026", "YYYY"),
        ("10/27", "MM/DD"),
        ("Due", "[Due]"),
        ("26-10", "YYYY-MM"),
        ("Nonember 27 2026", "MMMM D YYYY"),
        ("Non 27 2026", "MMM D YYYY"),
        ("Someday 2026-10-27", "dddd YYYY-MM-DD"),
        ("Som 2026-10-27", "ddd YYYY-MM-DD"),
        ("2026-10-27 13:00 PM", "YYYY-MM-DD h:mm A"),
        ("2026-10-27 00:00 AM", "YYYY-MM-DD hh:mm A"),
        ("2026-10-27 24:00", "YYYY-MM-DD HH:mm"),
        ("2026-10-27 23:60", "YYYY-MM-DD HH:mm"),
        ("2026-10-27 23:59:60", "YYYY-MM-DD HH:mm:ss"),
        ("2026-10-27 2:30 XM", "YYYY-MM-DD h:mm A"),
        ("2026-10-27 14:30 +01", "YYYY-MM-DD HH:mm Z"),
        ("2026-10-27 14:30 éééééé", "YYYY-MM-DD HH:mm Z"),
        ("2026-10-27 14:30 ABCDE", "YYYY-MM-DD HH:mm Z"),
    ] {
        assert_eq!(ev(&format!("is_err(date.parse({:?}, {:?}))", s, p)), "true", "{} by {}", s, p);
    }
}

#[test]
fn what_is_written_reads_back() {
    let round = ev("[\"0001-01-01\", \"1600-02-29\", \"1969-12-31\", \"1970-01-01\", \"2000-02-29\", \"9999-12-31\"]
        .map(d => date.add_days(date.add_days(d, 0), 0) == d && date.parse(date.format(d, \"YYYY-MM-DD\")) == d)");
    assert_eq!(round, "[true, true, true, true, true, true]");
    assert_eq!(ev("let t = \"2026-10-27T14:30:00+05:30\"\ndate.ms(date.from_ms(date.ms(t), \"+05:30\")) == date.ms(t)"), "true");
}

#[test]
fn every_day_of_four_centuries_follows_the_one_before() {
    assert_eq!(quillo::dates::days(1970, 1, 1), 0);
    assert_eq!(quillo::dates::civil(0), (1970, 1, 1));
    let start = quillo::dates::days(1600, 1, 1);
    let mut last = quillo::dates::civil(start - 1);
    for z in start..start + 146_097 {
        let (y, m, d) = quillo::dates::civil(z);
        assert_eq!(quillo::dates::days(y, m, d), z);
        assert!(d == last.2 + 1 || (d == 1 && (m == last.1 + 1 || (m == 1 && y == last.0 + 1))), "{:?} after {:?}", (y, m, d), last);
        last = (y, m, d);
    }
    assert_eq!(quillo::dates::parse_date("2026-10-27"), Some(quillo::dates::days(2026, 10, 27)));
    assert_eq!(quillo::dates::parse_date("2026-10-32"), None);
}
